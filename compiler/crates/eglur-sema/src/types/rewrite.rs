use super::*;

#[derive(Clone, Copy)]
pub(super) enum LeafRewrite<'a> {
    Generic(&'a BTreeMap<GenericParameterId, TypeId>),
    OwnedSelf {
        owner: &'a CanonicalTarget,
        replacement: TypeId,
    },
}

enum RewriteStep {
    Visit(TypeId),
    Rebuild(TypeId),
    ProjectionArguments(TypeId),
}

impl CanonicalTypeStore {
    pub(super) fn rewrite(
        &mut self,
        root: TypeId,
        leaves: LeafRewrite<'_>,
    ) -> Result<TypeId, TypeStoreError> {
        let mut memo = BTreeMap::new();
        let mut pending = vec![RewriteStep::Visit(root)];

        while let Some(step) = pending.pop() {
            match step {
                RewriteStep::Visit(current) => {
                    if memo.contains_key(&current) {
                        continue;
                    }
                    #[cfg(test)]
                    if matches!(leaves, LeafRewrite::Generic(_)) {
                        tests::observe_substitution_read();
                    }

                    let kind = self.kind(current)?;
                    let resolved = match kind {
                        TypeKind::Never | TypeKind::Primitive(_) => Some(current),
                        TypeKind::GenericParameter(parameter) => Some(match leaves {
                            LeafRewrite::Generic(replacements) => {
                                replacements.get(parameter).copied().unwrap_or(current)
                            }
                            LeafRewrite::OwnedSelf { .. } => current,
                        }),
                        TypeKind::SelfType(found) => Some(match leaves {
                            LeafRewrite::OwnedSelf { owner, replacement } if found == owner => {
                                replacement
                            }
                            _ => current,
                        }),
                        _ => None,
                    };
                    if let Some(resolved) = resolved {
                        memo.insert(current, resolved);
                        continue;
                    }

                    if let TypeKind::AssociatedProjection(projection) = kind {
                        // Argument lookup must follow base completion, including on errors.
                        pending.push(RewriteStep::ProjectionArguments(current));
                        pending.push(RewriteStep::Visit(projection.base));
                        continue;
                    }

                    pending.push(RewriteStep::Rebuild(current));
                    match kind {
                        TypeKind::Nominal { substitution, .. } => pending.extend(
                            self.substitution(*substitution)?
                                .iter()
                                .rev()
                                .copied()
                                .map(RewriteStep::Visit),
                        ),
                        TypeKind::Reference { referent, .. }
                        | TypeKind::RawPointer { referent, .. } => {
                            pending.push(RewriteStep::Visit(*referent));
                        }
                        TypeKind::Tuple(elements) | TypeKind::ErrorUnion(elements) => {
                            pending.extend(elements.iter().rev().copied().map(RewriteStep::Visit));
                        }
                        TypeKind::Callable { parameters, result } => {
                            pending.push(RewriteStep::Visit(*result));
                            pending
                                .extend(parameters.iter().rev().copied().map(RewriteStep::Visit));
                        }
                        TypeKind::Never
                        | TypeKind::Primitive(_)
                        | TypeKind::GenericParameter(_)
                        | TypeKind::SelfType(_)
                        | TypeKind::AssociatedProjection(_) => {
                            unreachable!("leaves and projections have separate rewrite steps");
                        }
                    }
                }
                RewriteStep::ProjectionArguments(current) => {
                    let TypeKind::AssociatedProjection(projection) = self.kind(current)? else {
                        unreachable!("projection step is scheduled only for a projection");
                    };
                    pending.push(RewriteStep::Rebuild(current));
                    pending.extend(
                        self.substitution(projection.trait_ref.substitution)?
                            .iter()
                            .rev()
                            .copied()
                            .map(RewriteStep::Visit),
                    );
                }
                RewriteStep::Rebuild(current) => {
                    let child = |id| {
                        *memo
                            .get(&id)
                            .expect("rewrite children complete before their parents")
                    };
                    let resolved = match self.kind(current)?.clone() {
                        TypeKind::Nominal { head, substitution } => {
                            let arguments = self
                                .substitution(substitution)?
                                .iter()
                                .copied()
                                .map(child)
                                .collect::<Vec<_>>();
                            self.nominal(head, arguments)?
                        }
                        TypeKind::Reference {
                            mutability,
                            referent,
                        } => self.reference(mutability, child(referent))?,
                        TypeKind::RawPointer {
                            mutability,
                            referent,
                        } => self.raw_pointer(mutability, child(referent))?,
                        TypeKind::Tuple(elements) => {
                            self.tuple(elements.iter().copied().map(child))?
                        }
                        TypeKind::Callable { parameters, result } => {
                            self.callable(parameters.iter().copied().map(child), child(result))?
                        }
                        TypeKind::ErrorUnion(members) => {
                            self.error_union(members.iter().copied().map(child))?
                        }
                        TypeKind::AssociatedProjection(projection) => {
                            let arguments = self
                                .substitution(projection.trait_ref.substitution)?
                                .iter()
                                .copied()
                                .map(child)
                                .collect::<Vec<_>>();
                            let substitution = self.intern_substitution(arguments)?;
                            self.associated_projection(AssociatedProjection::new(
                                child(projection.base),
                                TraitRef::new(projection.trait_ref.declaration, substitution),
                                projection.associated,
                            ))?
                        }
                        TypeKind::Never
                        | TypeKind::Primitive(_)
                        | TypeKind::GenericParameter(_)
                        | TypeKind::SelfType(_) => {
                            unreachable!("leaves complete without a rebuild step");
                        }
                    };
                    memo.insert(current, resolved);
                }
            }
        }

        Ok(memo[&root])
    }
}
