use super::facts::Snapshot;
use super::sweep_support::{self, Event, Trace};
use super::*;
use crate::analysis_work::{ChargeEvent, ChargeKind};
use crate::body_types::checker::test_support::{
    BodyPhaseInputs, analysis_inputs, fingerprints, with_phase_inputs,
};
use crate::{BodyAnalysisInput, CapabilityDemand, SemanticFingerprints};
use eglur_diagnostics::Diagnostic;

mod fixtures;

const ALLOWANCE: u64 = 10_000_000;

struct Support;

impl CompileSupport for Support {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

struct Run {
    trace: Trace,
    policy: Vec<BodyAnalysisInput>,
    snapshot: Snapshot,
    diagnostics: Vec<Diagnostic>,
    demands: Vec<CapabilityDemand>,
    fingerprints: Option<SemanticFingerprints>,
    stop: Option<crate::AnalysisWorkIncomplete>,
    internal_failures: Vec<BodyTypingError>,
}

#[derive(Clone, Copy)]
struct Cut {
    input: BodyAnalysisInput,
    allowance: u64,
}

fn run(text: &str, metered: bool, cut: Option<Cut>, corrupt_cleanup: bool, valid: bool) -> Run {
    let mut result = None;
    fixtures::with_inputs(text, |mut inputs, external| {
        let mut accounting =
            AnalysisWorkAccounting::new(AnalysisWorkPricing::for_test(7, 3, 5).unwrap(), 1)
                .unwrap();
        accounting.observe_charges(sweep_support::charge);
        let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
        let policy = RefCell::new(Vec::new());
        let allowance = |input| {
            policy.borrow_mut().push(input);
            cut.filter(|cut| cut.input == input)
                .map_or(ALLOWANCE, |cut| cut.allowance)
        };
        let (execution, trace) = sweep_support::capture(corrupt_cleanup, || {
            if metered {
                typecheck_bodies_with_work(
                    inputs.collection,
                    inputs.imports,
                    inputs.names,
                    Some(external),
                    inputs.sources,
                    &Support,
                    inputs.declarations,
                    inputs.trait_environment,
                    inputs.diagnostics,
                    BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &allowance),
                )
                .unwrap()
            } else {
                BodyExecution::Completed(Box::new(
                    typecheck_bodies(
                        inputs.collection,
                        inputs.imports,
                        inputs.names,
                        Some(external),
                        inputs.sources,
                        &Support,
                        inputs.declarations,
                        inputs.trait_environment,
                        inputs.diagnostics,
                    )
                    .unwrap(),
                ))
            }
        });

        let (snapshot, demands, fingerprint, stop, internal_failures) = match execution {
            BodyExecution::Completed(phase) => {
                assert!(cut.is_none(), "cut execution completed");
                assert_eq!(trace.finishes, 1);
                let (PhaseProduct::Performed(bodies), demands) = phase.into_parts() else {
                    panic!("completed execution did not perform")
                };
                let fingerprint = valid.then(|| fingerprints(&mut inputs, &bodies));
                (
                    Snapshot::body(&bodies),
                    demands.into_vec(),
                    fingerprint,
                    None,
                    Vec::new(),
                )
            }
            BodyExecution::Interrupted(stopped) => {
                assert!(cut.is_some(), "uncut execution interrupted");
                assert_eq!(trace.finishes, 0, "success finish after stop");
                assert_eq!(accounting.incomplete(), Some(stopped.stop()));
                inspect_prefix(&inputs, stopped.prefix(), &trace);
                let snapshot = Snapshot::prefix(stopped.prefix());
                assert_eq!(trace.terminal_facts.as_ref(), Some(&snapshot));
                (
                    snapshot,
                    stopped.demands().to_vec(),
                    None,
                    Some(stopped.stop()),
                    stopped.internal_failures().to_vec(),
                )
            }
        };
        assert_eq!(
            trace.terminal,
            Some(true),
            "lexical state leaked at terminal"
        );
        let diagnostics = std::mem::replace(inputs.diagnostics, DiagnosticStore::new())
            .finalize(None)
            .diagnostics()
            .to_vec();
        result = Some(Run {
            trace,
            policy: policy.into_inner(),
            snapshot,
            diagnostics,
            demands,
            fingerprints: fingerprint,
            stop,
            internal_failures,
        });
    });
    result.unwrap()
}

