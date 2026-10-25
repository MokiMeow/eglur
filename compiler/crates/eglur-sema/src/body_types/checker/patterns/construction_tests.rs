use std::cell::Cell;

use super::*;
use crate::body_types::checker::test_support::with_checker;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    copies: usize,
    rows: usize,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work { copies: 0, rows: 0 }) };
}

pub(super) fn observe_copy(rows: usize) {
    WORK.update(|work| Work {
        copies: work.copies + 1,
        rows: work.rows + rows,
    });
}

fn payload(width: usize, named: bool) -> String {
    (0..width)
        .map(|index| {
            let ty = if index % 2 == 0 { "T" } else { "Bool" };
            if named {
                format!("field_{index}: {ty}")
            } else {
                ty.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[test]
fn variant_prefix_copies_follow_written_children_and_preserve_substituted_types() {
    for width in [8, 32, 128] {
        for prefix in [0, 1, 3, width] {
            let repeats = if prefix == width { 2 } else { width };
            let mut source = format!(
                "enum Wide[T] {{ Wrap({})\nEnd }}\nfn f(input: Wide[i64]) -> i64 {{\n",
                payload(width, false)
            );
            for row in 0..repeats {
                let names = (0..prefix)
                    .map(|column| format!("head_{row}_{column}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let pattern = match prefix {
                    0 => "(..)".to_owned(),
                    n if n == width => format!("({names})"),
                    _ => format!("({names}, ..)"),
                };
                source += &format!(
                    "let out_{row} = match input {{ Wide::Wrap{pattern} => 0, _ => 0 }}\n"
                );
            }
            source += "yield 0\n}";

            with_checker(&source, |checker| {
                WORK.set(Work::default());
                checker.check_all().unwrap();
                assert!(!checker.diagnostics.has_errors(), "{width}/{prefix}");
                assert_eq!(
                    WORK.get(),
                    Work {
                        copies: if prefix == 0 { 0 } else { repeats },
                        rows: prefix * repeats,
                    },
                    "{width}/{prefix}"
                );
                assert_eq!(checker.locals.len(), 1 + repeats * (prefix + 1));

                for row in 0..repeats {
                    for column in 0..prefix {
                        let name = format!("head_{row}_{column}");
                        let binding = checker
                            .names
                            .locals()
                            .iter()
                            .find(|binding| binding.name().spelling() == name)
                            .unwrap();
                        let FactState::Known(fact) = checker.locals.state(&binding.id()) else {
                            panic!("variant payload binding lost its type: {name}")
                        };
                        assert_eq!(
                            checker.signatures.types().kind(fact.ty).unwrap(),
                            &TypeKind::Primitive(if column % 2 == 0 {
                                PrimitiveType::I64
                            } else {
                                PrimitiveType::Bool
                            }),
                            "{width}/{prefix}/{name}"
                        );
                    }
                }
            });
        }
    }
}

#[test]
fn variant_head_form_and_full_arity_errors_do_not_copy_payloads() {
    for width in [8, 32, 128] {
        for form in ["few", "many", "many-rest", "absent", "result", "named"] {
            let explicit = if form.starts_with("many") {
                width + 1
            } else {
                1
            };
            let names = (0..explicit)
                .map(|index| format!("head_{index}"))
                .collect::<Vec<_>>()
                .join(", ");
            let pattern = match form {
                "few" | "many" => format!("({names})"),
                "many-rest" | "result" | "named" => format!("({names}, ..)"),
                "absent" => String::new(),
                _ => unreachable!(),
            };
            let fields = payload(width, form == "named");
            let declaration = if form == "named" {
                format!("enum Wide[T] {{ Wrap {{ {fields} }} }}")
            } else {
                format!("enum Wide[T] {{ Wrap({fields}) }}")
            };
            let input = if form == "result" {
                "Bool"
            } else {
                "Wide[i64]"
            };
            let source = format!(
                "{declaration}\nfn f(input: {input}) -> i64 = match input {{ Wide::Wrap{pattern} => 0 }}"
            );
            with_checker(&source, |checker| {
                WORK.set(Work::default());
                checker.check_all().unwrap();
                assert_eq!(WORK.get(), Work::default(), "{form}/{width}");
                for binding in checker.names.locals() {
                    assert!(match checker.locals.state(&binding.id()) {
                        FactState::Known(_) => binding.name().spelling() == "input",
                        FactState::Poisoned(_) => binding.name().spelling() != "input",
                        _ => false,
                    });
                }

                let snapshot =
                    std::mem::replace(checker.diagnostics, DiagnosticStore::new()).finalize(None);
                let diagnostics = snapshot.diagnostics();
                assert_eq!(diagnostics.len(), 1, "{form}/{width}: {diagnostics:?}");
                let diagnostic = &diagnostics[0];
                let code = match form {
                    "absent" | "named" => "PATTERN_PAYLOAD_FORM_MISMATCH",
                    "result" => "PATTERN_TYPE_MISMATCH",
                    _ => "PATTERN_ARITY_MISMATCH",
                };
                assert_eq!(diagnostic.code().as_str(), code);
                if code == "PATTERN_ARITY_MISMATCH" {
                    assert_eq!(
                        diagnostic.data().get("expected_count"),
                        Some(&DiagnosticDataValue::Integer(width as i64))
                    );
                    assert_eq!(
                        diagnostic.data().get("actual_count"),
                        Some(&DiagnosticDataValue::Integer(explicit as i64))
                    );
                }
            });
        }
    }
}

#[test]
fn absent_and_empty_positional_variants_keep_distinct_payload_forms() {
    for (declaration, pattern, valid) in [
        ("Wrap", "Wrap", true),
        ("Wrap()", "Wrap()", true),
        ("Wrap", "Wrap()", false),
        ("Wrap()", "Wrap", false),
    ] {
        let source = format!(
            "enum Empty {{ {declaration}\nOther }}\nfn f(input: Empty) -> i64 = match input {{ Empty::{pattern} => 0, _ => 0 }}"
        );
        with_checker(&source, |checker| {
            WORK.set(Work::default());
            checker.check_all().unwrap();
            assert_eq!(WORK.get(), Work::default());
            let snapshot =
                std::mem::replace(checker.diagnostics, DiagnosticStore::new()).finalize(None);
            let diagnostics = snapshot.diagnostics();
            assert_eq!(diagnostics.is_empty(), valid, "{source}");
            if !valid {
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(
                    diagnostics[0].code().as_str(),
                    "PATTERN_PAYLOAD_FORM_MISMATCH"
                );
            }
        });
    }
}
