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

fn tuple_type(width: usize) -> String {
    (0..width)
        .map(|index| if index % 2 == 0 { "i64" } else { "Bool" })
        .collect::<Vec<_>>()
        .join(", ")
}

#[test]
fn tuple_match_copies_only_written_children_and_keeps_their_types() {
    for width in [8, 32, 128] {
        for prefix in [0, 1, 3, width] {
            let repeats = if prefix == width { 2 } else { width };
            let mut text = format!(
                "type Wide = ({})\nfn f(input: Wide) -> i64 {{\n",
                tuple_type(width)
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
                text += &format!("let out_{row} = match input {{ {pattern} => 0 }}\n");
            }
            text += "yield 0\n}";

            with_checker(&text, |checker| {
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
                            panic!("tuple child lost its type: {name}")
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
                assert_eq!(checker.locals.len(), 1 + repeats * (prefix + 1));
            });
        }
    }
}

#[test]
fn tuple_match_checks_original_arity_and_kind_before_copying() {
    for width in [8, 32, 128] {
        for form in ["few", "many", "many-rest", "unit", "literal"] {
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
                "few" => format!("({names},)"),
                "many" => format!("({names})"),
                "many-rest" => format!("({names}, ..)"),
                "unit" => "(..)".into(),
                "literal" => "(true, ..)".into(),
                _ => unreachable!(),
            };
            let input = if form == "unit" { "Unit" } else { "Wide" };
            let text = format!(
                "type Wide = ({})\nfn f(input: {input}) -> i64 = match input {{ {pattern} => 0 }}",
                tuple_type(width)
            );
            with_checker(&text, |checker| {
                WORK.set(Work::default());
                checker.check_all().unwrap();
                assert_eq!(
                    WORK.get(),
                    if form == "literal" {
                        Work { copies: 1, rows: 1 }
                    } else {
                        Work::default()
                    },
                    "{form}/{width}"
                );
                assert_eq!(checker.locals.len(), checker.names.locals().len());
                for binding in checker.names.locals() {
                    assert!(
                        match checker.locals.state(&binding.id()) {
                            FactState::Known(_) => binding.name().spelling() == "input",
                            FactState::Poisoned(_) => binding.name().spelling() != "input",
                            _ => false,
                        },
                        "{form}/{width}: {binding:?}"
                    );
                }

                let snapshot =
                    std::mem::replace(checker.diagnostics, DiagnosticStore::new()).finalize(None);
                let diagnostics = snapshot.diagnostics();
                assert_eq!(diagnostics.len(), 1, "{form}/{width}: {diagnostics:?}");
                let diagnostic = &diagnostics[0];
                if matches!(form, "unit" | "literal") {
                    assert_eq!(diagnostic.code().as_str(), "PATTERN_TYPE_MISMATCH");
                } else {
                    assert_eq!(diagnostic.code().as_str(), "PATTERN_ARITY_MISMATCH");
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