fn inspect_prefix(inputs: &BodyPhaseInputs<'_, '_>, prefix: &BodyTypingPrefix, trace: &Trace) {
    let later = inputs
        .collection
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| {
            declaration
                .name()
                .is_some_and(|name| name.text() == "later")
        })
        .unwrap()
        .id();
    assert!(matches!(prefix.body(later), FactState::Unperformed));
    for event in &trace.events {
        if let Event::PublishedBody(owner, fact) = event {
            assert_eq!(&format!("{:?}", prefix.body(*owner)), fact);
        }
    }

    let region_exists = |key| assert!(matches!(prefix.regions.state(key), FactState::Known(_)));
    for (_, fact) in prefix.regions.iter() {
        if let SemanticFact::Known(region) = fact
            && let Some(parent) = region.parent()
        {
            region_exists(parent);
        }
    }
    for (_, fact) in prefix.locals.iter() {
        if let SemanticFact::Known(local) = fact {
            region_exists(local.region());
        }
    }
    for transfer in &prefix.transfers {
        region_exists(transfer.target());
        for exited in transfer.exited_regions() {
            region_exists(exited);
        }
        if let ControlTransferSource::RegionCompletion(region) = transfer.source() {
            region_exists(region);
        }
    }

    let types = inputs.declarations.signatures().types();
    for (_, fact) in prefix.expressions.iter() {
        if let SemanticFact::Known(expression) = fact {
            assert!(types.kind(expression.ty).is_ok());
        }
    }
    for (_, fact) in prefix.callable_values.iter() {
        if let SemanticFact::Known(id) = fact {
            assert!(prefix.callable_instance(*id).is_some());
        }
    }
    for (_, fact) in prefix.invocations.iter() {
        if let SemanticFact::Known(invocation) = fact
            && let InvocationTarget::Callable(id) = invocation.target()
        {
            assert!(prefix.callable_instance(*id).is_some());
        }
    }
    for instance in &prefix.callable_instances {
        assert!(
            inputs
                .declarations
                .signatures()
                .callable(instance.signature())
                .is_some()
        );
        assert!(types.substitution(instance.substitution()).is_ok());
    }
}

fn charges(run: &Run) -> Vec<(usize, ChargeEvent)> {
    run.trace
        .events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            Event::Charge(charge) => Some((index, *charge)),
            _ => None,
        })
        .collect()
}

fn cut_before(complete: &Run, ordinal: usize) -> Cut {
    let charges = charges(complete);
    let root = charges[ordinal].1.root;
    let admission = charges
        .iter()
        .filter(|(_, charge)| charge.kind == ChargeKind::Admission)
        .position(|(_, charge)| charge.root == root)
        .expect("work had no original admission");
    Cut {
        input: complete.policy[admission],
        allowance: charges[..ordinal]
            .iter()
            .filter(|(_, charge)| charge.root == root)
            .map(|(_, charge)| charge.ticks)
            .sum(),
    }
}

fn assert_cut(complete: &Run, stopped: &Run, ordinal: usize) {
    let charges = charges(complete);
    let (event_index, charge) = charges[ordinal];
    assert!(charge.ticks > 0);
    assert_eq!(stopped.trace.events, complete.trace.events[..=event_index]);
    assert_eq!(
        stopped.stop.unwrap().reason(),
        AnalysisRootStopReason::Exhausted {
            required_ticks: charge.ticks,
            remaining_ticks: 0,
        }
    );
    assert_eq!(stopped.stop.unwrap().root(), charge.root);
    assert!(complete.policy.starts_with(&stopped.policy));
    stopped.snapshot.assert_retained_from(&complete.snapshot);
    for demand in &stopped.demands {
        assert!(complete.demands.contains(demand));
    }
    for diagnostic in &stopped.diagnostics {
        assert!(
            complete.diagnostics.contains(diagnostic),
            "fabricated source diagnostic {diagnostic:?}"
        );
    }
}

#[test]
fn every_integrated_charge_stops_without_continuation_or_fabricated_facts() {
    for (name, source, valid) in fixtures::sources() {
        let source = format!("{source}\nfn later() -> i32 = 0");
        let complete = run(&source, true, None, false, valid);
        let unmetered = run(&source, false, None, false, valid);
        assert_eq!(complete.snapshot, unmetered.snapshot, "{name}");
        assert_eq!(complete.diagnostics, unmetered.diagnostics, "{name}");
        assert_eq!(complete.demands, unmetered.demands, "{name}");
        assert_eq!(complete.fingerprints, unmetered.fingerprints, "{name}");
        assert_eq!(
            charges(&complete)
                .iter()
                .filter(|(_, charge)| charge.kind == ChargeKind::Admission)
                .count(),
            complete.policy.len(),
        );
        assert!(!charges(&complete).is_empty());
        assert!(complete.internal_failures.is_empty());
        assert_eq!(complete.diagnostics.is_empty(), valid, "{name}");
        eprintln!(
            "{name}: {} integrated charge cuts",
            charges(&complete).len()
        );
        for ordinal in 0..charges(&complete).len() {
            let stopped = run(
                &source,
                true,
                Some(cut_before(&complete, ordinal)),
                false,
                valid,
            );
            assert!(stopped.internal_failures.is_empty(), "{name}/{ordinal}");
            assert_cut(&complete, &stopped, ordinal);
        }
    }
}

#[test]
fn natural_stop_retains_cleanup_fault_as_supplemental_internal_evidence() {
    let source = "fn id[T](x: T) -> T = x\nfn inspect(x: i32) -> i32 { yield { yield id(x) } }\nfn later() -> i32 = 0";
    let complete = run(source, true, None, false, true);
    let mut witnessed = false;
    for ordinal in 0..charges(&complete).len() {
        let stopped = run(
            source,
            true,
            Some(cut_before(&complete, ordinal)),
            true,
            true,
        );
        assert_cut(&complete, &stopped, ordinal);
        if stopped.trace.corrupted {
            assert!(!stopped.internal_failures.is_empty());
            assert!(
                stopped
                    .internal_failures
                    .iter()
                    .all(|error| matches!(error, BodyTypingError::MissingControlRegion))
            );
            assert!(stopped.diagnostics.is_empty());
            witnessed = true;
        } else {
            assert!(stopped.internal_failures.is_empty());
        }
    }
    assert!(
        witnessed,
        "no real charge stopped inside an active lexical scope"
    );
}
