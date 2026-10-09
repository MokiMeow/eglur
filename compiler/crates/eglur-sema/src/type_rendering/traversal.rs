use super::*;

enum Form {
    Application(String),
    Prefix(&'static str),
    Tuple,
    Callable,
    Union,
    Projection(CanonicalTarget),
}

enum Step {
    Type(TypeId),
    Finish(Form, usize),
}

impl DiagnosticTypeRenderer<'_> {
    pub(crate) fn render_type(&self, root: TypeId) -> Result<String, DiagnosticTypeRenderingError> {
        let mut pending = vec![Step::Type(root)];
        let mut rendered = Vec::new();
        while let Some(step) = pending.pop() {
            let (form, children) = match step {
                Step::Type(ty) => match self.facts.types().kind(ty)? {
                    TypeKind::Never => {
                        rendered.push("Never".to_owned());
                        continue;
                    }
                    TypeKind::Primitive(primitive) => {
                        rendered.push(
                            self.render_target_path(&CanonicalTarget::Primitive(*primitive))?,
                        );
                        continue;
                    }
                    TypeKind::GenericParameter(parameter) => {
                        rendered.push(
                            self.facts
                                .generic_parameter_name(parameter)
                                .ok_or_else(|| {
                                    DiagnosticTypeRenderingError::UnavailableGenericParameter(
                                        parameter.clone(),
                                    )
                                })?
                                .to_owned(),
                        );
                        continue;
                    }
                    TypeKind::SelfType(target) => {
                        if self.owner_contains_target(target) {
                            rendered.push("Self".to_owned());
                        } else if let Some((owner, replacement)) = &self.self_binding
                            && owner == target
                        {
                            pending.push(Step::Type(*replacement));
                        } else {
                            return Err(DiagnosticTypeRenderingError::UnboundSelfType {
                                target: target.clone(),
                                owner: self.owner,
                            });
                        }
                        continue;
                    }
                    TypeKind::Nominal { head, substitution } => {
                        let name = match head {
                            NominalHead::Intrinsic(intrinsic) => intrinsic.spelling().to_owned(),
                            NominalHead::Declaration(target) => self.render_target_path(target)?,
                        };
                        (
                            Form::Application(name),
                            self.facts.types().substitution(*substitution)?.to_vec(),
                        )
                    }
                    TypeKind::Reference {
                        mutability,
                        referent,
                    } => {
                        let marker = if *mutability == ReferenceMutability::Mutable {
                            "&mut "
                        } else {
                            "&"
                        };
                        (Form::Prefix(marker), vec![*referent])
                    }
                    TypeKind::RawPointer {
                        mutability,
                        referent,
                    } => {
                        let marker = if *mutability == RawPointerMutability::Mutable {
                            "*mut "
                        } else {
                            "*const "
                        };
                        (Form::Prefix(marker), vec![*referent])
                    }
                    TypeKind::Tuple(elements) => (Form::Tuple, elements.to_vec()),
                    TypeKind::Callable { parameters, result } => {
                        let mut children = parameters.to_vec();
                        children.push(*result);
                        (Form::Callable, children)
                    }
                    TypeKind::ErrorUnion(members) => (Form::Union, members.to_vec()),
                    TypeKind::AssociatedProjection(projection) => (
                        Form::Projection(projection.associated().clone()),
                        vec![projection.base()],
                    ),
                },
                Step::Finish(form, start) => {
                    let mut children = rendered.split_off(start);
                    let text = match form {
                        Form::Application(name) => {
                            if children.is_empty() {
                                name
                            } else {
                                format!("{name}[{}]", children.join(", "))
                            }
                        }
                        Form::Prefix(marker) => format!("{marker}{}", children[0]),
                        Form::Tuple => {
                            let suffix = if children.len() == 1 { "," } else { "" };
                            format!("({}{suffix})", children.join(", "))
                        }
                        Form::Callable => {
                            let result = children.pop().expect("callable result is rendered last");
                            format!("Fn({}) -> {result}", children.join(", "))
                        }
                        Form::Union => {
                            children.sort();
                            match children.len() {
                                0 => {
                                    return Err(DiagnosticTypeRenderingError::InvalidUnionArity(0));
                                }
                                1 => children.pop().expect("one rendered union member"),
                                _ => format!("Union[{}]", children.join(", ")),
                            }
                        }
                        Form::Projection(associated) => {
                            let name = self.names.target_name(&associated).ok_or(
                                DiagnosticTypeRenderingError::MissingTargetName(associated),
                            )?;
                            format!("{}::{name}", children[0])
                        }
                    };
                    rendered.push(text);
                    continue;
                }
            };
            pending.push(Step::Finish(form, rendered.len()));
            pending.extend(children.into_iter().rev().map(Step::Type));
        }
        assert_eq!(rendered.len(), 1);
        Ok(rendered.pop().expect("root type is rendered"))
    }
}
