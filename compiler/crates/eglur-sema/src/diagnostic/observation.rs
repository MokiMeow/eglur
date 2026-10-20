use std::cell::Cell;

use super::{Diagnostic, SourceObligationKey};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RootEvidence {
    pub diagnostic: Diagnostic,
    pub obligation: Option<SourceObligationKey>,
}

thread_local! {
    static OBSERVER: Cell<Option<fn(RootEvidence)>> = const { Cell::new(None) };
}

pub(super) fn prepare(
    diagnostic: &Diagnostic,
    obligation: Option<&SourceObligationKey>,
) -> Option<RootEvidence> {
    OBSERVER.with(|observer| {
        observer.get().map(|_| RootEvidence {
            diagnostic: diagnostic.clone(),
            obligation: obligation.cloned(),
        })
    })
}

pub(super) fn publish(evidence: Option<RootEvidence>) {
    if let Some(evidence) = evidence {
        OBSERVER.with(|observer| observer.get().expect("active diagnostic observer")(evidence));
    }
}

pub(crate) fn capture<T>(observer: fn(RootEvidence), run: impl FnOnce() -> T) -> T {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            OBSERVER.set(None);
        }
    }

    OBSERVER.with(|slot| assert!(slot.replace(Some(observer)).is_none()));
    let _restore = Restore;
    run()
}
