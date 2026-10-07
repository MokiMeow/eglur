use super::*;

#[test]
fn wide_direct_generic_owners_keep_known_signatures_and_parameter_order() {
    for count in [8, 32, 128] {
        let parameters = (0..count)
            .map(|index| format!("T{index:03}"))
            .collect::<Vec<_>>()
            .join(", ");
        let methods = (0..count)
            .map(|index| format!("M{index:03}"))
            .collect::<Vec<_>>()
            .join(", ");
        let last = format!("T{:03}", count - 1);
        for visibility in ["", "pub "] {
            let text = format!(
                "{visibility}fn top[{parameters}](value: {last}) -> {last} = value\n\
                 {visibility}type Alias[{parameters}] = {last}\n\
                 {visibility}trait Marker[{parameters}] {{ type Item\n fn method[{methods}](self: &Self, value: M000) -> M000 = value }}\n\
                 {visibility}struct Host[{parameters}] {{ field value: T000\n fn method[{methods}](self: &Self, value: M000) -> M000 = value }}\n\
                 {visibility}enum Choice[{parameters}] {{ Empty\n Pair {{ first: T000, last: {last} }} }}\n\
                 impl[{parameters}] Marker[{parameters}] for Host[{parameters}] {{ type Item = {last} }}\n"
            );
            let analysis = analyze(
                &text,
                &CapabilitySet::signatures(),
                Some(&empty_external()),
                None,
            );
            assert!(
                analysis.diagnostics.diagnostics().is_empty(),
                "{:?}",
                diagnostic_codes(&analysis.diagnostics)
            );
            let signatures = analysis.signatures.unwrap();
            let mut seen_generics = 0;
            for module in analysis.declarations.modules() {
                for declaration in module.declarations() {
                    assert!(matches!(
                        signatures.source_declaration(declaration.id()),
                        FactState::Known(_)
                    ));
                    if declaration.kind() != DeclarationKind::GenericParameter {
                        continue;
                    }
                    seen_generics += 1;
                    let FactState::Known(parameter) =
                        signatures.generic_parameter(&GenericParameterId::Source(declaration.id()))
                    else {
                        panic!("generic fact not resolved")
                    };
                    assert_eq!(parameter.name(), declaration.name().unwrap().text());
                }
            }
            assert_eq!(seen_generics, 8 * count);
        }
    }
}

#[test]
fn poisoned_generic_and_impl_headers_leave_valid_later_siblings_known() {
    let analysis = analyze(
        "fn duplicate[A, A, B](value: B) -> B = value\n\
         fn valid[C, D](value: D) -> D = value\n\
         trait Link { type Item }\n struct Host {}\n\
         impl[A] Link for Host { type Item = A }\n",
        &CapabilitySet::signatures(),
        Some(&empty_external()),
        None,
    );
    assert_eq!(
        diagnostic_codes(&analysis.diagnostics),
        [
            "GENERIC_PARAMETER_REDECLARATION",
            "IMPL_UNCONSTRAINED_TYPE_PARAM"
        ]
    );
    let signatures = analysis.signatures.unwrap();
    let valid = source_def(&analysis.declarations, "valid", DeclarationKind::Function);
    assert!(matches!(
        signatures.source_declaration(valid),
        FactState::Known(_)
    ));
    for name in ["C", "D"] {
        let id = analysis.declarations.child_candidates(
            valid,
            &NameKey::new(name, false).unwrap(),
            DeclarationKind::GenericParameter,
        )[0];
        assert!(matches!(
            signatures.generic_parameter(&GenericParameterId::Source(id)),
            FactState::Known(_)
        ));
    }
    let binding = source_def(
        &analysis.declarations,
        "Item",
        DeclarationKind::AssociatedTypeBinding,
    );
    assert!(matches!(
        signatures.source_declaration(binding),
        FactState::Poisoned(_)
    ));
}
