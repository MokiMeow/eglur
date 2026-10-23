use super::*;
use std::cell::RefCell;

use super::super::tests::with_resolver;
use crate::{
    AnalysisRootTicket, AnalysisSourceInputs, AnalysisWork, AnalysisWorkAccounting,
    AnalysisWorkPricing,
};

struct Support;

impl CompileSupport for Support {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

fn assert_same_fact<T: PartialEq + std::fmt::Debug>(
    left: FactState<'_, T>,
    right: FactState<'_, T>,
) {
    match (left, right) {
        (FactState::Known(left), FactState::Known(right)) => assert_eq!(left, right),
        (FactState::Poisoned(_), FactState::Poisoned(_)) => {}
        (FactState::Unperformed, FactState::Unperformed)
        | (FactState::NotApplicable, FactState::NotApplicable) => {}
        (left, right) => panic!("fact state changed: {left:?} / {right:?}"),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Fault {
    Exhaustion,
    Overflow,
    Marker,
    Internal,
    InternalWithStop,
}

struct Probe {
    ticket: AnalysisRootTicket,
    events: Vec<Checkpoint>,
    at: Option<usize>,
    only: Option<Checkpoint>,
    fault: Fault,
}

thread_local! {
    static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) };
}

struct ProbeScope;

impl ProbeScope {
    fn new(
        ticket: &AnalysisRootTicket,
        at: Option<usize>,
        only: Option<Checkpoint>,
        fault: Fault,
    ) -> Self {
        PROBE.with_borrow_mut(|probe| {
            assert!(probe.is_none());
            *probe = Some(Probe {
                ticket: ticket.clone(),
                events: Vec::new(),
                at,
                only,
                fault,
            });
        });
        Self
    }

