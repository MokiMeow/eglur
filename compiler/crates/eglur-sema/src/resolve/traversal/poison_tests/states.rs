use super::*;
use crate::resolve::{NameResolutionError, ResolvedName};
use crate::{CanonicalTarget, DefId, LocalBindingId};
use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticStoreError, Poison, PoisonAccumulator, PrimarySpan, Severity,
};

#[derive(Clone, Copy, Debug)]
enum State {
    Known,
    Declaration,
    Local,
    Owner,
    First,
    Second,
    Wide,
    Foreign,
    Unperformed,
    NotApplicable,
}

struct Inputs {
    roots: Vec<Poison>,
    wide: Poison,
    foreign: Poison,
    declaration: DefId,
    local: LocalBindingId,
    owner: DefId,
}

impl Inputs {
    fn outcome(&self, state: State) -> VisitOutcome {
        match state {
            State::Known => VisitOutcome::Known,
            State::Declaration => VisitOutcome::Resolved(ResolvedName::Declarations(Box::new([
                CanonicalTarget::Source(self.declaration),
            ]))),
            State::Local => VisitOutcome::Resolved(ResolvedName::Local(self.local)),
            State::Owner => VisitOutcome::Resolved(ResolvedName::TypeMemberOwner(
                CanonicalTarget::Source(self.owner),
            )),
            State::First => VisitOutcome::Poisoned(self.roots[1].clone()),
            State::Second => VisitOutcome::Poisoned(self.roots[2].clone()),
            State::Wide => VisitOutcome::Poisoned(self.wide.clone()),
            State::Foreign => VisitOutcome::Poisoned(self.foreign.clone()),
            State::Unperformed => VisitOutcome::Unperformed,
            State::NotApplicable => VisitOutcome::NotApplicable,
        }
    }
}

fn root(store: &mut DiagnosticStore, index: usize) -> Poison {
    let diagnostic = Diagnostic::new(
        DiagnosticCode::new(format!("ROOT_{index}")).unwrap(),
        Severity::Error,
        DiagnosticPhase::new("resolve").unwrap(),
        "independent sequence failure",
        PrimarySpan::Unavailable,
    );
    let root = DiagnosticRoot::boundary(
        BoundaryRootKey::new(format!("root-{index}")).unwrap(),
        diagnostic,
    )
    .unwrap();
    store.record_failure(DiagnosticFailure::Root(root)).unwrap()
}

fn image(outcome: &VisitOutcome) -> (u8, Option<ResolvedName>) {
    match outcome {
        VisitOutcome::Known => (0, None),
        VisitOutcome::Resolved(value) => (1, Some(value.clone())),
        VisitOutcome::Poisoned(_) => (2, None),
        VisitOutcome::Unperformed => (3, None),
        VisitOutcome::NotApplicable => (4, None),
    }
}

fn check_roots(mut store: DiagnosticStore, poison: &Poison, all: &Poison, expected: &[usize]) {
    store.select_preceding_roots(poison, all).unwrap();
    let snapshot = store.finalize(None);
    let mut codes = snapshot
        .diagnostics()
        .iter()
        .map(|d| d.code().as_str().to_owned())
        .collect::<Vec<_>>();
    let mut expected = expected
        .iter()
        .map(|index| format!("ROOT_{index}"))
        .collect::<Vec<_>>();
    codes.sort();
    expected.sort();
    assert_eq!(codes, expected);
}

