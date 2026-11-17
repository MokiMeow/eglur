use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Work {
    pub pairs: usize,
    pub includes: usize,
    pub finishes: usize,
}

thread_local! {
    static WORK: Cell<Work> = Cell::new(Work::default());
}

pub(super) enum Operation {
    Pair,
    Include,
    Finish,
}

pub(super) fn observe(operation: Operation) {
    WORK.update(|mut work| {
        match operation {
            Operation::Pair => work.pairs += 1,
            Operation::Include => work.includes += 1,
            Operation::Finish => work.finishes += 1,
        }
        work
    });
}

pub(crate) fn reset() {
    WORK.set(Work::default());
}

pub(crate) fn work() -> Work {
    WORK.get()
}
