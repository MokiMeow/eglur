use std::collections::BTreeMap;

use crate::{
    CanonicalFingerprintBuilder, CanonicalTarget, DeclarationCollection, DeclarationKind,
    DeclarationSignatures, EffectCategory, EffectFacts, EffectFingerprint, EffectSummary,
    FactState, FingerprintDomain, KnownEffectSummary, ModuleAddress, ResolvedImports,
    StableDeclarationKeys, StableTargetKey, StateScope,
};

use super::SemanticFingerprintError;
use super::interface::{child_is_externally_reachable, interface_reachable, target_error};

pub(super) fn compute(
    declarations: &DeclarationCollection,
    imports: &ResolvedImports,
    signatures: &DeclarationSignatures,
    effects: &EffectFacts,
    keys: &StableDeclarationKeys,
) -> Result<BTreeMap<ModuleAddress, EffectFingerprint>, SemanticFingerprintError> {
    let mut reachable = BTreeMap::new();
    declarations
        .modules()
        .map(|module| {
            let mut summaries = Vec::new();
            for declaration in module.declarations() {
                if !matches!(
                    declaration.kind(),
                    DeclarationKind::Function | DeclarationKind::ExternFunction
                ) || !interface_reachable(
                    declaration.id(),
                    declarations,
                    signatures,
                    &mut reachable,
                )? {
                    continue;
                }
                push_summary(
                    &mut summaries,
                    &CanonicalTarget::Source(declaration.id()),
                    effects,
                    keys,
                )?;
            }

            for import in module.imports() {
                let FactState::Known(bindings) = imports.fact(import.id()) else {
                    return Err(SemanticFingerprintError::IncompleteImport(import.id()));
                };
                for target in bindings
                    .iter()
                    .filter(|binding| binding.is_public())
                    .flat_map(|binding| binding.targets())
                {
                    let CanonicalTarget::Source(declaration) = target else {
                        continue;
                    };
                    push_reexported_summaries(
                        &mut summaries,
                        *declaration,
                        declarations,
                        effects,
                        keys,
                    )?;
                }
            }

            summaries.sort_by(|left, right| left.0.cmp(&right.0));
            summaries.dedup_by(|left, right| left.0 == right.0);
            let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Effect);
            builder.write_str(module.address().as_str());
            builder.write_len(summaries.len());
            for (target, summary) in summaries {
                target.write_to(&mut builder);
                write_effect_summary(&mut builder, &summary);
            }
            Ok((
                module.address().clone(),
                EffectFingerprint::new(builder.finish()),
            ))
        })
        .collect()
}

fn push_reexported_summaries(
    summaries: &mut Vec<(StableTargetKey, EffectSummary)>,
    declaration: crate::DefId,
    declarations: &DeclarationCollection,
    effects: &EffectFacts,
    keys: &StableDeclarationKeys,
) -> Result<(), SemanticFingerprintError> {
    let declaration_fact = declarations
        .declaration(declaration)
        .ok_or(SemanticFingerprintError::UnknownDeclaration(declaration))?;
    if matches!(
        declaration_fact.kind(),
        DeclarationKind::Function | DeclarationKind::ExternFunction
    ) {
        push_summary(
            summaries,
            &CanonicalTarget::Source(declaration),
            effects,
            keys,
        )?;
    }
    let module = declarations
        .module(declaration.module())
        .ok_or(SemanticFingerprintError::UnknownDeclaration(declaration))?;
    for child in module.children(declaration) {
        if child_is_externally_reachable(child, declaration, declarations)? {
            push_reexported_summaries(summaries, child.id(), declarations, effects, keys)?;
        }
    }
    Ok(())
}

fn push_summary(
    summaries: &mut Vec<(StableTargetKey, EffectSummary)>,
    target: &CanonicalTarget,
    effects: &EffectFacts,
    keys: &StableDeclarationKeys,
) -> Result<(), SemanticFingerprintError> {
    let stable = keys.target(target).ok_or_else(|| target_error(target))?;
    let FactState::Known(summary) = effects.summary(target) else {
        return Err(SemanticFingerprintError::IncompleteEffect(stable));
    };
    summaries.push((stable, summary.clone()));
    Ok(())
}

fn write_effect_summary(builder: &mut CanonicalFingerprintBuilder, summary: &EffectSummary) {
    match summary {
        EffectSummary::Unknown => builder.write_u8(0),
        EffectSummary::Known(summary) => {
            builder.write_u8(1);
            write_known_effect_summary(builder, summary);
        }
    }
}

fn write_known_effect_summary(
    builder: &mut CanonicalFingerprintBuilder,
    summary: &KnownEffectSummary,
) {
    builder.write_len(summary.categories().len());
    for category in summary.categories() {
        builder.write_u8(match category {
            EffectCategory::Io => 0,
            EffectCategory::State => 1,
            EffectCategory::Nondet => 2,
        });
    }
    builder.write_u8(match summary.state_scope() {
        StateScope::None => 0,
        StateScope::Localizable => 1,
        StateScope::Nonlocal => 2,
    });
}
