use super::session;
use eglur_diagnostics::DiagnosticSnapshot;
use eglur_driver::{SupportProfile, TrustedCatalog};
use eglur_sema::{FactState, PrimitiveType, TypeKind};

#[test]
fn overloaded_callable_values_keep_wide_contextual_parameter_types() {
    for width in [8, 32, 128] {
        let elements = vec!["i64"; width].join(", ");
        let mut source = format!("type Wide = ({elements})\n");
        for index in 0..width {
            source += &format!(
                "struct S{index} {{}}\noverload fn choose[T](x: S{index}, seed: T, tag: Bool) -> T = seed\n"
            );
        }
        source += "overload fn choose[T, U](x: T, seed: U, tag: i64) -> U = seed\nfn f() -> Unit {\nlet value: Fn(Wide, i64, i64) -> i64 = choose\nyield ()\n}";
        let catalog = TrustedCatalog::first_slice().unwrap();
        let mut checked = session(&source);
        let analysis = checked
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        assert!(!analysis.trace().has_missing());
        let resolution = analysis.resolution().unwrap();
        let binding = resolution
            .names()
            .locals()
            .iter()
            .find(|binding| binding.name().spelling() == "value")
            .unwrap();
        let FactState::Known(fact) = resolution.body_types().local(binding.id()) else {
            panic!("callable value lost its type")
        };
        let types = resolution.signatures().types();
        let TypeKind::Callable { parameters, result } = types.kind(fact.ty()).unwrap() else {
            panic!("callable type expected")
        };
        assert_eq!(parameters.len(), 3);
        let TypeKind::Tuple(elements) = types.kind(parameters[0]).unwrap() else {
            panic!("wide contextual tuple expected")
        };
        assert_eq!(elements.len(), width);
        for &ty in elements
            .iter()
            .chain(parameters[1..].iter())
            .chain(std::iter::once(result))
        {
            assert_eq!(
                types.kind(ty).unwrap(),
                &TypeKind::Primitive(PrimitiveType::I64)
            );
        }
        assert!(matches!(
            checked.into_diagnostics(None),
            DiagnosticSnapshot::Accepted(_)
        ));
    }
}
