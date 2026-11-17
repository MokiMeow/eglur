use crate::record_reservation::RecordReservation;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BoundLink {
    Leaf { key: usize, holder: usize },
    Branch(usize),
}

#[derive(Clone, Copy)]
pub(super) struct BoundBranch {
    bit: u32,
    children: [BoundLink; 2],
}

pub(super) enum BoundLookup {
    Existing(usize),
    Vacant(BoundInsertion),
}

pub(super) struct BoundInsertion {
    parent: Option<(usize, usize)>,
    previous: Option<BoundLink>,
    bit: u32,
    key: usize,
}

impl BoundInsertion {
    pub(super) const fn needs_branch(&self) -> bool {
        self.previous.is_some()
    }
}

#[derive(Default)]
pub(super) struct BoundIndex {
    branches: Vec<BoundBranch>,
    #[cfg(test)]
    inspected: std::cell::Cell<usize>,
}

impl BoundIndex {
    pub(super) fn lookup(&self, root: Option<BoundLink>, key: usize) -> BoundLookup {
        #[cfg(test)]
        self.inspected.set(0);
        let Some(mut link) = root else {
            return BoundLookup::Vacant(BoundInsertion {
                parent: None,
                previous: None,
                bit: 0,
                key,
            });
        };
        let found = loop {
            match link {
                BoundLink::Leaf { key, holder } => break (key, holder),
                BoundLink::Branch(index) => {
                    let branch = self.branch(index);
                    link = branch.children[direction(key, branch.bit)];
                }
            }
        };
        if found.0 == key {
            return BoundLookup::Existing(found.1);
        }

        // Branch bits strictly increase, so both walks are bounded by the key width.
        let bit = (found.0 ^ key).leading_zeros();
        let mut parent = None;
        let mut link = root.expect("nonempty root was inspected");
        while let BoundLink::Branch(index) = link {
            let branch = self.branch(index);
            if branch.bit >= bit {
                break;
            }
            let side = direction(key, branch.bit);
            parent = Some((index, side));
            link = branch.children[side];
        }
        BoundLookup::Vacant(BoundInsertion {
            parent,
            previous: Some(link),
            bit,
            key,
        })
    }

    pub(super) fn reservation(
        &self,
        insertion: &BoundInsertion,
    ) -> Option<Option<RecordReservation<BoundBranch>>> {
        if insertion.needs_branch() {
            RecordReservation::try_for_append(&self.branches).map(Some)
        } else {
            Some(None)
        }
    }

    pub(super) fn install(
        &mut self,
        root: &mut Option<BoundLink>,
        insertion: BoundInsertion,
        holder: usize,
        reservation: Option<RecordReservation<BoundBranch>>,
    ) {
        let leaf = BoundLink::Leaf {
            key: insertion.key,
            holder,
        };
        let replacement = if let Some(previous) = insertion.previous {
            reservation
                .expect("nonempty insertion needs a checked branch reservation")
                .reserve(&mut self.branches);
            let mut children = [previous; 2];
            children[direction(insertion.key, insertion.bit)] = leaf;
            let index = self.branches.len();
            self.branches.push(BoundBranch {
                bit: insertion.bit,
                children,
            });
            BoundLink::Branch(index)
        } else {
            leaf
        };
        if let Some((index, side)) = insertion.parent {
            self.branches[index].children[side] = replacement;
        } else {
            *root = Some(replacement);
        }
    }

    pub(super) const fn branch_size() -> usize {
        size_of::<BoundBranch>()
    }

    #[cfg(test)]
    pub(super) fn inspected_branches(&self) -> usize {
        self.inspected.get()
    }

    fn branch(&self, index: usize) -> BoundBranch {
        #[cfg(test)]
        self.inspected.set(self.inspected.get() + 1);
        self.branches[index]
    }
}

const fn direction(key: usize, bit: u32) -> usize {
    (key >> (usize::BITS - bit - 1)) & 1
}

#[cfg(test)]
mod tests;
