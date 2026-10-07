use super::{diagnostic_codes, session};
use eglur_driver::{SupportProfile, TrustedCatalog};
use eglur_sema::{CompileCapability, FactState, TypecheckCapability};

fn check_values(source: &str) {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(source);
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let missing = analysis.trace().missing().collect::<Vec<_>>();
    assert!(missing.is_empty(), "{missing:?}");
    assert!(analysis.fingerprints().is_some());

    let diagnostics = session.into_diagnostics(None);
    assert!(
        diagnostic_codes(&diagnostics).is_empty(),
        "{:?}",
        diagnostics.diagnostics()
    );
}

#[test]
fn nested_tuple_values_typecheck_on_the_default_stack() {
    let depth = 240;
    let source = format!(
        "fn f(x: i64) -> Unit {{ let value = {}x{}\nyield () }}\n",
        "(x, ".repeat(depth),
        ")".repeat(depth),
    );
    check_values(&source);
}

#[test]
fn nested_references_and_dereferences_preserve_places_on_the_default_stack() {
    let depth = 120;
    let source = format!("fn f(x: i64) -> i64 = {}x\n", "* & ".repeat(depth),);
    check_values(&source);
}

#[test]
fn chained_tuple_projections_typecheck_on_the_default_stack() {
    let depth = 240;
    let source = format!(
        "fn f(value: {}i64{}) -> i64 = value{}\n",
        "(i64, ".repeat(depth),
        ")".repeat(depth),
        ".1".repeat(depth),
    );
    check_values(&source);
}

#[test]
fn chained_members_through_references_typecheck_on_the_default_stack() {
    let depth = 255;
    let source = format!(
        "struct Cell {{ field next: &Cell }}\nfn f(cell: &Cell) -> &Cell = cell{}\n",
        ".next".repeat(depth),
    );
    check_values(&source);
}

#[test]
fn mixed_tuple_call_and_projection_frames_share_the_work_stack() {
    let depth = 100;
    let source = format!(
        "fn id[T](value: T) -> T = value\nfn f(x: i64) -> i64 = {}x{}\n",
        "id((".repeat(depth),
        ", x)).0".repeat(depth),
    );
    check_values(&source);
}

#[test]
fn deep_projection_failure_preserves_one_diagnostic() {
    let source = format!("fn f(x: i64) -> i64 = x{}\n", ".0".repeat(255),);
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(&source);
    session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();

    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["TUPLE_PROJECTION_NON_TUPLE"]
    );
}

#[test]
fn deep_unsupported_reference_operand_stays_unperformed() {
    let source = format!("fn f() -> i64 = {}[1]\n", "* & ".repeat(120),);
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(&source);
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckContainers)
    }));
    let resolution = analysis.resolution().unwrap();
    let function = resolution
        .declarations()
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| {
            declaration
                .name()
                .is_some_and(|name| name.spelling() == "f")
        })
        .unwrap();

    assert!(matches!(
        resolution.body_types().body(function.id()),
        FactState::Unperformed
    ));
    assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
}

#[test]
fn tuple_typing_visits_later_elements_after_poison_and_missing_support() {
    let source = "fn f(x: i64) -> Unit { let value = (x.0, [1], x.1)\nyield () }\n";
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(source);
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckContainers)
    }));

    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["TUPLE_PROJECTION_NON_TUPLE", "TUPLE_PROJECTION_NON_TUPLE"]
    );
}

#[test]
fn wide_tuple_failures_preserve_later_bodies_and_partial_diagnostics() {
    for count in [8_usize, 32, 128] {
        for mode in ["names", "projection", "mixed-partial", "normal-absence"] {
            let values = (0..count)
                .map(|index| match mode {
                    "projection" => format!("x.{index}"),
                    "mixed-partial" if index % 2 == 0 => "[1]".into(),
                    "normal-absence" if index == 0 => "{ return }".into(),
                    _ => format!("missing{index}"),
                })
                .collect::<Vec<_>>()
                .join(", ");
            let source = format!(
                "fn f(x: i64) -> Unit {{ let value = ({values})\nyield () }}\nfn after() -> Unit = ()\n"
            );
            let mut session = session(&source);
            let catalog = TrustedCatalog::first_slice().unwrap();
            let analysis = session
                .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
                .unwrap();
            assert_eq!(analysis.trace().has_missing(), mode == "mixed-partial");
            let resolution = analysis.resolution().unwrap();
            let declaration = |name: &str| {
                resolution
                    .declarations()
                    .modules()
                    .flat_map(|module| module.declarations())
                    .find(|declaration| declaration.name().is_some_and(|key| key.text() == name))
                    .unwrap()
                    .id()
            };
            assert!(matches!(
                resolution.body_types().body(declaration("f")),
                FactState::Poisoned(_)
            ));
            assert!(matches!(
                resolution.body_types().body(declaration("after")),
                FactState::Known(_)
            ));

            let expected_code = if mode == "projection" {
                "TUPLE_PROJECTION_NON_TUPLE"
            } else {
                "NAME_NOT_FOUND"
            };
            let expected_count = match mode {
                "mixed-partial" => count / 2,
                "normal-absence" => count - 1,
                _ => count,
            };
            assert_eq!(
                diagnostic_codes(&session.into_diagnostics(None)),
                vec![expected_code; expected_count]
            );
        }
    }
}
