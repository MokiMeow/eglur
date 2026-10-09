use std::collections::{BTreeMap, BTreeSet};

use eglur_syntax::is_identifier_spelling;

use super::{
    InherentMemberForm, TrustedDeclarationSignature, TrustedInherentMember, TrustedInherentOwner,
    TrustedInherentOwnerHead, TrustedSignatureError, TrustedSignatureKind, TrustedTypeExpr,
};
use crate::ExternalDefId;

pub(super) fn admit_inherent_owners(
    owners: impl IntoIterator<Item = TrustedInherentOwner>,
    declarations: &[TrustedDeclarationSignature],
    by_id: &BTreeMap<ExternalDefId, usize>,
    hidden_callables: &mut BTreeSet<ExternalDefId>,
) -> Result<BTreeMap<TrustedInherentOwnerHead, TrustedInherentOwner>, TrustedSignatureError> {
    let mut admitted = BTreeMap::new();
    for mut owner in owners {
        if admitted.contains_key(&owner.head) {
            return Err(TrustedSignatureError::DuplicateInherentOwner(owner.head));
        }
        if let TrustedInherentOwnerHead::Nominal(id) = &owner.head
            && !by_id.get(id).is_some_and(|&index| {
                matches!(
                    declarations[index].kind(),
                    TrustedSignatureKind::Nominal { .. } | TrustedSignatureKind::Enum { .. }
                )
            })
        {
            return Err(TrustedSignatureError::InvalidInherentOwner {
                head: owner.head,
                reason: "owner is not a declared nominal value type",
            });
        }
        owner.members.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.callable.cmp(&right.callable))
        });

        for member in &owner.members {
            if !hidden_callables.insert(member.callable.clone()) {
                return Err(invalid_member(
                    member,
                    "callable already has a method owner",
                ));
            }
            let Some(&index) = by_id.get(&member.callable) else {
                return Err(invalid_member(member, "unknown callable"));
            };
            validate_member(member, &owner.head, &declarations[index])?;
        }

        let mut start = 0;
        while start < owner.members.len() {
            let mut end = start + 1;
            while end < owner.members.len() && owner.members[end].name == owner.members[start].name
            {
                end += 1;
            }
            if end - start > 1 {
                for member in &owner.members[start..end] {
                    if !member.overload {
                        return Err(invalid_member(
                            member,
                            "overload family member lacks marker",
                        ));
                    }
                }
            }
            start = end;
        }
        admitted.insert(owner.head.clone(), owner);
    }
    Ok(admitted)
}

