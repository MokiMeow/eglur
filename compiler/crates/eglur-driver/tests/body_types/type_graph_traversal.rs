use super::{diagnostic_codes, session};
use eglur_diagnostics::DiagnosticDataValue;
use eglur_driver::{SupportProfile, TrustedCatalog};
use eglur_sema::{FactState, IntrinsicNominal, NominalHead, PrimitiveType, TypeKind};
use std::fmt::Write;

fn inferred_chain(depth: usize, option: bool, mismatch: bool) -> String {
    let result = if mismatch { "i64" } else { "Unit" };
    let mut source = format!("fn id[T](value: T) -> T = value\nfn f(x0: i64) -> {result} {{\n");
    for index in 1..=depth {
        let previous = index - 1;
        let value = if option {
            format!("Some(x{previous})")
        } else {
            format!("(x{previous},)")
        };
        writeln!(source, "let x{index} = {value}").unwrap();
    }
    if mismatch {
        writeln!(source, "return x{depth}\n}}").unwrap();
    } else {
        writeln!(source, "let last = id(x{depth})\nyield ()\n}}").unwrap();
    }
    source
}

fn check_inferred_chain(option: bool) {
    let depth = 512;
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut checked = session(&inferred_chain(depth, option, false));
    let analysis = checked
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert!(!analysis.trace().has_missing());
    assert!(analysis.fingerprints().is_some());

    let resolution = analysis.resolution().unwrap();
    let binding = resolution
        .names()
        .locals()
        .iter()
        .find(|binding| binding.name().spelling() == "last")
        .unwrap();
    let FactState::Known(fact) = resolution.body_types().local(binding.id()) else {
        panic!("inferred result did not retain its type");
    };
    let types = resolution.signatures().types();
    let mut ty = fact.ty();
    for _ in 0..depth {
        ty = match types.kind(ty).unwrap() {
            TypeKind::Tuple(elements) if !option => {
                assert_eq!(elements.len(), 1);
                elements[0]
            }
            TypeKind::Nominal {
                head: NominalHead::Intrinsic(IntrinsicNominal::Opt),
                substitution,
            } if option => {
                let arguments = types.substitution(*substitution).unwrap();
                assert_eq!(arguments.len(), 1);
                arguments[0]
            }
            kind => panic!("unexpected chain element: {kind:?}"),
        };
    }
    assert_eq!(
        types.kind(ty).unwrap(),
        &TypeKind::Primitive(PrimitiveType::I64)
    );
    assert_eq!(
        resolution.body_types().invocations().len(),
        if option { depth + 1 } else { 1 }
    );
    assert!(diagnostic_codes(&checked.into_diagnostics(None)).is_empty());
}

#[test]
fn shallow_bindings_can_infer_deep_tuple_types() {
    check_inferred_chain(false);
}

#[test]
fn shallow_bindings_can_infer_deep_option_types() {
    check_inferred_chain(true);
}

#[test]
fn inferred_type_mismatch_renders_the_complete_actual_type() {
    let depth = 512;
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut checked = session(&inferred_chain(depth, false, true));
    checked
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let snapshot = checked.into_diagnostics(None);
    assert_eq!(diagnostic_codes(&snapshot), ["RETURN_TYPE_MISMATCH"]);
    let data = snapshot.diagnostics()[0].data();
    assert_eq!(data["expected_type"], DiagnosticDataValue::from("i64"));
    let actual = format!("{}i64{}", "(".repeat(depth), ",)".repeat(depth));
    assert_eq!(data["actual_type"], DiagnosticDataValue::from(actual));
}

#[test]
fn nested_nominal_parameters_constrain_generic_arguments() {
    let depth = 254;
    let template = format!("{}T{}", "Opt[".repeat(depth), "]".repeat(depth));
    let actual = format!("{}i64{}", "Opt[".repeat(depth), "]".repeat(depth));
    let source = format!(
        "fn take[T](value: {template}) -> Unit = ()\nfn f(input: {actual}) -> Unit = take(input)\n"
    );
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut checked = session(&source);
    let analysis = checked
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert!(!analysis.trace().has_missing());
    assert_eq!(
        analysis
            .resolution()
            .unwrap()
            .body_types()
            .invocations()
            .len(),
        1
    );
    assert!(diagnostic_codes(&checked.into_diagnostics(None)).is_empty());
}

#[test]
fn diagnostic_type_children_keep_their_syntax_and_source_names() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (actual, expected) in [
        ("(i64, (Bool, Str))", "(i64, (Bool, Str))"),
        ("(i64,)", "(i64,)"),
        ("Fn(i64, Bool) -> Str", "Fn(i64, Bool) -> Str"),
        ("&mut i64", "&mut i64"),
        ("&i64", "&i64"),
        ("Opt[T]", "Opt[T]"),
        ("T::Item", "T::Item"),
        (
            "Res[i64, Union[Second, First]]",
            "Res[i64, Union[First, Second]]",
        ),
    ] {
        let source = format!(
            "trait Has {{ type Item }}\nenum First {{ A }}\nenum Second {{ B }}\nfn f![T: Has](input: {actual}) -> Bool = input\n"
        );
        let mut checked = session(&source);
        checked
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        let snapshot = checked.into_diagnostics(None);
        assert_eq!(
            diagnostic_codes(&snapshot),
            ["RETURN_TYPE_MISMATCH"],
            "{actual}"
        );
        let data = snapshot.diagnostics()[0].data();
        assert_eq!(
            data["actual_type"],
            DiagnosticDataValue::from(expected),
            "{actual}"
        );
        assert_eq!(
            data["expected_type"],
            DiagnosticDataValue::from("Bool"),
            "{actual}"
        );
    }
}
