use super::{body_profile, diagnostic_codes, session};
use eglur_driver::{SupportProfile, TrustedCatalog};
use eglur_sema::{
    BodyTypes, CompileCapability, ControlTransferKind, FactState, TypecheckCapability,
};

fn accepted(source: &str, inspect: impl FnOnce(&BodyTypes)) {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(source);
    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    let declaration = resolution
        .declarations()
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| declaration.name().is_some_and(|name| name.text() == "f"))
        .unwrap();
    assert!(matches!(
        resolution.body_types().body(declaration.id()),
        FactState::Known(_)
    ));
    inspect(resolution.body_types());
    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        Vec::<&str>::new()
    );
}

fn mixed_family_source(leaf: &str, depth: usize) -> String {
    let mut expression = leaf.to_owned();
    for _ in 0..depth {
        expression = format!(
            "id((if flag {{ yield match x {{ value => Wrap {{ value: {expression} }}.value }} }} else {{ yield x }},).0)"
        );
    }
    format!(
        "struct Wrap[T] {{ field value: T }}\nfn id[T](value: T) -> T = value\nfn f(flag: Bool, x: i64) -> Res[i64, NumericError] = result {{ yield Ok(checked {{ yield ({expression}) + 0 }}?) }}\n"
    )
}

#[test]
fn checked_question_as_struct_field_is_supported() {
    let source = "struct Wrap { field value: i64 }\nfn f(x: i64) -> Res[i64, NumericError] = result { yield Ok((Wrap { value: (checked { yield x + 0 }?) }).value) }\n";
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut accepted = session(source);
    accepted
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let diagnostics = accepted.into_diagnostics(None);
    assert!(
        diagnostic_codes(&diagnostics).is_empty(),
        "{:?}",
        diagnostics.diagnostics()
    );
}

#[test]
fn wildcard_match_arm_accepts_struct_literal_result() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for pattern in ["_", "value"] {
        let source = format!(
            "struct Wrap {{ field value: i64 }}\nfn f(x: i64) -> i64 = match x {{ {pattern} => Wrap {{ value: x }}.value }}\n"
        );
        let mut accepted = session(&source);
        accepted
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        let diagnostics = accepted.into_diagnostics(None);
        assert!(
            diagnostic_codes(&diagnostics).is_empty(),
            "{pattern}: {:?}",
            diagnostics.diagnostics()
        );
    }

    let depth = 2;
    let source = format!(
        "struct Wrap[T] {{ field value: T }}\nfn id[T](value: T) -> T = value\nfn f(flag: Bool, x: i64) -> i64 = {}x{}\n",
        "id((if flag { yield match x { _ => Wrap { value: ".repeat(depth),
        " }.value } } else { yield x },).0)".repeat(depth),
    );
    let mut accepted = session(&source);
    accepted
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let diagnostics = accepted.into_diagnostics(None);
    assert!(
        diagnostic_codes(&diagnostics).is_empty(),
        "nested wildcard: {:?}",
        diagnostics.diagnostics()
    );
}

#[test]
fn wildcard_match_inside_expression_block_retains_its_result() {
    let source = "fn f(x: i64) -> i64 = { yield match x { _ => x } }\n";
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut accepted = session(source);
    accepted
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let diagnostics = accepted.into_diagnostics(None);
    assert!(
        diagnostic_codes(&diagnostics).is_empty(),
        "{:?}",
        diagnostics.diagnostics()
    );
}

#[test]
fn mixed_family_nesting_keeps_results_and_invocations_on_the_default_stack() {
    let depth = 30;
    let source = mixed_family_source("x", depth);
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut accepted = session(&source);
    let analysis = accepted
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert!(analysis.trace().missing().next().is_none());
    let invocations = analysis
        .resolution()
        .unwrap()
        .body_types()
        .invocations()
        .len();
    let diagnostics = accepted.into_diagnostics(None);
    assert!(
        diagnostic_codes(&diagnostics).is_empty(),
        "source length {}: {:?}",
        source.len(),
        diagnostics.diagnostics()
    );
    assert_eq!(invocations, 2 * depth + 1);
}

