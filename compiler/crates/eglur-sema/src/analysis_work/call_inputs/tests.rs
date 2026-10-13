use std::cell::Cell;
use std::num::NonZeroU64;

use eglur_diagnostics::{DiagnosticStore, SourceLabel, SourceSpan};
use eglur_hir::{HirFile, ItemKind};
use eglur_source::{FileId, LineIndex, SourceFile};

use crate::{AnalysisRootClaimError, AnalysisRootStopReason, ModuleId};

use super::*;
use crate::analysis_work::AnalysisWorkPricing;

thread_local! {
    static HEADER_WRITES: Cell<usize> = const { Cell::new(0) };
    static SLOT_INSTALLS: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn record_header_write() {
    HEADER_WRITES.set(HEADER_WRITES.get() + 1);
}

pub(super) fn record_slot_install() {
    SLOT_INSTALLS.set(SLOT_INSTALLS.get() + 1);
}

fn pricing() -> AnalysisWorkPricing {
    AnalysisWorkPricing {
        ticks_per_step: NonZeroU64::new(7).unwrap(),
        ticks_per_processed_byte: NonZeroU64::new(2).unwrap(),
        ticks_per_reserved_byte: NonZeroU64::new(3).unwrap(),
    }
}

fn accounting() -> AnalysisWorkAccounting {
    AnalysisWorkAccounting::new(pricing(), 2).unwrap()
}

fn parse(text: &str) -> HirFile {
    let raw = SourceFile::new(FileId::new(0), text.as_bytes().to_vec());
    let parsed = eglur_syntax::parse_validated(raw.validate().unwrap());
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    let hir = eglur_hir::lower(&parsed);
    assert!(hir.invalid_occurrences().is_empty());
    hir
}

fn inspect_body(hir: &HirFile) -> &Body {
    let function = hir
        .items()
        .find_map(|(_, item)| match item.kind() {
            ItemKind::Function(function) if function.name.text() == Some("inspect") => {
                Some(function)
            }
            _ => None,
        })
        .unwrap();
    hir.body(function.body)
}

fn sites(body: &Body) -> Vec<ExpressionKey> {
    body.expressions()
        .filter(|(_, expression)| matches!(expression.kind(), ExprKind::Call(_)))
        .map(|(id, _)| ExpressionKey::new(ModuleId::from_index(0).unwrap(), id))
        .collect()
}

fn resolved_candidates(hir: &HirFile, text: &str) -> Vec<crate::CallableSignature> {
    struct Support;

    impl crate::CompileSupport for Support {
        fn supports(&self, _: crate::CompileCapability) -> bool {
            true
        }
    }

    fn performed<T>(phase: crate::PhaseAnalysis<T>) -> T {
        match phase.into_parts().0 {
            crate::PhaseProduct::Performed(value) => value,
            crate::PhaseProduct::Unperformed => panic!("source phase must be performed"),
        }
    }

    let raw = SourceFile::new(hir.file(), text.as_bytes().to_vec());
    let lines = LineIndex::new(raw.validate().unwrap());
    let label = SourceLabel::new("main.eglur").unwrap();
    let source = crate::SemanticSource::new(
        hir.file(),
        crate::ModuleAddress::new([Box::<str>::from("main")]).unwrap(),
        Some(hir),
        hir.origins().map(|(origin, value)| {
            (
                origin,
                SourceSpan::from_origin(label.clone(), value, &lines).unwrap(),
            )
        }),
        [],
        None,
    )
    .unwrap();
    let sources = [source];
    let external = crate::ExternalNameEnvironment::new([], [], []).unwrap();
    let mut diagnostics = DiagnosticStore::new();
    let collection = performed(crate::collect_declarations(&sources, &Support).unwrap());
    let plan = performed(
        crate::plan_imports(
            &collection,
            &sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let imports = performed(
        crate::resolve_imports(
            &collection,
            &plan,
            &[crate::ImportAnalysisUnit::new([hir.file()]).unwrap()],
            &sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let declarations = performed(
        crate::resolve_declaration_signatures(
            &collection,
            &imports,
            &sources,
            &Support,
            crate::SignatureResolutionInputs::new(Some(&external), None),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let (signatures, _) = declarations.into_parts();
    let snapshot = diagnostics.finalize(None);
    assert!(snapshot.diagnostics().is_empty(), "{snapshot:?}");

    collection
        .modules()
        .flat_map(|module| module.declarations())
        .filter_map(|declaration| {
            if !declaration.name().is_some_and(|name| name.text() == "pick") {
                return None;
            }
            let crate::FactState::Known(crate::DeclarationSignature::Callable(id)) =
                signatures.declaration(&crate::CanonicalTarget::Source(declaration.id()))
            else {
                panic!(
                    "source callable signature expected: {:?}, {:?}",
                    declaration.kind(),
                    signatures.declaration(&crate::CanonicalTarget::Source(declaration.id()))
                )
            };
            Some(signatures.callable(*id).unwrap().clone())
        })
        .collect()
}

fn admit(
    accounting: &mut AnalysisWorkAccounting,
    body: &Body,
    site: ExpressionKey,
) -> AnalysisCallInputs {
    AnalysisCallInputs::admit(accounting, 0, site, body, 1_000_000, None).unwrap()
}

fn failure<T>(result: Result<T, AnalysisCallInputFailure>) -> AnalysisCallInputFailure {
    match result {
        Ok(_) => panic!("expected input/accounting refusal"),
        Err(failure) => failure,
    }
}

fn admission_ticks() -> u64 {
    let header = size_of::<super::super::AnalysisRootBalance>() as u64;
    7 + 3 * header * 2 + 4 * header * 3
}

#[test]
fn parsed_call_prepares_only_paid_written_headers_and_lazy_roots() {
    for count in [0, 1, 8, 32, 128] {
        let arguments = vec!["x"; count].join(", ");
        let hir = parse(&format!("fn inspect(x: i32) -> Unit = f({arguments})"));
        let body = inspect_body(&hir);
        let site = sites(body)[0];
        let mut accounting = accounting();
        HEADER_WRITES.set(0);
        SLOT_INSTALLS.set(0);
        let mut inputs = admit(&mut accounting, body, site);

        assert_eq!(inputs.site(), site);
        assert_eq!(inputs.written_argument_count(), count);
        assert_eq!(HEADER_WRITES.get(), count);
        assert_eq!(SLOT_INSTALLS.get(), 0);
        assert!(inputs.arguments.iter().all(Option::is_none));
        assert_eq!(inputs.selection_ticket().identity().requirement(), 0);
        let bytes = (count * size_of::<Option<AnalysisRootTicket>>()) as u64;
        let paid = accounting.balance(inputs.selection_ticket()).unwrap();
        assert_eq!(
            paid.charged_ticks(),
            admission_ticks()
                + 7
                + 2 * (bytes + size_of::<AnalysisCallInputs>() as u64)
                + 3 * bytes,
        );

        for index in 0..count {
            let ticket = inputs
                .argument_ticket(&mut accounting, index, 10_000)
                .unwrap();
            assert_eq!(ticket.identity().requirement(), index + 1);
            assert_eq!(
                accounting.balance(ticket).unwrap().charged_ticks(),
                admission_ticks() + 7 + 2 * size_of::<Option<AnalysisRootTicket>>() as u64,
            );
        }
        assert_eq!(SLOT_INSTALLS.get(), count);
        let next = accounting.admit(0, 10_000).unwrap();
        assert_eq!(next.identity().requirement(), count + 1);
    }
}

#[test]
fn source_named_slot_identity_survives_candidate_parameter_permutations() {
    let text = "overload fn pick(left: i32, right: Bool) -> Unit = ()\n\
         overload fn pick(right: Bool, left: f64) -> Unit = ()\n\
         fn inspect(x: i32, y: Bool) -> Unit = pick(right = y, left = x)";
    let hir = parse(text);
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let ExprKind::Call(call) = body.expression(site.expression()).unwrap().kind() else {
        panic!("call expected")
    };
    let mut accounting = accounting();
    let mut inputs = admit(&mut accounting, body, site);
    let mut by_name = Vec::new();
    for candidate in resolved_candidates(&hir, text) {
        let mut bound = Vec::new();
        for parameter in candidate.parameters() {
            let name = parameter.name().unwrap();
            let index = call
                .arguments
                .iter()
                .position(|argument| {
                    argument.name.as_ref().and_then(|name| name.text()) == Some(name)
                })
                .unwrap();
            let ticket = inputs
                .argument_ticket(&mut accounting, index, 10_000)
                .unwrap()
                .clone();
            bound.push((name.to_owned(), ticket));
        }
        by_name.push(bound);
    }
    assert_eq!(by_name.len(), 2);
    assert_ne!(by_name[0][0].0, by_name[1][0].0);
    for (name, ticket) in &by_name[0] {
        let other = &by_name[1]
            .iter()
            .find(|(other, _)| other == name)
            .unwrap()
            .1;
        assert_eq!(ticket, other);
    }
    assert_eq!(
        accounting
            .admit(0, 10_000)
            .unwrap()
            .identity()
            .requirement(),
        3
    );
}

#[test]
fn replay_inherits_the_balance_and_ignores_a_new_prospective_allowance() {
    let hir = parse("fn inspect(x: i32) -> Unit = f(x)");
    let body = inspect_body(&hir);
    let mut accounting = accounting();
    let mut inputs = admit(&mut accounting, body, sites(body)[0]);
    let ticket = inputs
        .argument_ticket(&mut accounting, 0, 10_000)
        .unwrap()
        .clone();
    accounting
        .claim(&ticket)
        .unwrap()
        .precharge(AnalysisWork {
            steps: 17,
            inspected_bytes: 11,
            transferred_bytes: 3,
            reserved_bytes: 5,
        })
        .unwrap();
    let before = accounting.balance(&ticket).unwrap();

    for pass in 1..=16 {
        assert_eq!(
            inputs
                .argument_ticket(&mut accounting, 0, u64::MAX)
                .unwrap(),
            &ticket
        );
        let after = accounting.balance(&ticket).unwrap();
        assert_eq!(after.allowance_ticks(), 10_000);
        assert_eq!(after.charged_ticks(), before.charged_ticks() + pass * 7);
    }
    assert_eq!(
        accounting
            .admit(0, 10_000)
            .unwrap()
            .identity()
            .requirement(),
        2
    );
}

#[test]
fn callee_result_uses_selection_and_annotation_uses_existing_custody() {
    let hir = parse("fn inspect(x: i32) -> Unit { let y: i32 = f(x, x) }");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let mut accounting = accounting();
    let expected = accounting.admit(1, 10_000).unwrap();
    let before = accounting.balance(&expected).unwrap();
    let mut inputs =
        AnalysisCallInputs::admit(&mut accounting, 0, site, body, 10_000, Some(&expected)).unwrap();

    assert_eq!(inputs.callee_result_ticket(), inputs.selection_ticket());
    assert_eq!(inputs.contextual_result_ticket(), Some(&expected));
    assert_eq!(accounting.balance(&expected).unwrap(), before);
    assert_ne!(inputs.selection_ticket(), &expected);
    let left = inputs
        .argument_ticket(&mut accounting, 0, 10_000)
        .unwrap()
        .clone();
    let right = inputs
        .argument_ticket(&mut accounting, 1, 10_000)
        .unwrap()
        .clone();
    assert_ne!(left, right);
    assert_eq!(
        accounting
            .admit(0, 10_000)
            .unwrap()
            .identity()
            .requirement(),
        3
    );
    assert_eq!(
        accounting
            .admit(1, 10_000)
            .unwrap()
            .identity()
            .requirement(),
        1
    );
}

#[test]
fn nested_call_reuses_an_outer_argument_expectation_without_readmission() {
    let hir = parse("fn inspect(x: i32) -> Unit = outer(inner(x))");
    let body = inspect_body(&hir);
    let mut call_sites = sites(body);
    call_sites.sort_by_key(|site| {
        let ExprKind::Call(call) = body.expression(site.expression()).unwrap().kind() else {
            unreachable!()
        };
        let ExprKind::Name(path) = body.expression(call.callee).unwrap().kind() else {
            unreachable!()
        };
        path.segments()[0].text() != Some("outer")
    });
    let mut accounting = accounting();
    let mut outer = admit(&mut accounting, body, call_sites[0]);
    let expected = outer
        .argument_ticket(&mut accounting, 0, 10_000)
        .unwrap()
        .clone();
    let before = accounting.balance(&expected).unwrap();
    let inner = AnalysisCallInputs::admit(
        &mut accounting,
        0,
        call_sites[1],
        body,
        10_000,
        Some(&expected),
    )
    .unwrap();
    assert_eq!(inner.contextual_result_ticket(), Some(&expected));
    assert_eq!(accounting.balance(&expected).unwrap(), before);
    assert_ne!(inner.selection_ticket(), outer.selection_ticket());
}

#[test]
fn aggregate_argument_type_is_one_written_slot_and_receiver_is_not_a_new_slot() {
    for text in [
        "type Pair = (i32, i32)\nfn inspect(x: Pair) -> Unit = f(x)",
        "fn inspect(x: i32) -> Unit = x.f(x)",
    ] {
        let hir = parse(text);
        let body = inspect_body(&hir);
        let mut accounting = accounting();
        let mut inputs = admit(&mut accounting, body, sites(body)[0]);
        assert_eq!(inputs.written_argument_count(), 1);
        inputs.argument_ticket(&mut accounting, 0, 10_000).unwrap();
        assert_eq!(
            accounting
                .admit(0, 10_000)
                .unwrap()
                .identity()
                .requirement(),
            2
        );
    }
}

#[test]
fn explicit_type_inputs_and_trailing_closure_do_not_repartition_value_slots() {
    for (text, count) in [
        ("fn inspect(x: i32) -> Unit = f::[i32](x)", 1),
        ("fn inspect(x: i32) -> Unit = f { |y| y }", 1),
    ] {
        let hir = parse(text);
        let body = inspect_body(&hir);
        let mut accounting = accounting();
        let inputs = admit(&mut accounting, body, sites(body)[0]);
        assert_eq!(inputs.written_argument_count(), count);
        assert_eq!(inputs.contextual_result_ticket(), None);
    }
}

#[test]
fn partial_and_postfix_desugared_calls_do_not_mint_written_slot_budgets() {
    for expression in ["f(_)", "x.then(f)", "(x, x).apply(f)"] {
        let hir = parse(&format!("fn inspect(x: i32) -> Unit = {expression}"));
        let body = inspect_body(&hir);
        let site = sites(body)[0];
        let mut accounting = accounting();
        assert_eq!(
            failure(AnalysisCallInputs::admit(
                &mut accounting,
                0,
                site,
                body,
                10_000,
                None
            )),
            AnalysisCallInputFailure::NotOriginalCall(site),
        );
        assert_eq!(accounting.incomplete(), None);
        assert_eq!(
            accounting
                .admit(0, 10_000)
                .unwrap()
                .identity()
                .requirement(),
            0
        );
    }
}

#[test]
fn invalid_source_and_argument_identity_leave_accounting_untouched() {
    let hir = parse("fn inspect(x: i32) -> Unit = f(x)\nfn other() -> Unit = ()");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let other = hir
        .bodies()
        .map(|(_, body)| body)
        .find(|other| other.id() != body.id())
        .unwrap();
    let mut accounting = accounting();
    assert_eq!(
        failure(AnalysisCallInputs::admit(
            &mut accounting,
            0,
            site,
            other,
            10_000,
            None
        )),
        AnalysisCallInputFailure::NotOriginalCall(site)
    );
    let noncall = body
        .expressions()
        .find(|(_, e)| !matches!(e.kind(), ExprKind::Call(_)))
        .unwrap()
        .0;
    let noncall = ExpressionKey::new(site.module(), noncall);
    assert_eq!(
        failure(AnalysisCallInputs::admit(
            &mut accounting,
            0,
            noncall,
            body,
            10_000,
            None
        )),
        AnalysisCallInputFailure::NotOriginalCall(noncall)
    );

    let mut inputs = admit(&mut accounting, body, site);
    let before = accounting.balance(inputs.selection_ticket()).unwrap();
    assert_eq!(
        failure(inputs.argument_ticket(&mut accounting, 1, 10_000)),
        AnalysisCallInputFailure::UnknownArgument {
            call: site,
            index: 1
        }
    );
    assert_eq!(
        accounting.balance(inputs.selection_ticket()).unwrap(),
        before
    );
    assert_eq!(accounting.incomplete(), None);
    assert_eq!(
        accounting
            .admit(0, 10_000)
            .unwrap()
            .identity()
            .requirement(),
        1
    );
}

#[test]
fn foreign_context_is_rejected_before_admission_and_foreign_replay_cannot_reset() {
    let hir = parse("fn inspect(x: i32) -> Unit = f(x)");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let mut first = accounting();
    let mut second = accounting();
    let other = second.admit(0, 10_000).unwrap();
    let foreign = AnalysisCallInputFailure::Work(AnalysisWorkFailure::Claim(
        AnalysisRootClaimError::ForeignAttempt,
    ));
    assert_eq!(
        failure(AnalysisCallInputs::admit(
            &mut first,
            0,
            site,
            body,
            10_000,
            Some(&other)
        )),
        foreign
    );

    let mut inputs = admit(&mut first, body, site);
    assert_eq!(inputs.selection_ticket().identity(), other.identity());
    assert_ne!(inputs.selection_ticket(), &other);
    assert_eq!(
        failure(inputs.argument_ticket(&mut second, 0, 10_000)),
        foreign
    );
    assert!(inputs.arguments[0].is_none());
    let ticket = inputs
        .argument_ticket(&mut first, 0, 10_000)
        .unwrap()
        .clone();
    let before = first.balance(&ticket).unwrap();
    assert_eq!(
        failure(inputs.argument_ticket(&mut second, 0, u64::MAX)),
        foreign
    );
    assert_eq!(first.balance(&ticket).unwrap(), before);
    assert_eq!(second.admit(0, 10_000).unwrap().identity().requirement(), 1);
}

#[test]
fn holder_preparation_refuses_before_header_writes_or_root_publication() {
    let hir = parse("fn inspect(x: i32) -> Unit = f(x, x)");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let mut accounting = accounting();
    HEADER_WRITES.set(0);
    let refused = failure(AnalysisCallInputs::admit(
        &mut accounting,
        0,
        site,
        body,
        admission_ticks(),
        None,
    ));
    let Some(stopped) = accounting.incomplete() else {
        panic!("incomplete expected")
    };
    assert_eq!(
        refused,
        AnalysisCallInputFailure::Work(AnalysisWorkFailure::Incomplete(stopped))
    );
    assert_eq!(stopped.root().requirement(), 0);
    assert_eq!(HEADER_WRITES.get(), 0);
    assert_eq!(
        failure(AnalysisCallInputs::admit(
            &mut accounting,
            0,
            site,
            body,
            u64::MAX,
            None
        )),
        refused
    );
}

#[test]
fn slot_install_is_paid_before_argument_balance_or_ticket_publication() {
    let hir = parse("fn inspect(x: i32) -> Unit = f(x, x)");
    let body = inspect_body(&hir);
    let mut accounting = accounting();
    let mut inputs = admit(&mut accounting, body, sites(body)[0]);
    SLOT_INSTALLS.set(0);
    let refused = failure(inputs.argument_ticket(&mut accounting, 0, admission_ticks()));
    let stopped = accounting.incomplete().unwrap();
    assert_eq!(
        refused,
        AnalysisCallInputFailure::Work(AnalysisWorkFailure::Incomplete(stopped))
    );
    assert_eq!(stopped.root().requirement(), 1);
    assert_eq!(SLOT_INSTALLS.get(), 0);
    assert!(inputs.arguments.iter().all(Option::is_none));
    assert_eq!(
        failure(inputs.argument_ticket(&mut accounting, 0, u64::MAX)),
        refused
    );
    assert_eq!(
        failure(inputs.argument_ticket(&mut accounting, 1, u64::MAX)),
        refused
    );
}

#[test]
fn existing_argument_and_selection_work_observe_the_same_sticky_stop() {
    let hir = parse("fn inspect(x: i32) -> Unit = f(x, x)");
    let body = inspect_body(&hir);
    let mut accounting = accounting();
    let mut inputs = admit(&mut accounting, body, sites(body)[0]);
    let ticket = inputs
        .argument_ticket(&mut accounting, 0, 10_000)
        .unwrap()
        .clone();
    let refused = failure(inputs.argument_ticket(&mut accounting, 1, 0));
    let before = accounting.balance(&ticket).unwrap();
    assert_eq!(
        failure(inputs.argument_ticket(&mut accounting, 0, u64::MAX)),
        refused
    );
    assert_eq!(accounting.balance(&ticket).unwrap(), before);
    let expected = accounting.incomplete().unwrap();
    assert_eq!(
        accounting.claim(inputs.callee_result_ticket()).err(),
        Some(AnalysisWorkFailure::Incomplete(expected))
    );
    assert!(matches!(
        expected.reason(),
        AnalysisRootStopReason::Exhausted { .. }
    ));
}

#[test]
fn oversized_header_estimate_is_typed_incomplete_without_publication() {
    let mut accounting = accounting();
    let estimate = AnalysisCallInputs::header_work(usize::MAX);
    assert_eq!(
        estimate,
        Err(AnalysisWorkOverflow::Quantity(
            AnalysisWorkCategory::ReservedBytes
        ))
    );
    let refused = accounting
        .admit_with_work(0, u64::MAX, estimate)
        .unwrap_err();
    let stopped = accounting.incomplete().unwrap();
    assert_eq!(refused, AnalysisWorkFailure::Incomplete(stopped));
    assert_eq!(
        stopped.reason(),
        AnalysisRootStopReason::Unrepresentable(AnalysisWorkOverflow::Quantity(
            AnalysisWorkCategory::ReservedBytes
        ))
    );
}

#[test]
fn separate_written_calls_keep_distinct_selection_and_argument_custody() {
    let hir = parse("fn inspect(x: i32) -> Unit {\nf(x)\nf(x)\n}");
    let body = inspect_body(&hir);
    let calls = sites(body);
    assert_eq!(calls.len(), 2);
    let mut accounting = accounting();
    let mut first = admit(&mut accounting, body, calls[0]);
    let mut second = admit(&mut accounting, body, calls[1]);
    assert_ne!(first.site(), second.site());
    assert_ne!(first.selection_ticket(), second.selection_ticket());
    assert_ne!(
        first.argument_ticket(&mut accounting, 0, 10_000).unwrap(),
        second.argument_ticket(&mut accounting, 0, 10_000).unwrap(),
    );
    assert_eq!(
        accounting
            .admit(0, 10_000)
            .unwrap()
            .identity()
            .requirement(),
        4
    );
}

#[test]
fn unknown_consumer_refuses_before_header_preparation() {
    let hir = parse("fn inspect(x: i32) -> Unit = f(x)");
    let body = inspect_body(&hir);
    let mut accounting = accounting();
    HEADER_WRITES.set(0);
    assert_eq!(
        failure(AnalysisCallInputs::admit(
            &mut accounting,
            2,
            sites(body)[0],
            body,
            10_000,
            None
        )),
        AnalysisCallInputFailure::Work(AnalysisWorkFailure::Claim(
            AnalysisRootClaimError::UnknownConsumer
        )),
    );
    assert_eq!(HEADER_WRITES.get(), 0);
    assert_eq!(accounting.incomplete(), None);
}

#[test]
fn trailing_call_chain_keeps_each_original_invocation_separate() {
    let hir = parse("fn inspect(x: i32) -> Unit = f(x) { |y| y }");
    let body = inspect_body(&hir);
    let mut accounting = accounting();
    let calls = sites(body);
    assert_eq!(calls.len(), 2);
    for site in calls {
        let mut inputs = admit(&mut accounting, body, site);
        assert_eq!(inputs.written_argument_count(), 1);
        inputs.argument_ticket(&mut accounting, 0, 10_000).unwrap();
    }
    assert_eq!(
        accounting
            .admit(0, 10_000)
            .unwrap()
            .identity()
            .requirement(),
        4
    );
}