fn validate_member(
    member: &TrustedInherentMember,
    head: &TrustedInherentOwnerHead,
    declaration: &TrustedDeclarationSignature,
) -> Result<(), TrustedSignatureError> {
    let TrustedSignatureKind::Callable {
        bang,
        generic_parameters,
        parameters,
        result,
        requirements,
    } = declaration.kind()
    else {
        return Err(invalid_member(member, "member target is not callable"));
    };
    if !is_identifier_spelling(member.name.text()) || *bang != member.name.is_bang() {
        return Err(invalid_member(member, "invalid name or bang mismatch"));
    }
    let matches_head = match (&member.owner_pattern, head) {
        (TrustedTypeExpr::Primitive(left), TrustedInherentOwnerHead::Primitive(right)) => {
            left == right
        }
        (
            TrustedTypeExpr::Intrinsic { head: left, .. },
            TrustedInherentOwnerHead::Intrinsic(right),
        ) => left == right,
        (TrustedTypeExpr::Nominal { head: left, .. }, TrustedInherentOwnerHead::Nominal(right)) => {
            left == right
        }
        _ => false,
    };
    if !matches_head {
        return Err(invalid_member(
            member,
            "owner pattern does not have the inventory head",
        ));
    }
    if member.owner_parameter_count > generic_parameters.len() {
        return Err(invalid_member(
            member,
            "owner binder prefix exceeds callable generics",
        ));
    }
    let mut used = vec![false; member.owner_parameter_count];
    visit_types([&member.owner_pattern], |ty| match ty {
        TrustedTypeExpr::GenericParameter(index) => {
            let Some(slot) = usize::try_from(*index)
                .ok()
                .and_then(|index| used.get_mut(index))
            else {
                return Err(invalid_member(
                    member,
                    "owner pattern uses a method-local or unknown binder",
                ));
            };
            *slot = true;
            Ok(())
        }
        TrustedTypeExpr::SelfType(_) | TrustedTypeExpr::AssociatedProjection { .. } => Err(
            invalid_member(member, "owner pattern must be structural and explicit"),
        ),
        _ => Ok(()),
    })?;
    if used.contains(&false) {
        return Err(invalid_member(
            member,
            "owner binder is absent from the owner pattern",
        ));
    }

    for generic in &generic_parameters[..member.owner_parameter_count] {
        for bound in &generic.bounds {
            visit_types(bound.arguments.iter(), |ty| {
                if let TrustedTypeExpr::GenericParameter(index) = ty
                    && usize::try_from(*index)
                        .map_or(true, |index| index >= member.owner_parameter_count)
                {
                    return Err(invalid_member(
                        member,
                        "owner bound uses a method-local binder",
                    ));
                }
                Ok(())
            })?;
        }
    }
    let mut roots = parameters
        .iter()
        .map(|parameter| &parameter.ty)
        .collect::<Vec<_>>();
    roots.push(result);
    for requirement in requirements {
        let super::TrustedCallableRequirement::FixedWidthInteger(ty) = requirement;
        roots.push(ty);
    }
    for generic in generic_parameters {
        for bound in &generic.bounds {
            roots.extend(bound.arguments.iter());
        }
    }
    visit_types(roots, |ty| {
        if matches!(ty, TrustedTypeExpr::SelfType(_)) {
            Err(invalid_member(
                member,
                "ordinary members require an explicit complete Self equivalent",
            ))
        } else {
            Ok(())
        }
    })?;

    let receiver = parameters
        .first()
        .is_some_and(|parameter| parameter.name.as_deref() == Some("self"));
    if parameters
        .iter()
        .skip(1)
        .any(|parameter| parameter.name.as_deref() == Some("self"))
    {
        return Err(invalid_member(member, "self parameter must be first"));
    }
    if (member.form != InherentMemberForm::Associated) != receiver {
        return Err(invalid_member(
            member,
            "receiver form disagrees with first self parameter",
        ));
    }
    if member.form == InherentMemberForm::MutableReceiver && !bang {
        return Err(invalid_member(
            member,
            "mutable receiver requires a bang callable",
        ));
    }
    Ok(())
}

fn invalid_member(member: &TrustedInherentMember, reason: &'static str) -> TrustedSignatureError {
    TrustedSignatureError::InvalidInherentMember {
        callable: member.callable.clone(),
        reason,
    }
}

fn visit_types<'a>(
    roots: impl IntoIterator<Item = &'a TrustedTypeExpr>,
    mut visit: impl FnMut(&TrustedTypeExpr) -> Result<(), TrustedSignatureError>,
) -> Result<(), TrustedSignatureError> {
    let mut pending = roots.into_iter().collect::<Vec<_>>();
    while let Some(ty) = pending.pop() {
        visit(ty)?;
        match ty {
            TrustedTypeExpr::Intrinsic { arguments, .. }
            | TrustedTypeExpr::Nominal { arguments, .. } => pending.extend(arguments.iter()),
            TrustedTypeExpr::Reference { referent, .. }
            | TrustedTypeExpr::RawPointer { referent, .. } => pending.push(referent),
            TrustedTypeExpr::Tuple(elements) | TrustedTypeExpr::ErrorUnion(elements) => {
                pending.extend(elements.iter())
            }
            TrustedTypeExpr::Callable { parameters, result } => {
                pending.extend(parameters.iter());
                pending.push(result);
            }
            TrustedTypeExpr::AssociatedProjection {
                base,
                trait_arguments,
                ..
            } => {
                pending.push(base);
                pending.extend(trait_arguments.iter());
            }
            TrustedTypeExpr::Never
            | TrustedTypeExpr::Primitive(_)
            | TrustedTypeExpr::GenericParameter(_)
            | TrustedTypeExpr::SelfType(_) => {}
        }
    }
    Ok(())
}
