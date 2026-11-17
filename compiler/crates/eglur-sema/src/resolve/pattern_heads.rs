use super::{
    LookupPurpose, NameKey, NameResolutionError, Namespace, PathLookup, ResolutionKey, Resolver,
    UnqualifiedLookup, VisitFold, VisitOutcome,
};
use crate::{ModuleId, SemanticSource};
use eglur_hir::{Body, PatternElement, PatternId, PatternKind};

impl Resolver<'_, '_> {
    pub(super) fn visit_pattern_heads(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        body: &Body,
        pattern_id: PatternId,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let pattern = body
            .pattern(pattern_id)
            .ok_or(NameResolutionError::WrongBodyPattern(pattern_id))?;
        let seed = match pattern.kind() {
            PatternKind::Variant { path, .. } if path.is_unqualified_residual_constructor() => {
                let name = &path.segments()[0];
                let name_key =
                    NameKey::from_hir(name).expect("residual constructor has a valid name");
                let plan = self.plan_unqualified(
                    module,
                    &name_key,
                    UnqualifiedLookup::ResidualConstructor,
                )?;
                let outcome = self.materialize_lookup_plan(
                    source,
                    name.origin(),
                    LookupPurpose::Ordinary,
                    &plan,
                )?;
                self.record_fact(
                    ResolutionKey::Pattern {
                        module,
                        pattern: pattern_id,
                    },
                    outcome.clone(),
                )?;
                outcome
            }
            PatternKind::Variant {
                path,
                payload: eglur_hir::PatternPayload::Named(_),
            }
            | PatternKind::Struct {
                path: Some(path), ..
            } => self.resolve_path(
                module,
                source,
                path,
                PathLookup::BracedConstructor,
                ResolutionKey::Pattern {
                    module,
                    pattern: pattern_id,
                },
                LookupPurpose::Ordinary,
            )?,
            PatternKind::Variant { path, .. } => self.resolve_path(
                module,
                source,
                path,
                PathLookup::Namespace(Namespace::Value),
                ResolutionKey::Pattern {
                    module,
                    pattern: pattern_id,
                },
                LookupPurpose::Ordinary,
            )?,
            PatternKind::Invalid => self.invalid_outcome(source, pattern.origin())?,
            _ => VisitOutcome::Known,
        };
        let mut outcome = VisitFold::Empty.include(seed)?;
        match pattern.kind() {
            PatternKind::Or(patterns) => {
                for pattern in patterns {
                    outcome = outcome
                        .include(self.visit_pattern_heads(module, source, body, *pattern)?)?;
                }
            }
            PatternKind::Tuple(elements) => {
                for element in elements {
                    let PatternElement::Pattern(pattern) = element else {
                        continue;
                    };
                    outcome = outcome
                        .include(self.visit_pattern_heads(module, source, body, *pattern)?)?;
                }
            }
            PatternKind::Alias { pattern, .. } | PatternKind::Group(pattern) => {
                outcome =
                    outcome.include(self.visit_pattern_heads(module, source, body, *pattern)?)?;
            }
            PatternKind::Variant { payload, .. } => match payload {
                eglur_hir::PatternPayload::Positional(elements) => {
                    for element in elements {
                        if let PatternElement::Pattern(pattern) = element {
                            outcome = outcome.include(
                                self.visit_pattern_heads(module, source, body, *pattern)?,
                            )?;
                        }
                    }
                }
                eglur_hir::PatternPayload::Named(fields) => {
                    for field in fields {
                        if let eglur_hir::FieldPattern::Field {
                            pattern: Some(pattern),
                            ..
                        } = field
                        {
                            outcome = outcome.include(
                                self.visit_pattern_heads(module, source, body, *pattern)?,
                            )?;
                        }
                    }
                }
                eglur_hir::PatternPayload::Absent => {}
            },
            PatternKind::Struct { fields, .. } => {
                for field in fields {
                    if let eglur_hir::FieldPattern::Field {
                        pattern: Some(pattern),
                        ..
                    } = field
                    {
                        outcome = outcome
                            .include(self.visit_pattern_heads(module, source, body, *pattern)?)?;
                    }
                }
            }
            PatternKind::Binding(_)
            | PatternKind::Wildcard
            | PatternKind::Literal(_)
            | PatternKind::Unit
            | PatternKind::Invalid => {}
        }
        Ok(outcome.finish())
    }
}