    fn events(&self) -> Vec<Checkpoint> {
        PROBE.with_borrow(|probe| probe.as_ref().unwrap().events.clone())
    }
}

impl Drop for ProbeScope {
    fn drop(&mut self) {
        PROBE.with_borrow_mut(|probe| *probe = None);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::signatures::resolver) enum Checkpoint {
    Type(TypeOccurrenceId),
    Trait(TypeOccurrenceId),
    Alias(DefId),
}

pub(in crate::signatures::resolver) fn checkpoint(
    resolver: &mut SignatureResolver<'_, '_>,
    point: Checkpoint,
) -> Result<(), SignatureFlow> {
    let request = PROBE.with_borrow_mut(|probe| {
        let probe = probe.as_mut()?;
        probe.events.push(point);
        let stop = probe.at == Some(probe.events.len()) || probe.only == Some(point);
        Some((probe.ticket.clone(), stop, probe.fault))
    });
    let Some((ticket, stop, fault)) = request else {
        return resolver.ensure_analysis_active();
    };
    if stop && fault == Fault::Marker {
        return Err(SignatureFlow::Interrupted);
    }
    if stop && fault == Fault::Internal {
        return Err(SignatureResolutionError::MissingSource(FileId::new(99)).into());
    }

    let work = resolver
        .analysis_work
        .as_mut()
        .expect("checkpoint requires accounting");
    let mut scope = work.accounting.claim(&ticket)?;
    if stop {
        let estimate = AnalysisWork {
            steps: if fault == Fault::Overflow {
                u64::MAX
            } else {
                1_000_000
            },
            inspected_bytes: 0,
            transferred_bytes: 0,
            reserved_bytes: 0,
        };
        let failure = scope.precharge(estimate).unwrap_err();
        if fault == Fault::InternalWithStop {
            return Err(SignatureResolutionError::MissingSource(FileId::new(99)).into());
        }
        return Err(failure.into());
    }
    scope.precharge(AnalysisWork {
        steps: 1,
        inspected_bytes: 0,
        transferred_bytes: 0,
        reserved_bytes: 0,
    })?;
    resolver.ensure_analysis_active()
}

fn accounting() -> (AnalysisWorkAccounting, AnalysisRootTicket) {
    let mut work =
        AnalysisWorkAccounting::new(AnalysisWorkPricing::for_test(1, 1, 1).unwrap(), 1).unwrap();
    let ticket = work.admit(0, 100_000).unwrap();
    (work, ticket)
}

fn interrupted(outcome: SignatureExecution) -> Box<InterruptedSignatures> {
    let SignatureExecution::Interrupted(value) = outcome else {
        panic!("expected interruption")
    };
    value
}

fn completed(outcome: SignatureExecution) -> PhaseAnalysis<ResolvedDeclarationSemantics> {
    let SignatureExecution::Completed(value) = outcome else {
        panic!("expected completion")
    };
    *value
}

fn owner(resolver: &SignatureResolver<'_, '_>, name: &str) -> DefId {
    resolver
        .collection
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| {
            declaration
                .name()
                .is_some_and(|candidate| candidate.text() == name)
        })
        .unwrap()
        .id()
}

fn run_public(
    resolver: &SignatureResolver<'_, '_>,
    sources: &[SemanticSource<'_>],
    diagnostics: &mut DiagnosticStore,
    work: &mut AnalysisWorkAccounting,
) -> Result<SignatureExecution, SignatureResolutionError> {
    let mut source_inputs =
        AnalysisSourceInputs::new(work, sources.iter().map(|source| (0, source))).unwrap();
    resolve_declaration_signatures_with_work(
        resolver.collection,
        resolver.imports,
        sources,
        &Support,
        SignatureResolutionInputs::new(resolver.external_names, None),
        diagnostics,
        SignatureAnalysisWork::new(work, &mut source_inputs, &|_| 100_000),
    )
}

#[test]
fn active_public_execution_preserves_completed_facts_and_diagnostics_exactly() {
    with_resolver(
        &[
            "type Pair[T] = (T, T)\nstruct Box[T] { field value: T }\nfn inspect(x: Pair[i32], y: Fn(i32) -> Bool) -> Opt[i32] = None\ntype Bad = Missing",
        ],
        |resolver, sources| {
            let mut old_diagnostics = DiagnosticStore::new();
            let mut new_diagnostics = DiagnosticStore::new();
            let old = resolve_declaration_signatures(
                resolver.collection,
                resolver.imports,
                sources,
                &Support,
                SignatureResolutionInputs::new(resolver.external_names, None),
                &mut old_diagnostics,
            )
            .unwrap();
            let (mut work, ticket) = accounting();
            let before = work.balance(&ticket).unwrap();
            let new =
                completed(run_public(resolver, sources, &mut new_diagnostics, &mut work).unwrap());
            let (old, old_demands) = old.into_parts();
            let (new, new_demands) = new.into_parts();
            assert_eq!(old_demands, new_demands);
            let (PhaseProduct::Performed(old), PhaseProduct::Performed(new)) = (old, new) else {
                panic!("expected performed signatures")
            };
            let a = old.signatures();
            let b = new.signatures();
            assert_eq!(a.types().type_count(), b.types().type_count());
            for (id, _) in sources[0].hir().unwrap().type_refs() {
                assert_same_fact(
                    a.occurrence(TypeOccurrenceId::new(sources[0].file(), id)),
                    b.occurrence(TypeOccurrenceId::new(sources[0].file(), id)),
                );
            }
            for module in resolver.collection.modules() {
                for declaration in module.declarations() {
                    assert_same_fact(
                        a.source_declaration(declaration.id()),
                        b.source_declaration(declaration.id()),
                    );
                }
            }
            assert_eq!(a.callables, b.callables);
            assert_eq!(
                a.type_applications.applications,
                b.type_applications.applications
            );
            assert_eq!(
                old_diagnostics.finalize(None),
                new_diagnostics.finalize(None)
            );
            assert_eq!(work.balance(&ticket).unwrap(), before);
        },
    );
}

#[test]
fn preexisting_stop_precedes_source_preparation_and_capability_callbacks() {
    struct UnreachableSupport;
    impl CompileSupport for UnreachableSupport {
        fn supports(&self, _: CompileCapability) -> bool {
            panic!("no capability work after stop")
        }
    }
    with_resolver(&["type Value = i32"], |resolver, sources| {
        let (mut work, _) = accounting();
        let mut source_inputs =
            AnalysisSourceInputs::new(&work, sources.iter().map(|source| (0, source))).unwrap();
        let stop = match work.admit(0, 0).unwrap_err() {
            AnalysisWorkFailure::Incomplete(stop) => stop,
            other => panic!("{other:?}"),
        };
        let mut diagnostics = DiagnosticStore::new();
        let result = resolve_declaration_signatures_with_work(
            resolver.collection,
            resolver.imports,
            &[],
            &UnreachableSupport,
            SignatureResolutionInputs::new(resolver.external_names, None),
            &mut diagnostics,
            SignatureAnalysisWork::new(&mut work, &mut source_inputs, &|_| {
                panic!("no admission after stop")
            }),
        )
        .unwrap();
        let result = interrupted(result);
        assert_eq!(result.stop(), stop);
        assert!(result.prefix().is_none());
        assert!(result.demands().is_empty());
        assert!(result.internal_failures().is_empty());
        assert!(diagnostics.finalize(None).diagnostics().is_empty());
    });
}

#[test]
fn unsupported_capability_remains_completed_unperformed_not_interrupted() {
    struct Unsupported;
    impl CompileSupport for Unsupported {
        fn supports(&self, _: CompileCapability) -> bool {
            false
        }
    }
    with_resolver(&["type Value = i32"], |resolver, sources| {
        let (mut work, _) = accounting();
        let mut source_inputs =
            AnalysisSourceInputs::new(&work, sources.iter().map(|source| (0, source))).unwrap();
        let mut diagnostics = DiagnosticStore::new();
        let result = resolve_declaration_signatures_with_work(
            resolver.collection,
            resolver.imports,
            sources,
            &Unsupported,
            SignatureResolutionInputs::new(resolver.external_names, None),
            &mut diagnostics,
            SignatureAnalysisWork::new(&mut work, &mut source_inputs, &|_| {
                panic!("no admission for unsupported capability")
            }),
        )
        .unwrap();
        let (product, demands) = completed(result).into_parts();
        assert!(matches!(product, PhaseProduct::Unperformed));
        assert!(!demands.is_empty());
        assert!(work.incomplete().is_none());
    });
}

#[test]
fn nested_stop_preserves_earlier_callable_type_names_and_child_facts_only() {
    with_resolver(
        &[
            "fn first(x: i32) -> i32 = x\ntype Pair[T] = (T, T)\nfn later(x: Pair[(i32, Bool)]) -> i32 = 0",
        ],
        |resolver, sources| {
            let first = owner(resolver, "first");
            let later = owner(resolver, "later");
            let hir = sources[0].hir().unwrap();
            let bool_ref = hir
                .type_refs()
                .find_map(|(id, ty)| match ty.kind() {
                    TypeRefKind::Path { path, .. }
                        if path.segments().last().unwrap().text() == Some("Bool") =>
                    {
                        Some(id)
                    }
                    _ => None,
                })
                .unwrap();
            let (mut work, ticket) = accounting();
            let probe = ProbeScope::new(
                &ticket,
                None,
                Some(Checkpoint::Type(TypeOccurrenceId::new(
                    sources[0].file(),
                    bool_ref,
                ))),
                Fault::Exhaustion,
            );
            let mut diagnostics = DiagnosticStore::new();
            let result =
                interrupted(run_public(resolver, sources, &mut diagnostics, &mut work).unwrap());
            assert_eq!(work.incomplete(), Some(result.stop()));
            let prefix = result.prefix().unwrap();
            let FactState::Known(DeclarationSignature::Callable(id)) =
                prefix.declaration(&CanonicalTarget::Source(first))
            else {
                panic!("earlier callable missing")
            };
            let callable = prefix.callable(*id).unwrap();
            for parameter in callable.parameters() {
                prefix.types().kind(parameter.ty()).unwrap();
            }
            prefix.types().kind(callable.result()).unwrap();
            assert_eq!(
                prefix.declared_name(&CanonicalTarget::Source(first)),
                Some("first")
            );
            assert!(matches!(
                prefix.declaration(&CanonicalTarget::Source(later)),
                FactState::Unperformed
            ));
            assert!(matches!(
                prefix.occurrence(TypeOccurrenceId::new(sources[0].file(), bool_ref)),
                FactState::Unperformed
            ));
            assert!(prefix.fact_count() > 0);
            assert!(prefix.inhabitance().is_none());
            assert!(!result.demands().is_empty());
            assert!(result.internal_failures().is_empty());
            assert!(!probe.events().is_empty());
        },
    );
}

#[test]
fn trait_and_alias_entries_propagate_the_same_accounting_stop() {
    for text in [
        "trait Tr[A] {}\nfn inspect[T: Tr[i32]](x: T) -> T = x",
        "type Base = i32\ntype Input = (Base, Bool)",
    ] {
        with_resolver(&[text], |resolver, sources| {
            let (mut first_work, first_ticket) = accounting();
            let record = ProbeScope::new(&first_ticket, None, None, Fault::Exhaustion);
            let mut first_diagnostics = DiagnosticStore::new();
            completed(
                run_public(resolver, sources, &mut first_diagnostics, &mut first_work).unwrap(),
            );
            let point = record
                .events()
                .into_iter()
                .find(|point| {
                    if text.starts_with("trait") {
                        matches!(point, Checkpoint::Trait(_))
                    } else {
                        matches!(point, Checkpoint::Alias(_))
                    }
                })
                .unwrap();
            drop(record);
            let (mut work, ticket) = accounting();
            let _probe = ProbeScope::new(&ticket, None, Some(point), Fault::Exhaustion);
            let mut diagnostics = DiagnosticStore::new();
            let result =
                interrupted(run_public(resolver, sources, &mut diagnostics, &mut work).unwrap());
            assert_eq!(work.incomplete(), Some(result.stop()));
            assert!(result.prefix().is_some());
            assert!(result.internal_failures().is_empty());
        });
    }
}

#[test]
fn internal_faults_without_a_stop_remain_internal_errors() {
    with_resolver(&["type Input = i32"], |resolver, sources| {
        let (mut work, ticket) = accounting();
        let _probe = ProbeScope::new(&ticket, Some(1), None, Fault::Internal);
        let mut diagnostics = DiagnosticStore::new();
        assert!(
            matches!(run_public(resolver, sources, &mut diagnostics, &mut work), Err(SignatureResolutionError::MissingSource(file)) if file == FileId::new(99))
        );
        assert!(work.incomplete().is_none());
        assert!(diagnostics.finalize(None).diagnostics().is_empty());
    });
}

#[test]
fn sticky_stop_wins_over_an_internal_fault_and_retains_it_separately() {
    with_resolver(&["type Input = i32"], |resolver, sources| {
        let (mut work, ticket) = accounting();
        let _probe = ProbeScope::new(&ticket, Some(1), None, Fault::InternalWithStop);
        let mut diagnostics = DiagnosticStore::new();
        let result =
            interrupted(run_public(resolver, sources, &mut diagnostics, &mut work).unwrap());
        assert_eq!(
            result.internal_failures(),
            [SignatureResolutionError::MissingSource(FileId::new(99))]
        );
        assert_eq!(work.incomplete(), Some(result.stop()));
        assert!(diagnostics.finalize(None).diagnostics().is_empty());
    });
}

#[test]
fn stop_marker_without_a_cause_is_an_invariant_error_not_incomplete() {
    with_resolver(&["type Input = i32"], |resolver, sources| {
        let (mut work, ticket) = accounting();
        let _probe = ProbeScope::new(&ticket, Some(1), None, Fault::Marker);
        let mut diagnostics = DiagnosticStore::new();
        assert!(matches!(
            run_public(resolver, sources, &mut diagnostics, &mut work),
            Err(SignatureResolutionError::MissingAnalysisStop)
        ));
        assert!(work.incomplete().is_none());
    });
}

#[test]
fn unrepresentable_work_is_interrupted_not_a_source_error() {
    with_resolver(&["type Input = i32"], |resolver, sources| {
        let (mut work, ticket) = accounting();
        let _probe = ProbeScope::new(&ticket, Some(1), None, Fault::Overflow);
        let mut diagnostics = DiagnosticStore::new();
        let result =
            interrupted(run_public(resolver, sources, &mut diagnostics, &mut work).unwrap());
        assert!(matches!(
            result.stop().reason(),
            crate::AnalysisRootStopReason::Unrepresentable(_)
        ));
        assert!(diagnostics.finalize(None).diagnostics().is_empty());
    });
}

#[test]
fn foreign_checkpoint_custody_is_internal_and_does_not_stop_analysis() {
    with_resolver(&["type Input = i32"], |resolver, sources| {
        let (mut work, _) = accounting();
        let (_, foreign) = accounting();
        let _probe = ProbeScope::new(&foreign, None, None, Fault::Exhaustion);
        let mut diagnostics = DiagnosticStore::new();
        assert!(matches!(
            run_public(resolver, sources, &mut diagnostics, &mut work),
            Err(SignatureResolutionError::AnalysisRootClaim(
                crate::AnalysisRootClaimError::ForeignAttempt
            ))
        ));
        assert!(work.incomplete().is_none());
    });
}

#[test]
fn every_checkpoint_stop_refuses_completion_and_preserves_a_diagnostic_subset() {
    for text in [
        "type MissingFirst = Missing\ntrait Tr[A] {}\nstruct Host {}\nimpl Tr[i32] for Host {}\ntype Pair[T] = (T, T)\nfn inspect(x: Pair[(i32, Bool)]) -> Pair[i32] = x",
        "trait Has[A] { type Item }\ntrait Uses[A] {}\ntype Erase[A] = i64\nfn inspect[T:Uses[U::Item],U:Has[Erase[U::Item]]]() -> Unit = ()",
        "trait Has[A] { type Item }\nfn inspect[T:Has[(T::Item,Missing)]]() -> Unit = ()",
    ] {
        with_resolver(&[text], |resolver, sources| {
            let (mut full_work, ticket) = accounting();
            let probe = ProbeScope::new(&ticket, None, None, Fault::Exhaustion);
            let mut full_diagnostics = DiagnosticStore::new();
            completed(
                run_public(resolver, sources, &mut full_diagnostics, &mut full_work).unwrap(),
            );
            let count = probe.events().len();
            assert!(count > 1);
            drop(probe);
            let full = full_diagnostics.finalize(None);
            for ordinal in 1..=count {
                let (mut work, ticket) = accounting();
                let probe = ProbeScope::new(&ticket, Some(ordinal), None, Fault::Exhaustion);
                let mut diagnostics = DiagnosticStore::new();
                let result = interrupted(
                    run_public(resolver, sources, &mut diagnostics, &mut work).unwrap(),
                );
                assert_eq!(probe.events().len(), ordinal);
                assert_eq!(work.incomplete(), Some(result.stop()));
                for diagnostic in diagnostics.finalize(None).diagnostics() {
                    assert!(
                        full.diagnostics().contains(diagnostic),
                        "new downstream diagnosis after stop {ordinal}"
                    );
                }
            }
        });
    }
}

#[test]
fn retained_poison_still_belongs_to_the_callers_diagnostic_store() {
    with_resolver(
        &["type Bad = Missing\ntype Next = (i32, Bool)"],
        |resolver, sources| {
            let bad = owner(resolver, "Bad");
            let next = owner(resolver, "Next");
            let (mut work, ticket) = accounting();
            let _probe = ProbeScope::new(
                &ticket,
                None,
                Some(Checkpoint::Alias(next)),
                Fault::Exhaustion,
            );
            let mut diagnostics = DiagnosticStore::new();
            let result =
                interrupted(run_public(resolver, sources, &mut diagnostics, &mut work).unwrap());
            let FactState::Poisoned(poison) = result
                .prefix()
                .unwrap()
                .declaration(&CanonicalTarget::Source(bad))
            else {
                panic!("earlier source poison missing")
            };
            diagnostics
                .record_failure(eglur_diagnostics::DiagnosticFailure::Derivative(
                    poison.clone(),
                ))
                .unwrap();
            let snapshot = diagnostics.finalize(None);
            assert_eq!(snapshot.diagnostics().len(), 1);
            assert_eq!(snapshot.diagnostics()[0].code().as_str(), "NAME_NOT_FOUND");
        },
    );
}

#[test]
fn prefix_declaration_queries_preserve_overload_poison_masking() {
    with_resolver(
        &[
            "fn duplicate(x: i32) -> i32 = x\nfn duplicate(x: Bool) -> Bool = x\nfn tail() -> Unit {\nlet value: Opt[i32] = None\nyield ()\n}",
        ],
        |resolver, sources| {
            let module = resolver.collection.modules().next().unwrap();
            let duplicates = module
                .declarations()
                .filter(|declaration| {
                    declaration
                        .name()
                        .is_some_and(|name| name.text() == "duplicate")
                })
                .map(Declaration::id)
                .collect::<Vec<_>>();
            let annotation = sources[0]
                .hir()
                .unwrap()
                .bodies()
                .flat_map(|(_, body)| body.statements())
                .find_map(|(_, statement)| match statement.kind() {
                    StmtKind::Let { ty, .. } => *ty,
                    _ => None,
                })
                .unwrap();
            let (mut work, ticket) = accounting();
            let _probe = ProbeScope::new(
                &ticket,
                None,
                Some(Checkpoint::Type(TypeOccurrenceId::new(
                    sources[0].file(),
                    annotation,
                ))),
                Fault::Exhaustion,
            );
            let mut diagnostics = DiagnosticStore::new();
            let result =
                interrupted(run_public(resolver, sources, &mut diagnostics, &mut work).unwrap());
            let prefix = result.prefix().unwrap();
            for id in duplicates {
                assert!(matches!(
                    prefix.declarations.state(&CanonicalTarget::Source(id)),
                    FactState::Known(_)
                ));
                assert!(matches!(
                    prefix.declaration(&CanonicalTarget::Source(id)),
                    FactState::Poisoned(_)
                ));
            }
            assert!(
                diagnostics
                    .finalize(None)
                    .diagnostics()
                    .iter()
                    .any(|diagnostic| diagnostic.code().as_str() == "OVERLOAD_MARKER_REQUIRED")
            );
        },
    );
}

#[test]
fn recovered_source_inputs_keep_the_same_unmetered_source_disposition() {
    use eglur_diagnostics::{
        BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase,
        DiagnosticRoot, PrimarySpan, Severity, SourceLabel,
    };
    use eglur_source::{LineIndex, SourceFile};

    let file = FileId::new(0);
    let raw = SourceFile::new(
        file,
        b"type Broken = Fn(i32) MissingArrow\ntype Good = i32".to_vec(),
    );
    let text = raw.validate().unwrap();
    let parsed = eglur_syntax::parse_validated(text);
    let hir = eglur_hir::lower(&parsed);
    assert!(!hir.invalid_occurrences().is_empty());
    let lines = LineIndex::new(text);
    let label = SourceLabel::new("main.eglur").unwrap();
    let spans = hir
        .origins()
        .map(|(origin, value)| {
            (
                origin,
                SourceSpan::from_origin(label.clone(), value, &lines).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    let analyze = |mut work: Option<&mut AnalysisWorkAccounting>| {
        let mut diagnostics = DiagnosticStore::new();
        let invalid = hir
            .invalid_occurrences()
            .iter()
            .map(|invalid| {
                let diagnostic = Diagnostic::new(
                    DiagnosticCode::new("PARSE_UNEXPECTED_TOKEN").unwrap(),
                    Severity::Error,
                    DiagnosticPhase::new("parse").unwrap(),
                    "recovered syntax",
                    PrimarySpan::Unavailable,
                );
                let root = DiagnosticRoot::boundary(
                    BoundaryRootKey::new(format!("recovery-{}", invalid.origin().get())).unwrap(),
                    diagnostic,
                )
                .unwrap();
                let poison = diagnostics
                    .record_failure(DiagnosticFailure::Root(root))
                    .unwrap();
                (invalid.origin(), poison)
            })
            .collect::<Vec<_>>();
        let sources = [SemanticSource::new(
            file,
            ModuleAddress::new([Box::<str>::from("main")]).unwrap(),
            Some(&hir),
            spans.clone(),
            invalid,
            None,
        )
        .unwrap()];
        let PhaseProduct::Performed(collection) = crate::collect_declarations(&sources, &Support)
            .unwrap()
            .into_parts()
            .0
        else {
            panic!("collection unavailable")
        };
        let PhaseProduct::Performed(plan) = crate::plan_imports(
            &collection,
            &sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap()
        .into_parts()
        .0
        else {
            panic!("plan unavailable")
        };
        let unit = crate::ImportAnalysisUnit::new([file]).unwrap();
        let PhaseProduct::Performed(imports) = crate::resolve_imports(
            &collection,
            &plan,
            &[unit],
            &sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap()
        .into_parts()
        .0
        else {
            panic!("imports unavailable")
        };
        let inputs = SignatureResolutionInputs::new(Some(&external), None);
        let result = match work.as_deref_mut() {
            Some(work) => {
                let mut source_inputs =
                    AnalysisSourceInputs::new(work, sources.iter().map(|source| (0, source)))
                        .unwrap();
                resolve_declaration_signatures_with_work(
                    &collection,
                    &imports,
                    &sources,
                    &Support,
                    inputs,
                    &mut diagnostics,
                    SignatureAnalysisWork::new(work, &mut source_inputs, &|_| 100_000),
                )
                .map(completed)
            }
            None => resolve_declaration_signatures(
                &collection,
                &imports,
                &sources,
                &Support,
                inputs,
                &mut diagnostics,
            ),
        };
        let shape = result.map(|phase| match phase.into_parts().0 {
            PhaseProduct::Unperformed => None,
            PhaseProduct::Performed(value) => Some((
                value.signatures().declaration_fact_count(),
                value.signatures().occurrence_fact_count(),
            )),
        });
        (shape, diagnostics.finalize(None))
    };
    let old = analyze(None);
    let (mut work, _) = accounting();
    let new = analyze(Some(&mut work));
    assert_eq!(old, new);
    assert!(old.0.is_ok());
    assert!(
        old.1
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code().as_str() == "PARSE_UNEXPECTED_TOKEN")
    );
    assert!(work.incomplete().is_none());
}

#[test]
fn terminal_stop_over_apparent_success_keeps_inhabitance_without_success_freeze() {
    with_resolver(&["fn inspect(x: i32) -> i32 = x"], |original, sources| {
        let (mut work, ticket) = accounting();
        let mut diagnostics = DiagnosticStore::new();
        let mut resolver = SignatureResolver::new(
            original.collection,
            original.imports,
            BTreeMap::from([(sources[0].file(), &sources[0])]),
            None,
            original.external_names,
            None,
            &mut diagnostics,
        )
        .unwrap();
        let mut source_inputs =
            AnalysisSourceInputs::new(&work, sources.iter().map(|source| (0, source))).unwrap();
        resolver.analysis_work = Some(SignatureAnalysisWork::new(
            &mut work,
            &mut source_inputs,
            &|_| 100_000,
        ));
        let facts = resolver.run_passes().unwrap();
        let count = resolver.callables.len();
        let stop = resolver
            .analysis_work
            .as_mut()
            .unwrap()
            .accounting
            .claim(&ticket)
            .unwrap()
            .precharge(AnalysisWork {
                steps: 1_000_000,
                inspected_bytes: 0,
                transferred_bytes: 0,
                reserved_bytes: 0,
            })
            .unwrap_err();
        assert!(matches!(stop, AnalysisWorkFailure::Incomplete(_)));
        let result = interrupted(
            resolver
                .complete_execution(Ok(facts), DemandRecorder::new(&Support))
                .unwrap(),
        );
        let prefix = result.prefix().unwrap();
        assert!(prefix.inhabitance().is_some());
        assert_eq!(prefix.callables.len(), count);
        assert!(prefix.callables.capacity() >= count);
    });
}
