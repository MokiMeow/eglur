use std::cell::Cell;

use super::test_support::with_checker;
use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    merges: usize,
}

thread_local! { static WORK: Cell<Work> = const { Cell::new(Work { merges: 0 }) }; }

pub(super) fn observe_merge() {
    WORK.update(|work| Work {
        merges: work.merges + 1,
    });
}

#[test]
fn coverage_merge_and_variant_wrapping_preserve_irrefutability() {
    let variant = CanonicalTarget::External(crate::ExternalDefId::new("fixture::Variant").unwrap());
    for left in [false, true] {
        for right in [false, true] {
            let make = |total| {
                if total {
                    PatternCheck::irrefutable()
                } else {
                    PatternCheck::partial()
                }
            };
            let wrapped = PatternCheck::variant(variant.clone(), make(left));
            assert!(!wrapped.irrefutable);
            assert!(!wrapped.coverage.total);
            assert_eq!(
                wrapped.coverage.variants,
                if left {
                    BTreeSet::from([variant.clone()])
                } else {
                    BTreeSet::new()
                }
            );
            let mut first = make(left);
            WORK.set(Work::default());
            first.merge(make(right));
            assert_eq!(first.irrefutable, left && right);
            assert_eq!(first.coverage.total, left && right);
            assert!(first.coverage.variants.is_empty());
            assert_eq!(WORK.get().merges, 1);
        }
    }
}

#[test]
fn real_skewed_and_alias_patterns_merge_coverage_once_per_child() {
    for form in ["left", "right", "alias", "single"] {
        for count in [8_usize, 32, if form == "alias" { 64 } else { 128 }] {
            let mut ty = "i64".to_owned();
            let mut pattern = "leaf".to_owned();
            for index in 0..count {
                match form {
                    "left" => {
                        ty = format!("({ty}, i64)");
                        pattern = format!("({pattern}, v{index})");
                    }
                    "right" => {
                        ty = format!("(i64, {ty})");
                        pattern = format!("(v{index}, {pattern})");
                    }
                    "alias" => {
                        ty = format!("({ty},)");
                        pattern = format!("alias_{index} @ ({pattern},)");
                    }
                    _ => {
                        ty = format!("({ty},)");
                        pattern = format!("({pattern},)");
                    }
                }
            }
            let text = format!("fn f(input: {ty}) -> i64 = match input {{ {pattern} => leaf }}");
            with_checker(&text, |checker| {
                WORK.set(Work::default());
                checker.check_all().unwrap();
                assert!(!checker.diagnostics.has_errors(), "{form}/{count}");
                assert_eq!(
                    WORK.get(),
                    Work {
                        merges: if matches!(form, "left" | "right") {
                            2 * count
                        } else {
                            count
                        },
                    },
                    "{form}/{count}"
                );
                assert_eq!(
                    checker.names.locals().len(),
                    if form == "single" { 2 } else { count + 2 }
                );
                for local in checker.names.locals() {
                    let FactState::Known(fact) = checker.locals.state(&local.id()) else {
                        panic!("{} lost its installed type", local.name().spelling());
                    };
                    if local.name().spelling() != "input"
                        && !local.name().spelling().starts_with("alias_")
                    {
                        assert_eq!(
                            checker.signatures.types().kind(fact.ty()).unwrap(),
                            &TypeKind::Primitive(PrimitiveType::I64)
                        );
                    }
                }
            });
        }
    }
}
