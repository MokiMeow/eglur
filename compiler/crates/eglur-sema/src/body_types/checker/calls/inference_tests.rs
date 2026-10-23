use std::cell::Cell;

use super::*;
use crate::body_types::checker::test_support::with_checker;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    visits: usize,
    pairs: usize,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work { visits: 0, pairs: 0 }) };
}

pub(super) fn observe_visit() {
    WORK.update(|work| Work {
        visits: work.visits + 1,
        ..work
    });
}

pub(super) fn observe_pairs(pairs: usize) {
    WORK.update(|work| Work {
        pairs: work.pairs + pairs,
        ..work
    });
}

struct Fixture {
    parameters: [GenericParameterId; 2],
    generic: [TypeId; 2],
    integer: TypeId,
    boolean: TypeId,
}

impl Fixture {
    fn new(checker: &mut BodyChecker<'_, '_, '_>) -> Self {
        let parameters: [GenericParameterId; 2] = checker
            .collection
            .modules()
            .flat_map(|module| module.declarations())
            .filter(|declaration| declaration.kind() == DeclarationKind::GenericParameter)
            .map(|declaration| GenericParameterId::Source(declaration.id()))
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let types = checker.signatures.types_mut();
        let generic = [
            types.generic_parameter(parameters[0].clone()).unwrap(),
            types.generic_parameter(parameters[1].clone()).unwrap(),
        ];
        Self {
            parameters,
            generic,
            integer: types.primitive(PrimitiveType::I64).unwrap(),
            boolean: types.primitive(PrimitiveType::Bool).unwrap(),
        }
    }

    fn active(&self) -> BTreeSet<GenericParameterId> {
        self.parameters.iter().cloned().collect()
    }
}

const SEED: &str = "fn seed[T, U](left: T, right: U) -> Unit = ()";

#[test]
fn generic_evidence_keeps_identity_existing_bindings_and_never_rules() {
    with_checker(SEED, |checker| {
        let fixture = Fixture::new(checker);
        let active = fixture.active();
        let mut replacements = BTreeMap::new();
        assert_eq!(
            checker
                .infer_constraints(
                    fixture.generic[0],
                    fixture.boolean,
                    &BTreeSet::new(),
                    &mut replacements
                )
                .unwrap(),
            ConstraintOutcome::NoEvidence
        );
        assert!(replacements.is_empty());

        for actual in [
            fixture.boolean,
            checker.signatures.types().never(),
            fixture.integer,
        ] {
            let expected = if actual == fixture.integer {
                ConstraintOutcome::Conflict
            } else {
                ConstraintOutcome::Evidence
            };
            assert_eq!(
                checker
                    .infer_constraints(fixture.generic[0], actual, &active, &mut replacements)
                    .unwrap(),
                expected
            );
            assert_eq!(
                replacements,
                BTreeMap::from([(fixture.parameters[0].clone(), fixture.boolean)])
            );
        }
        assert_eq!(
            checker
                .infer_constraints(
                    fixture.generic[1],
                    fixture.integer,
                    &active,
                    &mut replacements
                )
                .unwrap(),
            ConstraintOutcome::Evidence
        );
        assert_eq!(
            replacements.get(&fixture.parameters[1]),
            Some(&fixture.integer)
        );

        let mut empty = BTreeMap::new();
        let never = checker.signatures.types().never();
        assert_eq!(
            checker
                .infer_constraints(fixture.generic[0], never, &active, &mut empty)
                .unwrap(),
            ConstraintOutcome::Evidence
        );
        assert_eq!(empty.get(&fixture.parameters[0]), Some(&never));
    });
}

