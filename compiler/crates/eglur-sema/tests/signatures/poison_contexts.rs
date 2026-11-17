use super::*;

#[test]
fn wide_poisoned_generic_contexts_preserve_each_root_and_support_boundary() {
    for count in [8, 32, 128] {
        let parameters = vec!["Box"; count].join(",");
        for text in [
            format!("fn many[{parameters}]()->Unit {{}}\n"),
            format!("struct Host[{parameters}] {{ fn method[A](self:&Self)->Unit {{}} }}\n"),
            format!("trait Host[{parameters}] {{ fn method[A](self:&Self)->Unit }}\n"),
        ] {
            for support in [
                CapabilitySet::signatures(),
                CapabilitySet::without_signatures(),
            ] {
                let analysis = analyze(&text, &support, Some(&empty_external()), None);
                let Some(signatures) = &analysis.signatures else {
                    assert!(!support.supports(CompileCapability::Typecheck(
                        TypecheckCapability::ResolveDeclarationSignatures
                    )));
                    continue;
                };
                let module = analysis.declarations.modules().next().unwrap();
                let mut expected: Option<eglur_diagnostics::Poison> = None;
                let mut roots = 0;
                for declaration in module.declarations() {
                    if declaration.kind() != DeclarationKind::GenericParameter {
                        continue;
                    }
                    if let FactState::Poisoned(root) =
                        signatures.generic_parameter(&GenericParameterId::Source(declaration.id()))
                    {
                        roots += 1;
                        expected = Some(match expected {
                            Some(prior) => prior.merge(root).unwrap(),
                            None => root.clone(),
                        });
                    }
                }
                assert_eq!(roots, count);
                let expected = expected.unwrap();
                let image = format!("{expected:?}");
                for declaration in module.declarations().filter(|declaration| {
                    matches!(
                        declaration.kind(),
                        DeclarationKind::Function
                            | DeclarationKind::Struct
                            | DeclarationKind::Trait
                    )
                }) {
                    let FactState::Poisoned(actual) =
                        signatures.source_declaration(declaration.id())
                    else {
                        panic!("reserved generics must poison their containing signatures");
                    };
                    assert_eq!(format!("{actual:?}"), image);
                    assert_eq!(format!("{:?}", actual.merge(&expected).unwrap()), image);
                }
                assert_eq!(analysis.diagnostics.diagnostics().len(), count);
            }
        }
    }
}
