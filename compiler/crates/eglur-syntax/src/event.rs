use crate::SyntaxKind;

#[derive(Clone, Debug)]
pub(crate) enum Event {
    Tombstone,
    Start {
        kind: SyntaxKind,
        forward_parent: Option<usize>,
    },
    Finish,
    Token {
        physical_index: usize,
    },
    Missing {
        kind: SyntaxKind,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Marker {
    position: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CompletedMarker {
    position: usize,
}

impl Marker {
    pub(crate) fn new(position: usize) -> Self {
        Self { position }
    }

    pub(crate) const fn inactive() -> Self {
        Self {
            position: usize::MAX,
        }
    }

    pub(crate) fn position(self) -> Option<usize> {
        (self.position != usize::MAX).then_some(self.position)
    }
}

impl CompletedMarker {
    pub(crate) fn new(position: usize) -> Self {
        Self { position }
    }

    pub(crate) const fn inactive() -> Self {
        Self {
            position: usize::MAX,
        }
    }

    pub(crate) fn position(self) -> Option<usize> {
        (self.position != usize::MAX).then_some(self.position)
    }
}