#[test]
fn actual_sequence_window_preserves_optional_seed_identity_partial_states_and_foreign_frontier() {
    with_resolver(
        "struct S {}\nfn declared() -> Unit = ()\nfn probe(x: i64) -> Unit = ()\n",
        |resolver, source| {
            let module = resolver.collection.module_for_file(source.file()).unwrap();
            let hir = source.hir().unwrap();
            for &item in hir.roots() {
                resolver.visit_item(module, source, hir, item).unwrap();
            }
            let declaration = resolver
                .collection
                .modules()
                .flat_map(|m| m.declarations())
                .find(|d| d.name().is_some_and(|n| n.text() == "declared"))
                .unwrap()
                .id();
            let owner = resolver
                .collection
                .modules()
                .flat_map(|m| m.declarations())
                .find(|d| d.name().is_some_and(|n| n.text() == "S"))
                .unwrap()
                .id();
            let local = resolver
                .locals
                .iter()
                .find(|local| local.name().text() == "x")
                .unwrap()
                .id();
            let states = [
                State::Known,
                State::Declaration,
                State::Local,
                State::Owner,
                State::First,
                State::Second,
                State::Wide,
                State::Foreign,
                State::Unperformed,
                State::NotApplicable,
            ];
            let seeds = std::iter::once(None)
                .chain(states.into_iter().map(Some))
                .collect::<Vec<_>>();
            let mut cases = vec![Vec::new()];
            for a in states {
                cases.push(vec![a]);
                for b in states {
                    cases.push(vec![a, b]);
                    for c in states {
                        cases.push(vec![a, b, c]);
                    }
                }
            }

            for seed in seeds {
                for children in &cases {
                    let mut store = DiagnosticStore::new();
                    let roots = (0..9)
                        .map(|index| root(&mut store, index))
                        .collect::<Vec<_>>();
                    let mut all = PoisonAccumulator::new();
                    for root in &roots {
                        all.include(root).unwrap();
                    }
                    let all = all.finish().unwrap();
                    let mut wide = PoisonAccumulator::new();
                    for root in &roots[1..] {
                        wide.include(root).unwrap();
                    }
                    let wide = wide.finish().unwrap();
                    let mut foreign_store = DiagnosticStore::new();
                    let foreign = root(&mut foreign_store, 999);
                    let inputs = Inputs {
                        roots,
                        wide,
                        foreign,
                        declaration,
                        local,
                        owner,
                    };

                    let mut expected = seed.map(|state| inputs.outcome(state));
                    let reference = (|| {
                        for &state in children {
                            let next = inputs.outcome(state);
                            expected = Some(match expected.take() {
                                Some(previous) => previous.merge(next)?,
                                None => next,
                            });
                        }
                        Ok::<_, NameResolutionError>(expected.take().unwrap_or(VisitOutcome::Known))
                    })();
                    let prefix = inputs.outcome(State::Declaration);
                    let mut traversal = Traversal::default();
                    traversal.outcomes.push(prefix.clone());
                    traversal
                        .outcomes
                        .extend(children.iter().map(|&state| inputs.outcome(state)));
                    reset();
                    let actual = traversal
                        .complete_sequence(seed.map(|state| inputs.outcome(state)), children.len());
                    let count = seed
                        .iter()
                        .chain(children.iter())
                        .filter(|&&state| {
                            matches!(
                                state,
                                State::First | State::Second | State::Wide | State::Foreign
                            )
                        })
                        .count();
                    assert_eq!(image(&traversal.outcomes[0]), image(&prefix));
                    match (reference, actual) {
                        (
                            Err(NameResolutionError::DiagnosticStore(
                                DiagnosticStoreError::ForeignPoison,
                            )),
                            Err(NameResolutionError::DiagnosticStore(
                                DiagnosticStoreError::ForeignPoison,
                            )),
                        ) => {
                            assert_eq!(traversal.outcomes.len(), 1);
                            assert_eq!(work().finishes, 0);
                        }
                        (Ok(expected), Ok(())) => {
                            assert_eq!(traversal.outcomes.len(), 2);
                            let actual = traversal.outcome();
                            assert_eq!(image(&actual), image(&expected), "{seed:?}/{children:?}");
                            assert_eq!(
                                work(),
                                Work {
                                    pairs: usize::from(count > 1),
                                    includes: if count > 2 { count - 1 } else { 0 },
                                    finishes: usize::from(count > 2)
                                }
                            );
                            if let VisitOutcome::Poisoned(poison) = actual {
                                let sequence = seed
                                    .iter()
                                    .chain(children.iter())
                                    .copied()
                                    .collect::<Vec<_>>();
                                let foreign =
                                    sequence.iter().any(|state| matches!(state, State::Foreign));
                                if foreign {
                                    check_roots(foreign_store, &poison, &inputs.foreign, &[999]);
                                } else {
                                    let expected = if sequence
                                        .iter()
                                        .any(|state| matches!(state, State::Wide))
                                    {
                                        (1..9).collect::<Vec<_>>()
                                    } else {
                                        let mut expected = Vec::new();
                                        if sequence
                                            .iter()
                                            .any(|state| matches!(state, State::First))
                                        {
                                            expected.push(1);
                                        }
                                        if sequence
                                            .iter()
                                            .any(|state| matches!(state, State::Second))
                                        {
                                            expected.push(2);
                                        }
                                        expected
                                    };
                                    check_roots(store, &poison, &all, &expected);
                                }
                            }
                        }
                        _ => panic!("sequence disposition changed for {seed:?}/{children:?}"),
                    }
                }
            }
        },
    );
}