#[test]
fn tuple_callable_and_nominal_pairs_keep_order_and_first_conflict() {
    with_checker(SEED, |checker| {
        let fixture = Fixture::new(checker);
        let active = fixture.active();
        let types = checker.signatures.types_mut();
        let tuple = types
            .tuple([fixture.generic[0], fixture.generic[0], fixture.generic[1]])
            .unwrap();
        let actual = types
            .tuple([fixture.boolean, fixture.integer, fixture.boolean])
            .unwrap();
        let callable = types.callable(fixture.generic, fixture.generic[0]).unwrap();
        let good = types
            .callable([fixture.boolean, fixture.integer], fixture.boolean)
            .unwrap();
        let bad = types
            .callable([fixture.boolean, fixture.integer], fixture.integer)
            .unwrap();
        let short = types.callable([fixture.boolean], fixture.boolean).unwrap();
        let opt = types
            .nominal(
                NominalHead::Intrinsic(IntrinsicNominal::Opt),
                [fixture.generic[0]],
            )
            .unwrap();
        let opt_actual = types
            .nominal(
                NominalHead::Intrinsic(IntrinsicNominal::Opt),
                [fixture.boolean],
            )
            .unwrap();
        let boxed = types
            .nominal(
                NominalHead::Intrinsic(IntrinsicNominal::Box),
                [fixture.boolean],
            )
            .unwrap();

        for (template, actual, outcome, visits, pairs, binds_second) in [
            (tuple, actual, ConstraintOutcome::Conflict, 3, 3, false),
            (callable, good, ConstraintOutcome::Evidence, 4, 3, true),
            (callable, bad, ConstraintOutcome::Conflict, 4, 3, true),
            (callable, short, ConstraintOutcome::NoEvidence, 1, 0, false),
            (opt, opt_actual, ConstraintOutcome::Evidence, 2, 1, false),
            (opt, boxed, ConstraintOutcome::NoEvidence, 1, 0, false),
        ] {
            WORK.set(Work::default());
            let mut replacements = BTreeMap::new();
            assert_eq!(
                checker
                    .infer_constraints(template, actual, &active, &mut replacements)
                    .unwrap(),
                outcome
            );
            assert_eq!(WORK.get(), Work { visits, pairs });
            if outcome == ConstraintOutcome::NoEvidence {
                assert!(replacements.is_empty());
            } else {
                assert_eq!(
                    replacements.get(&fixture.parameters[0]),
                    Some(&fixture.boolean)
                );
                assert_eq!(
                    replacements.get(&fixture.parameters[1]),
                    binds_second.then_some(&fixture.integer)
                );
            }
        }
    });
}

#[test]
fn reference_and_raw_pointer_variance_keep_direction_and_binding() {
    with_checker(SEED, |checker| {
        let fixture = Fixture::new(checker);
        let active = fixture.active();
        for pointer in [false, true] {
            for template_mutable in [false, true] {
                for actual_mutable in [false, true] {
                    let types = checker.signatures.types_mut();
                    let (template, actual) = if pointer {
                        let mutability = |mutable| {
                            if mutable {
                                RawPointerMutability::Mutable
                            } else {
                                RawPointerMutability::Const
                            }
                        };
                        (
                            types
                                .raw_pointer(mutability(template_mutable), fixture.generic[0])
                                .unwrap(),
                            types
                                .raw_pointer(mutability(actual_mutable), fixture.integer)
                                .unwrap(),
                        )
                    } else {
                        let mutability = |mutable| {
                            if mutable {
                                ReferenceMutability::Mutable
                            } else {
                                ReferenceMutability::Shared
                            }
                        };
                        (
                            types
                                .reference(mutability(template_mutable), fixture.generic[0])
                                .unwrap(),
                            types
                                .reference(mutability(actual_mutable), fixture.integer)
                                .unwrap(),
                        )
                    };
                    let admitted = !template_mutable || actual_mutable;
                    WORK.set(Work::default());
                    let mut replacements = BTreeMap::new();
                    assert_eq!(
                        checker
                            .infer_constraints(template, actual, &active, &mut replacements)
                            .unwrap(),
                        if admitted {
                            ConstraintOutcome::Evidence
                        } else {
                            ConstraintOutcome::NoEvidence
                        }
                    );
                    assert_eq!(
                        WORK.get(),
                        Work {
                            visits: if admitted { 2 } else { 1 },
                            pairs: usize::from(admitted)
                        }
                    );
                    assert_eq!(
                        replacements.get(&fixture.parameters[0]),
                        admitted.then_some(&fixture.integer)
                    );
                }
            }
        }
    });
}

#[test]
fn incompatible_wide_kinds_do_not_visit_or_pair_unrelated_children() {
    with_checker(SEED, |checker| {
        let fixture = Fixture::new(checker);
        for width in [8, 32, 128] {
            let types = checker.signatures.types_mut();
            let tuple = types.tuple(vec![fixture.integer; width]).unwrap();
            let callable = types
                .callable(vec![fixture.integer; width], fixture.integer)
                .unwrap();
            for actual in [tuple, callable] {
                WORK.set(Work::default());
                let mut replacements = BTreeMap::new();
                assert_eq!(
                    checker
                        .infer_constraints(
                            fixture.boolean,
                            actual,
                            &fixture.active(),
                            &mut replacements
                        )
                        .unwrap(),
                    ConstraintOutcome::NoEvidence
                );
                assert_eq!(
                    WORK.get(),
                    Work {
                        visits: 1,
                        pairs: 0
                    }
                );
                assert!(replacements.is_empty());
            }
        }
    });
}
