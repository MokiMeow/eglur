mod parity;
mod test_inputs;

use test_inputs::with_file_set;

use std::cell::RefCell;

use super::*;
use crate::signatures::resolver::tests::with_resolver;
use crate::{
    AnalysisRootIdentity, AnalysisSourceInputFailure, AnalysisSourceInputs, AnalysisWork,
    AnalysisWorkAccounting, AnalysisWorkPricing,
};

const ALLOWANCE: u64 = 1_000_000;

struct Support;

impl CompileSupport for Support {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

#[derive(Clone, Copy, Debug)]
struct Visit {
    occurrence: TypeOccurrenceId,
    root: AnalysisRootIdentity,
}

struct Trace {
    visits: Vec<Visit>,
    claims: Vec<AnalysisRootIdentity>,
    stop_at: Option<usize>,
}

thread_local! {
    static TRACE: RefCell<Option<Trace>> = const { RefCell::new(None) };
}

struct TraceScope;

impl TraceScope {
    fn new(stop_at: Option<usize>) -> Self {
        TRACE.with_borrow_mut(|trace| {
            assert!(trace.is_none());
            *trace = Some(Trace {
                visits: Vec::new(),
                claims: Vec::new(),
                stop_at,
            });
        });
        Self
    }

    fn visits(&self) -> Vec<Visit> {
        TRACE.with_borrow(|trace| trace.as_ref().unwrap().visits.clone())
    }