#[test]
fn mixed_family_nesting_preserves_poison_and_unsupported_results() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (leaf, expected) in [("x.0", Some("TUPLE_PROJECTION_NON_TUPLE")), ("[1]", None)] {
        let source = mixed_family_source(leaf, 30);
        let mut rejected = session(&source);
        let analysis = rejected
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        let facts = analysis.resolution().unwrap().body_types();
        assert_eq!(facts.invocations().len(), 0, "{leaf}");
        if expected.is_none() {
            assert!(analysis.trace().missing().any(|entry| {
                entry.capability()
                    == CompileCapability::Typecheck(TypecheckCapability::CheckContainers)
            }));
        }
        let diagnostics = rejected.into_diagnostics(None);
        assert_eq!(
            diagnostic_codes(&diagnostics),
            expected.into_iter().collect::<Vec<_>>(),
            "{leaf}, source length {}: {:?}",
            source.len(),
            diagnostics.diagnostics()
        );
    }
}

#[test]
fn nested_yield_blocks_typecheck_on_the_default_stack() {
    let depth = 255;
    let source = format!(
        "fn f(x: i64) -> i64 = {}x{}\n",
        "{ yield ".repeat(depth),
        " }".repeat(depth),
    );
    accepted(&source, |facts| {
        assert_eq!(facts.regions().len(), depth);
        assert_eq!(
            facts
                .transfers()
                .iter()
                .filter(|transfer| transfer.kind() == ControlTransferKind::Yield)
                .count(),
            depth
        );
    });
}

#[test]
fn nested_let_and_assignment_blocks_typecheck_on_the_default_stack() {
    for assignment in [false, true] {
        let mut expression = "x".to_owned();
        for _ in 0..255 {
            expression = if assignment {
                format!("{{ let mut value = x\nvalue = {expression}\nyield value }}")
            } else {
                format!("{{ let value: i64 = {expression}\nyield value }}")
            };
        }
        accepted(&format!("fn f(x: i64) -> i64 = {expression}\n"), |facts| {
            assert_eq!(facts.regions().len(), 255);
        });
    }
}

#[test]
fn nested_returns_record_only_the_innermost_reachable_transfer() {
    let source = format!(
        "fn f(x: i64) -> i64 = {}x{}\n",
        "{ return ".repeat(255),
        " }".repeat(255),
    );
    accepted(&source, |facts| {
        assert_eq!(facts.regions().len(), 255);
        let transfers = facts.transfers();
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].kind(), ControlTransferKind::Return);
        assert_eq!(transfers[0].exited_regions().len(), 255);
    });
}

#[test]
fn mixed_statement_frames_propagate_nested_poison_without_cascades() {
    let mut expression = "true".to_owned();
    for index in 0..255 {
        expression = match index % 3 {
            0 => format!("{{ let value: i64 = {expression}\nyield value }}"),
            1 => format!("{{ let mut value: i64 = 0\nvalue = {expression}\nyield value }}"),
            _ => format!("{{ yield {expression} }}"),
        };
    }
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(&format!("fn f() -> i64 = {expression}\n"));
    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    assert_eq!(
        analysis.resolution().unwrap().body_types().regions().len(),
        255
    );
    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["ASSIGNMENT_TYPE_MISMATCH"]
    );
}

#[test]
fn nested_unsupported_values_do_not_become_known_block_results() {
    let source = format!(
        "fn f() -> i64 = {}[1]{}\n",
        "{ yield ".repeat(250),
        " }".repeat(250),
    );
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(&source);
    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckContainers)
    }));
    let resolution = analysis.resolution().unwrap();
    let declaration = resolution
        .declarations()
        .modules()
        .next()
        .unwrap()
        .declarations()
        .next()
        .unwrap();
    assert!(matches!(
        resolution.body_types().body(declaration.id()),
        FactState::Unperformed
    ));
    assert!(resolution.body_types().transfers().is_empty());
    assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
}

#[test]
fn unreachable_nested_blocks_keep_only_their_local_yield_transfers() {
    let depth = 240;
    let value = format!(
        "{}source()?{}",
        "{ yield ".repeat(depth),
        " }".repeat(depth)
    );
    let source = format!(
        concat!(
            "fn stop() -> Never = stop()\n",
            "fn source() -> Res[i64, Str] = Ok(1)\n",
            "fn f() -> Res[i64, Str] = result {{\n",
            "  stop()\n",
            "  let value = {value}\n",
            "  yield Ok(value)\n",
            "}}\n",
        ),
        value = value,
    );
    accepted(&source, |facts| {
        assert!(
            !facts
                .transfers()
                .iter()
                .any(|transfer| { transfer.kind() == ControlTransferKind::QuestionShortCircuit })
        );
        assert_eq!(
            facts
                .transfers()
                .iter()
                .filter(|transfer| { transfer.kind() == ControlTransferKind::Yield })
                .count(),
            depth
        );
    });
}
