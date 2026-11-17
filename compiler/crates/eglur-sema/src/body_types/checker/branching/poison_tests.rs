use super::*;
use crate::body_types::checker::poison::tests::{Work, reset, selected, union, work};
use crate::body_types::checker::test_support::with_checker;

mod states;

fn source(mode: &str, count: usize) -> String {
    let prefix = if mode == "shared" {
        "type Bad = Missing\nfn bad() -> Bad = ()\n"
    } else {
        ""
    };
    let arms = (0..count)
        .map(|index| match mode {
            "distinct" => format!("{index} => missing_{index},\n"),
            "shared" => format!("{index} => bad(),\n"),
            "guards" => format!("{index} if missing_{index} => 0,\n"),
            "mismatch" if index == 0 => "0 => \"anchor\",\n".to_owned(),
            "mismatch" | "healthy" => format!("{index} => 0,\n"),
            "patterns" => "true => 0,\n".to_owned(),
            _ => panic!("unknown source family"),
        })
        .collect::<String>();
    let fallback = if mode == "mismatch" {
        "\"anchor\""
    } else {
        "0"
    };
    format!(
        "{prefix}fn probe(x: i64) -> i64 = match x {{ {arms}_ => {fallback} }}\nfn after() -> Unit = ()\n"
    )
}

#[test]
fn prior_arm_poison_suppresses_coverage_without_suppressing_later_values() {
    with_checker(
        "fn probe(x: i64) -> i64 = match x { _ => missing, 0 => 0 }\nfn after() -> Unit = ()\n",
        |checker| {
            reset();
            checker.check_all().unwrap();
            assert_eq!(
                work(),
                Work {
                    inputs: 1,
                    completed: 1
                }
            );
            let diagnostics =
                std::mem::replace(checker.diagnostics, DiagnosticStore::new()).finalize(None);
            assert_eq!(
                diagnostics
                    .diagnostics()
                    .iter()
                    .map(|d| d.code().as_str())
                    .collect::<Vec<_>>(),
                ["NAME_NOT_FOUND"]
            );
        },
    );
    with_checker(
        "fn probe(x: i64) -> i64 = match x { _ => 0, 0 => 1 }\n",
        |checker| {
            reset();
            checker.check_all().unwrap();
            assert_eq!(
                work(),
                Work {
                    inputs: 1,
                    completed: 1
                }
            );
            let diagnostics =
                std::mem::replace(checker.diagnostics, DiagnosticStore::new()).finalize(None);
            assert_eq!(
                diagnostics
                    .diagnostics()
                    .iter()
                    .map(|d| d.code().as_str())
                    .collect::<Vec<_>>(),
                ["MATCH_UNREACHABLE_ARM"]
            );
        },
    );
}

#[test]
fn match_sources_complete_only_the_final_union_after_all_arms() {
    for count in [8, 32, 128] {
        for mode in [
            "distinct", "shared", "guards", "mismatch", "patterns", "healthy",
        ] {
            with_checker(&source(mode, count), |checker| {
                reset();
                checker.check_all().unwrap();
                let inputs = match mode {
                    "healthy" => 0,
                    "mismatch" => count - 1,
                    _ => count,
                };
                assert_eq!(
                    work(),
                    Work {
                        inputs,
                        completed: usize::from(inputs != 0)
                    },
                    "{mode}/{count}"
                );
                assert!(checker.region_stack.is_empty());
                let probe = checker
                    .collection
                    .modules()
                    .flat_map(|module| module.declarations())
                    .find(|declaration| {
                        declaration
                            .name()
                            .is_some_and(|name| name.text() == "probe")
                    })
                    .unwrap()
                    .id();
                let after = checker
                    .collection
                    .modules()
                    .flat_map(|module| module.declarations())
                    .find(|declaration| {
                        declaration
                            .name()
                            .is_some_and(|name| name.text() == "after")
                    })
                    .unwrap()
                    .id();
                assert!(matches!(checker.bodies.state(&after), FactState::Known(_)));

                if matches!(mode, "distinct" | "shared" | "guards") {
                    let FactState::Poisoned(poison) = checker.bodies.state(&probe) else {
                        panic!("poisoned match body expected")
                    };
                    let poison = poison.clone();
                    let children = checker
                        .expressions
                        .iter()
                        .filter_map(|(_, fact)| match fact {
                            SemanticFact::Poisoned(poison) => Some(poison.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    let all = union(&children, 0..children.len());
                    let root_count = if mode == "shared" { 1 } else { count };
                    assert_eq!(
                        selected(checker.diagnostics, &poison, &all),
                        vec!["NAME_NOT_FOUND".to_owned(); root_count]
                    );
                } else {
                    let diagnostics =
                        std::mem::replace(checker.diagnostics, DiagnosticStore::new())
                            .finalize(None);
                    let expected = match mode {
                        "mismatch" => "MATCH_ARM_TYPE_MISMATCH",
                        "patterns" => "PATTERN_TYPE_MISMATCH",
                        _ => "",
                    };
                    assert_eq!(
                        diagnostics
                            .diagnostics()
                            .iter()
                            .map(|d| d.code().as_str())
                            .collect::<Vec<_>>(),
                        vec![expected; inputs],
                        "{mode}/{count}"
                    );
                }
            });
        }
    }
}
