use std::cell::RefCell;

use super::facts::Snapshot;
use super::*;
use crate::analysis_work::ChargeEvent;
use crate::body_types::checker::expectation::tests::Event as TypingEvent;
use crate::diagnostic::observation::{self, RootEvidence};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Event {
    Charge(ChargeEvent),
    Diagnostic(RootEvidence),
    Expression(ExpressionKey),
    Typing(&'static str),
    PublishedBody(DefId, String),
}

#[derive(Default)]
pub(super) struct Trace {
    pub events: Vec<Event>,
    pub terminal: Option<bool>,
    pub terminal_facts: Option<Snapshot>,
    pub finishes: usize,
    pub corrupt_cleanup: bool,
    pub corrupted: bool,
}

thread_local! {
    static TRACE: RefCell<Option<Trace>> = const { RefCell::new(None) };
}

fn observe(event: Event) {
    TRACE.with_borrow_mut(|trace| {
        if let Some(trace) = trace {
            trace.events.push(event);
        }
    });
}

pub(super) fn charge(event: ChargeEvent) {
    observe(Event::Charge(event));
}

pub(in crate::body_types::checker) fn expression(key: ExpressionKey) {
    observe(Event::Expression(key));
}

pub(in crate::body_types::checker) fn typing(event: &TypingEvent) {
    let kind = match event {
        TypingEvent::Request { .. } => "request",
        TypingEvent::Normalize(_) => "normalize",
        TypingEvent::Candidate(_) => "candidate",
        TypingEvent::Resume { .. } => "resume",
    };
    observe(Event::Typing(kind));
}

pub(in crate::body_types::checker) fn finished() {
    TRACE.with_borrow_mut(|trace| {
        if let Some(trace) = trace {
            trace.finishes += 1;
        }
    });
}

pub(in crate::body_types::checker) fn terminal(checker: &BodyChecker<'_, '_, '_>) {
    TRACE.with_borrow_mut(|trace| {
        if let Some(trace) = trace {
            assert!(trace.terminal.is_none(), "terminal resolved twice");
            trace.terminal_facts = Some(Snapshot::checker(checker));
            trace.terminal = Some(
                checker.region_stack.is_empty()
                    && checker.block_region_overrides.is_empty()
                    && checker.result_contexts.is_empty()
                    && checker.question_floors == [0]
                    && checker.checked_contexts.is_empty()
                    && checker.control_path_barriers.is_empty()
                    && checker.current_owner.is_none()
                    && checker.current_return.is_none()
                    && !checker.preparing_numeric_tree,
            );
        }
    });
}

pub(in crate::body_types::checker) fn published_body(
    owner: DefId,
    fact: FactState<'_, BodyTypeSummary>,
) {
    TRACE.with_borrow_mut(|trace| {
        if let Some(trace) = trace {
            trace
                .events
                .push(Event::PublishedBody(owner, format!("{fact:?}")));
        }
    });
}

pub(in crate::body_types::checker) fn before_unwind(checker: &mut BodyChecker<'_, '_, '_>) {
    TRACE.with_borrow_mut(|trace| {
        if let Some(trace) = trace
            && trace.corrupt_cleanup
            && !trace.corrupted
            && !checker.region_stack.is_empty()
            && checker
                .analysis_work
                .as_ref()
                .is_some_and(|work| work.accounting().incomplete().is_some())
        {
            checker.region_stack.clear();
            trace.corrupted = true;
        }
    });
}

pub(super) fn capture<T>(corrupt_cleanup: bool, run: impl FnOnce() -> T) -> (T, Trace) {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            TRACE.with_borrow_mut(|trace| *trace = None);
        }
    }

    TRACE.with_borrow_mut(|trace| {
        assert!(trace.is_none());
        *trace = Some(Trace {
            corrupt_cleanup,
            ..Trace::default()
        });
    });
    let _restore = Restore;
    let result = observation::capture(|root| observe(Event::Diagnostic(root)), run);
    (result, TRACE.with_borrow_mut(|trace| trace.take().unwrap()))
}