    fn claims(&self, root: AnalysisRootIdentity) -> usize {
        TRACE.with_borrow(|trace| {
            trace
                .as_ref()
                .unwrap()
                .claims
                .iter()
                .filter(|claim| **claim == root)
                .count()
        })
    }
}

pub(in crate::signatures::resolver::source_types) fn record_claim(root: AnalysisRootIdentity) {
    TRACE.with_borrow_mut(|trace| {
        if let Some(trace) = trace {
            trace.claims.push(root);
        }
    });
}

impl Drop for TraceScope {
    fn drop(&mut self) {
        TRACE.with_borrow_mut(|trace| *trace = None);
    }
}

pub(super) fn observe(
    resolver: &mut SignatureResolver<'_, '_>,
    occurrence: TypeOccurrenceId,
    custody: &SourceCustody,
) -> Result<(), SignatureFlow> {
    let SourceCustody::Ticket(ticket) = custody else {
        return Ok(());
    };
    let stop = TRACE.with_borrow_mut(|trace| {
        let Some(trace) = trace else { return false };
        trace.visits.push(Visit {
            occurrence,
            root: ticket.identity(),
        });
        trace.stop_at == Some(trace.visits.len())
    });
    if stop {
        resolver
            .analysis_work
            .as_mut()
            .unwrap()
            .accounting
            .claim(ticket)?
            .precharge(AnalysisWork {
                steps: u64::MAX,
                inspected_bytes: 0,
                transferred_bytes: 0,
                reserved_bytes: 0,
            })?;
    }
    Ok(())
}

fn accounting(consumers: usize) -> AnalysisWorkAccounting {
    AnalysisWorkAccounting::new(AnalysisWorkPricing::for_test(1, 1, 1).unwrap(), consumers).unwrap()
}

fn completed(result: SignatureExecution) -> ResolvedDeclarationSemantics {
    let SignatureExecution::Completed(phase) = result else {
        panic!("interrupted")
    };
    let PhaseProduct::Performed(product) = phase.into_parts().0 else {
        panic!("unperformed")
    };
    product
}

fn heads(hir: &HirFile) -> Vec<TypeRefId> {
    let mut children = BTreeSet::new();
    for (_, ty) in hir.type_refs() {
        match ty.kind() {
            TypeRefKind::Reference { referent, .. }
            | TypeRefKind::Pointer { referent, .. }
            | TypeRefKind::Group(referent)
            | TypeRefKind::Dynamic {
                trait_ref: referent,
            } => {
                children.insert(*referent);
            }
            TypeRefKind::Tuple(arguments) | TypeRefKind::Path { arguments, .. } => {
                children.extend(arguments.iter().copied())
            }
            TypeRefKind::Function { parameters, result } => {
                children.extend(parameters.iter().copied());
                children.insert(*result);
            }
            TypeRefKind::Invalid => {}
        }
    }
    hir.type_refs()
        .map(|(id, _)| id)
        .filter(|id| !children.contains(id))
        .collect()
}

fn alias_rhs(hir: &HirFile, name: &str) -> TypeRefId {
    hir.items()
        .find_map(|(_, item)| match item.kind() {
            ItemKind::TypeAlias(value) if value.name.text() == Some(name) => Some(value.value),
            _ => None,
        })
        .unwrap()
}

fn run<'hir>(
    resolver: &SignatureResolver<'_, '_>,
    sources: &[SemanticSource<'hir>],
    diagnostics: &mut DiagnosticStore,
    work: &mut AnalysisWorkAccounting,
    inputs: &mut AnalysisSourceInputs<'hir>,
    allowance: &dyn Fn(TypeOccurrenceId) -> u64,
) -> Result<SignatureExecution, SignatureResolutionError> {
    resolve_declaration_signatures_with_work(
        resolver.collection,
        resolver.imports,
        sources,
        &Support,
        SignatureResolutionInputs::new(resolver.external_names, None),
        diagnostics,
        SignatureAnalysisWork::new(work, inputs, allowance),
    )
}

#[test]
fn active_foreign_missing_and_stale_source_inputs_refuse_before_admission() {
    with_resolver(&["type Input = i32"], |resolver, sources| {
        let original_work = accounting(1);
        let mut foreign_inputs =
            AnalysisSourceInputs::new(&original_work, [(0, &sources[0])]).unwrap();
        let mut work = accounting(1);
        let mut diagnostics = DiagnosticStore::new();
        let failure = run(
            resolver,
            sources,
            &mut diagnostics,
            &mut work,
            &mut foreign_inputs,
            &|_| panic!("invalid input called policy"),
        );
        assert!(matches!(
            failure,
            Err(SignatureResolutionError::AnalysisSourceInput(
                AnalysisSourceInputFailure::Work(crate::AnalysisWorkFailure::Claim(
                    crate::AnalysisRootClaimError::ForeignAttempt
                ))
            ))
        ));
        assert!(work.incomplete().is_none());
        assert!(!diagnostics.has_errors());
        assert_eq!(
            work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
            0
        );

        let mut work = accounting(1);
        let mut missing = AnalysisSourceInputs::new(&work, []).unwrap();
        assert!(matches!(
            run(
                resolver,
                sources,
                &mut diagnostics,
                &mut work,
                &mut missing,
                &|_| panic!("missing input called policy")
            ),
            Err(SignatureResolutionError::AnalysisSourceInput(
                AnalysisSourceInputFailure::MissingFile(_)
            ))
        ));
        assert!(work.incomplete().is_none());
        assert_eq!(
            work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
            0
        );

        let mut work = accounting(1);
        let mut inputs = AnalysisSourceInputs::new(&work, [(0, &sources[0])]).unwrap();
        let hir = sources[0].hir().unwrap().clone();
        let stale = SemanticSource::new(
            sources[0].file(),
            sources[0].module().clone(),
            Some(&hir),
            hir.origins()
                .map(|(id, _)| (id, sources[0].origin_span(id).unwrap().clone())),
            [],
            None,
        )
        .unwrap();
        assert!(matches!(
            run(
                resolver,
                &[stale],
                &mut diagnostics,
                &mut work,
                &mut inputs,
                &|_| panic!("stale input called policy")
            ),
            Err(SignatureResolutionError::AnalysisSourceInput(
                AnalysisSourceInputFailure::SourceMismatch(_)
            ))
        ));
        assert!(work.incomplete().is_none());
        assert_eq!(
            work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
            0
        );
    });
}

#[test]
fn duplicate_source_setup_creates_no_balances() {
    with_resolver(&["type Input = i32"], |_, sources| {
        let mut work = accounting(1);
        assert!(matches!(
            AnalysisSourceInputs::new(&work, [(0, &sources[0]), (0, &sources[0])]),
            Err(AnalysisSourceInputFailure::DuplicateFile(_))
        ));
        assert!(work.incomplete().is_none());
        assert_eq!(
            work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
            0
        );
    });
}

#[test]
fn cross_file_alias_expansion_keeps_the_demanding_file_root() {
    with_file_set(
        &[
            (
                "main",
                "import library::Pair\ntype First = Pair[i32]\ntype Second = Pair[Bool]",
            ),
            ("library", "pub type Pair[T] = (T, T)"),
        ],
        |resolver, sources| {
            let main = &sources[0];
            let library = &sources[1];
            let first = TypeOccurrenceId::new(main.file(), alias_rhs(main.hir().unwrap(), "First"));
            let pair =
                TypeOccurrenceId::new(library.file(), alias_rhs(library.hir().unwrap(), "Pair"));
            let mut work = accounting(sources.len());
            let mut inputs = AnalysisSourceInputs::new(&work, sources.iter().enumerate()).unwrap();
            let trace = TraceScope::new(None);
            let mut diagnostics = DiagnosticStore::new();
            completed(
                run(
                    resolver,
                    sources,
                    &mut diagnostics,
                    &mut work,
                    &mut inputs,
                    &|_| ALLOWANCE,
                )
                .unwrap(),
            );
            assert!(!diagnostics.has_errors());
            let first_root = inputs.admitted_ticket(first).unwrap().unwrap().identity();
            let pair_root = inputs.admitted_ticket(pair).unwrap().unwrap().identity();
            assert_ne!(first_root.consumer(), pair_root.consumer());
            assert!(
                trace
                    .visits()
                    .iter()
                    .any(|visit| visit.occurrence == pair && visit.root == first_root)
            );
            assert!(
                trace
                    .visits()
                    .iter()
                    .any(|visit| visit.occurrence == pair && visit.root == pair_root)
            );
            for (consumer, source) in sources.iter().enumerate() {
                assert_eq!(
                    work.admit(consumer, ALLOWANCE)
                        .unwrap()
                        .identity()
                        .requirement(),
                    heads(source.hir().unwrap()).len()
                );
            }
        },
    );
}

#[test]
fn every_written_head_admits_once_and_structural_children_inherit() {
    with_resolver(
        &[
            "type Pair[T] = (T, T)\nfn inspect(x: &(i32, Fn(i32) -> Bool), y: Pair[Opt[(i32, Bool)]]) -> i32 = 0",
        ],
        |resolver, sources| {
            let hir = sources[0].hir().unwrap();
            let originals = heads(hir);
            let mut work = accounting(1);
            let mut inputs = AnalysisSourceInputs::new(&work, [(0, &sources[0])]).unwrap();
            let calls = RefCell::new(Vec::new());
            let allowance = |occurrence| {
                calls.borrow_mut().push(occurrence);
                ALLOWANCE
            };
            let trace = TraceScope::new(None);
            let mut diagnostics = DiagnosticStore::new();
            completed(
                run(
                    resolver,
                    sources,
                    &mut diagnostics,
                    &mut work,
                    &mut inputs,
                    &allowance,
                )
                .unwrap(),
            );
            assert!(diagnostics.finalize(None).diagnostics().is_empty());
            let expected = originals
                .iter()
                .map(|&id| TypeOccurrenceId::new(sources[0].file(), id))
                .collect::<BTreeSet<_>>();
            assert_eq!(
                calls.borrow().iter().copied().collect::<BTreeSet<_>>(),
                expected
            );
            assert_eq!(calls.borrow().len(), expected.len());
            assert_eq!(
                work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
                expected.len()
            );
            for head in originals {
                let site = TypeOccurrenceId::new(sources[0].file(), head);
                let ticket = inputs.admitted_ticket(site).unwrap().unwrap();
                assert!(
                    trace
                        .visits()
                        .iter()
                        .any(|visit| visit.occurrence == site && visit.root == ticket.identity())
                );
            }
            for (id, _) in hir.type_refs().filter(|(id, _)| {
                !expected.contains(&TypeOccurrenceId::new(sources[0].file(), *id))
            }) {
                assert!(matches!(
                    inputs.admitted_ticket(TypeOccurrenceId::new(sources[0].file(), id)),
                    Err(AnalysisSourceInputFailure::NotOriginalType(_))
                ));
            }
        },
    );
}

#[test]
fn use_expansion_and_cached_standalone_rhs_keep_distinct_originals() {
    with_resolver(
        &["type First = Pair[i32]\ntype Pair[T] = (T, T)\ntype Second = Pair[Bool]"],
        |resolver, sources| {
            let hir = sources[0].hir().unwrap();
            let file = sources[0].file();
            let first = TypeOccurrenceId::new(file, alias_rhs(hir, "First"));
            let pair = TypeOccurrenceId::new(file, alias_rhs(hir, "Pair"));
            let second = TypeOccurrenceId::new(file, alias_rhs(hir, "Second"));
            let mut work = accounting(1);
            let mut inputs = AnalysisSourceInputs::new(&work, [(0, &sources[0])]).unwrap();
            let trace = TraceScope::new(None);
            let mut diagnostics = DiagnosticStore::new();
            completed(
                run(
                    resolver,
                    sources,
                    &mut diagnostics,
                    &mut work,
                    &mut inputs,
                    &|_| ALLOWANCE,
                )
                .unwrap(),
            );
            assert!(diagnostics.finalize(None).diagnostics().is_empty());
            let a = inputs.admitted_ticket(first).unwrap().unwrap().identity();
            let b = inputs.admitted_ticket(pair).unwrap().unwrap().identity();
            let c = inputs.admitted_ticket(second).unwrap().unwrap().identity();
            assert_ne!(a, b);
            assert_ne!(b, c);
            assert_ne!(a, c);
            let roots = trace
                .visits()
                .into_iter()
                .filter(|visit| visit.occurrence == pair)
                .map(|visit| visit.root)
                .collect::<Vec<_>>();
            assert_eq!(roots, [a, a, b, c]);
            assert_eq!(
                work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
                heads(hir).len()
            );
        },
    );
}

#[test]
fn differently_instantiated_alias_dags_do_not_admit_generated_roots() {
    let mut source = "type A0[T] = (T, T)\n".to_owned();
    for depth in 1..8 {
        source.push_str(&format!(
            "type A{depth}[T] = (A{}[T], A{}[T])\n",
            depth - 1,
            depth - 1
        ));
    }
    source.push_str("fn inspect(x: A7[i32], y: A7[Bool]) -> i32 = 0");
    with_resolver(&[&source], |resolver, sources| {
        let hir = sources[0].hir().unwrap();
        let mut work = accounting(1);
        let mut inputs = AnalysisSourceInputs::new(&work, [(0, &sources[0])]).unwrap();
        let calls = RefCell::new(Vec::new());
        let allowance = |site| {
            calls.borrow_mut().push(site);
            ALLOWANCE
        };
        let mut diagnostics = DiagnosticStore::new();
        completed(
            run(
                resolver,
                sources,
                &mut diagnostics,
                &mut work,
                &mut inputs,
                &allowance,
            )
            .unwrap(),
        );
        assert!(diagnostics.finalize(None).diagnostics().is_empty());
        assert_eq!(calls.borrow().len(), heads(hir).len());
        assert_eq!(
            work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
            heads(hir).len()
        );
    });
}

#[test]
fn replay_keeps_tickets_and_balances_without_reinvoking_allowance_policy() {
    with_resolver(
        &["type Pair[T] = (T, T)\nfn inspect(x: Pair[i32]) -> Pair[i32] = x"],
        |resolver, sources| {
            let hir = sources[0].hir().unwrap();
            let sites = heads(hir)
                .into_iter()
                .map(|id| TypeOccurrenceId::new(sources[0].file(), id))
                .collect::<Vec<_>>();
            let mut work = accounting(1);
            let mut inputs = AnalysisSourceInputs::new(&work, [(0, &sources[0])]).unwrap();
            let mut first_diagnostics = DiagnosticStore::new();
            let first = completed(
                run(
                    resolver,
                    sources,
                    &mut first_diagnostics,
                    &mut work,
                    &mut inputs,
                    &|_| ALLOWANCE,
                )
                .unwrap(),
            );
            let retained = sites
                .iter()
                .map(|site| {
                    let ticket = inputs.admitted_ticket(*site).unwrap().unwrap();
                    (ticket.clone(), work.balance(ticket).unwrap())
                })
                .collect::<Vec<_>>();
            let mut second_diagnostics = DiagnosticStore::new();
            let second = completed(
                run(
                    resolver,
                    sources,
                    &mut second_diagnostics,
                    &mut work,
                    &mut inputs,
                    &|_| panic!("replay invoked policy"),
                )
                .unwrap(),
            );
            assert_eq!(first.signatures().callables, second.signatures().callables);
            assert_eq!(
                first_diagnostics.finalize(None),
                second_diagnostics.finalize(None)
            );
            for (site, (ticket, balance)) in sites.iter().zip(retained) {
                assert_eq!(inputs.admitted_ticket(*site).unwrap().unwrap(), &ticket);
                let after = work.balance(&ticket).unwrap();
                assert_eq!(after.allowance_ticks(), balance.allowance_ticks());
                assert!(after.charged_ticks() > balance.charged_ticks());
            }
            assert_eq!(
                work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
                sites.len()
            );
        },
    );
}

#[test]
fn zero_allowance_stops_before_source_site_or_occurrence_publication() {
    with_resolver(
        &["type Input = Fn((i32, Bool)) -> Opt[i32]"],
        |resolver, sources| {
            let site = TypeOccurrenceId::new(
                sources[0].file(),
                alias_rhs(sources[0].hir().unwrap(), "Input"),
            );
            let mut work = accounting(1);
            let mut inputs = AnalysisSourceInputs::new(&work, [(0, &sources[0])]).unwrap();
            let trace = TraceScope::new(None);
            let mut diagnostics = DiagnosticStore::new();
            let SignatureExecution::Interrupted(result) = run(
                resolver,
                sources,
                &mut diagnostics,
                &mut work,
                &mut inputs,
                &|_| 0,
            )
            .unwrap() else {
                panic!("expected interruption")
            };
            assert_eq!(Some(result.stop()), work.incomplete());
            assert!(inputs.admitted_ticket(site).unwrap().is_none());
            assert!(trace.visits().is_empty());
            assert!(matches!(
                result.prefix().unwrap().occurrence(site),
                FactState::Unperformed
            ));
            assert!(diagnostics.finalize(None).diagnostics().is_empty());
        },
    );
}

#[test]
fn finite_visit_sweep_preserves_actual_demand_custody_and_diagnostic_prefixes() {
    for text in [
        "type Bad = Missing\ntype First = Pair[Bool]\ntype Pair[T] = (T, T)\nfn inspect(x: &(i32, Fn(i32) -> Bool)) -> Opt[i32] = None",
        "trait Has[A] { type Item }\ntrait Uses[A] {}\ntype Erase[A] = i64\nfn inspect[T:Uses[U::Item],U:Has[Erase[U::Item]]]() -> Unit = ()",
        "trait Has[A] { type Item }\nfn inspect[T:Has[(T::Item,Missing)]]() -> Unit = ()",
    ] {
        with_resolver(&[text], |resolver, sources| {
            let trace = TraceScope::new(None);
            let mut work = accounting(1);
            let mut inputs = AnalysisSourceInputs::new(&work, [(0, &sources[0])]).unwrap();
            let mut diagnostics = DiagnosticStore::new();
            completed(
                run(
                    resolver,
                    sources,
                    &mut diagnostics,
                    &mut work,
                    &mut inputs,
                    &|_| ALLOWANCE,
                )
                .unwrap(),
            );
            let full = diagnostics.finalize(None);
            let visits = trace.visits();
            assert!(!visits.is_empty());
            drop(trace);
            for ordinal in 1..=visits.len() {
                let trace = TraceScope::new(Some(ordinal));
                let mut work = accounting(1);
                let mut inputs = AnalysisSourceInputs::new(&work, [(0, &sources[0])]).unwrap();
                let mut diagnostics = DiagnosticStore::new();
                let SignatureExecution::Interrupted(result) = run(
                    resolver,
                    sources,
                    &mut diagnostics,
                    &mut work,
                    &mut inputs,
                    &|_| ALLOWANCE,
                )
                .unwrap() else {
                    panic!("false completion")
                };
                assert_eq!(Some(result.stop()), work.incomplete());
                assert_eq!(result.stop().root(), visits[ordinal - 1].root);
                assert_eq!(trace.visits().len(), ordinal);
                assert!(result.internal_failures().is_empty());
                for diagnostic in diagnostics.finalize(None).diagnostics() {
                    assert!(full.diagnostics().contains(diagnostic));
                }
            }
        });
    }
}

#[test]
fn original_bound_construction_and_validation_reuse_the_written_ticket() {
    with_resolver(
        &["trait Tr {}\nfn inspect[T: Tr](x: T) -> T = x"],
        |resolver, sources| {
            let hir = sources[0].hir().unwrap();
            let bound = hir
                .items()
                .find_map(|(_, item)| match item.kind() {
                    ItemKind::Function(value) => value
                        .generic_parameters
                        .first()
                        .and_then(|parameter| parameter.bounds.first().copied()),
                    _ => None,
                })
                .unwrap();
            let site = TypeOccurrenceId::new(sources[0].file(), bound);
            let trace = TraceScope::new(None);
            let mut work = accounting(1);
            let mut inputs = AnalysisSourceInputs::new(&work, [(0, &sources[0])]).unwrap();
            let calls = RefCell::new(Vec::new());
            let policy = |site| {
                calls.borrow_mut().push(site);
                ALLOWANCE
            };
            let mut diagnostics = DiagnosticStore::new();
            completed(
                run(
                    resolver,
                    sources,
                    &mut diagnostics,
                    &mut work,
                    &mut inputs,
                    &policy,
                )
                .unwrap(),
            );
            assert_eq!(
                calls
                    .borrow()
                    .iter()
                    .filter(|found| **found == site)
                    .count(),
                1
            );
            let ticket = inputs.admitted_ticket(site).unwrap().unwrap();
            let charged = work.balance(ticket).unwrap().charged_ticks();
            let visits = trace
                .visits()
                .into_iter()
                .filter(|visit| visit.occurrence == site)
                .collect::<Vec<_>>();
            assert_eq!(visits.len(), 2);
            assert!(visits.iter().all(|visit| visit.root == ticket.identity()));
            assert!(trace.claims(ticket.identity()) >= visits.len());
            drop(trace);
            let mut standalone_work = accounting(1);
            let mut standalone = crate::AnalysisSourceInputRegistry::new(
                &standalone_work,
                0,
                sources[0].file(),
                hir,
            )
            .unwrap();
            let standalone_ticket = standalone
                .written_ticket(&mut standalone_work, site, ALLOWANCE)
                .unwrap();
            standalone_work.claim(&standalone_ticket).unwrap();
            let expected = standalone_work
                .balance(&standalone_ticket)
                .unwrap()
                .charged_ticks();
            assert!(charged > expected);
            assert!(diagnostics.finalize(None).diagnostics().is_empty());

            let mut limited = accounting(1);
            let mut limited_inputs =
                AnalysisSourceInputs::new(&limited, [(0, &sources[0])]).unwrap();
            let mut diagnostics = DiagnosticStore::new();
            let policy = |input| {
                if input == site {
                    charged - 1
                } else {
                    ALLOWANCE
                }
            };
            let SignatureExecution::Interrupted(result) = run(
                resolver,
                sources,
                &mut diagnostics,
                &mut limited,
                &mut limited_inputs,
                &policy,
            )
            .unwrap() else {
                panic!("cached original request did not pay retrieval")
            };
            assert_eq!(
                result.stop().root(),
                limited_inputs
                    .admitted_ticket(site)
                    .unwrap()
                    .unwrap()
                    .identity()
            );
            assert!(result.internal_failures().is_empty());
            assert!(!diagnostics.has_errors());
        },
    );
}

#[test]
fn an_exactly_spent_original_cannot_be_renewed_by_a_later_phase() {
    with_resolver(&["type Input = i32"], |resolver, sources| {
        let site = TypeOccurrenceId::new(
            sources[0].file(),
            alias_rhs(sources[0].hir().unwrap(), "Input"),
        );
        let mut probe = accounting(1);
        let mut raw = crate::AnalysisSourceInputRegistry::new(
            &probe,
            0,
            sources[0].file(),
            sources[0].hir().unwrap(),
        )
        .unwrap();
        let ticket = raw.written_ticket(&mut probe, site, ALLOWANCE).unwrap();
        for _ in 0..3 {
            probe.claim(&ticket).unwrap();
        }
        let exact = probe.balance(&ticket).unwrap().charged_ticks();

        let mut work = accounting(1);
        let mut inputs = AnalysisSourceInputs::new(&work, [(0, &sources[0])]).unwrap();
        let mut diagnostics = DiagnosticStore::new();
        completed(
            run(
                resolver,
                sources,
                &mut diagnostics,
                &mut work,
                &mut inputs,
                &|_| exact,
            )
            .unwrap(),
        );
        let original = inputs.admitted_ticket(site).unwrap().unwrap().clone();
        assert_eq!(work.balance(&original).unwrap().remaining_ticks(), 0);
        let SignatureExecution::Interrupted(result) = run(
            resolver,
            sources,
            &mut diagnostics,
            &mut work,
            &mut inputs,
            &|_| panic!("replay tried to renew allowance"),
        )
        .unwrap() else {
            panic!("exhausted replay completed")
        };
        assert_eq!(result.stop().root(), original.identity());
        assert_eq!(inputs.admitted_ticket(site).unwrap().unwrap(), &original);
        assert_eq!(work.balance(&original).unwrap().allowance_ticks(), exact);
        assert!(!diagnostics.has_errors());
    });
}

#[test]
fn alias_cycles_and_dynamic_trait_source_errors_keep_their_original_diagnostics() {
    for text in [
        "type A = B\ntype B = A",
        "trait Tr {}\nfn inspect(x: dyn Tr) -> i32 = 0",
    ] {
        with_resolver(&[text], |resolver, sources| {
            let mut diagnostics = DiagnosticStore::new();
            let original = resolve_declaration_signatures(
                resolver.collection,
                resolver.imports,
                sources,
                &Support,
                SignatureResolutionInputs::new(resolver.external_names, None),
                &mut diagnostics,
            )
            .unwrap();
            let expected = diagnostics.finalize(None);
            assert!(!expected.diagnostics().is_empty());
            assert!(matches!(
                original.into_parts().0,
                PhaseProduct::Performed(_)
            ));
            let mut work = accounting(1);
            let mut inputs = AnalysisSourceInputs::new(&work, [(0, &sources[0])]).unwrap();
            let mut diagnostics = DiagnosticStore::new();
            completed(
                run(
                    resolver,
                    sources,
                    &mut diagnostics,
                    &mut work,
                    &mut inputs,
                    &|_| ALLOWANCE,
                )
                .unwrap(),
            );
            assert_eq!(diagnostics.finalize(None), expected);
            assert!(work.incomplete().is_none());
            assert_eq!(
                work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
                heads(sources[0].hir().unwrap()).len()
            );
        });
    }
}

#[test]
fn inherited_alias_ticket_copy_pays_the_existing_registry_contract() {
    with_resolver(
        &["type First = Pair[i32]\ntype Pair[T] = (T, T)"],
        |resolver, sources| {
            let hir = sources[0].hir().unwrap();
            let site = TypeOccurrenceId::new(sources[0].file(), alias_rhs(hir, "First"));
            let mut work = accounting(1);
            let mut inputs = AnalysisSourceInputs::new(&work, [(0, &sources[0])]).unwrap();
            let trace = TraceScope::new(None);
            let mut diagnostics = DiagnosticStore::new();
            completed(
                run(
                    resolver,
                    sources,
                    &mut diagnostics,
                    &mut work,
                    &mut inputs,
                    &|_| ALLOWANCE,
                )
                .unwrap(),
            );
            let ticket = inputs.admitted_ticket(site).unwrap().unwrap();
            let actual = work.balance(ticket).unwrap().charged_ticks();

            let mut model = accounting(1);
            let mut raw =
                crate::AnalysisSourceInputRegistry::new(&model, 0, sources[0].file(), hir).unwrap();
            let original = raw.written_ticket(&mut model, site, ALLOWANCE).unwrap();
            for _ in 0..trace.claims(ticket.identity()) {
                model.claim(&original).unwrap();
            }
            raw.inherited_ticket(&mut model, &original).unwrap();
            assert_eq!(actual, model.balance(&original).unwrap().charged_ticks());
            assert!(!diagnostics.has_errors());
        },
    );
}
