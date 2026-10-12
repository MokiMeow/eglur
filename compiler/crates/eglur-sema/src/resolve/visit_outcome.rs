use super::{NameResolutionError, Poison, ResolvedName};
use crate::owned_poison::OwnedPoisonUnion;

#[derive(Clone)]
pub(super) enum VisitOutcome {
    Known,
    Resolved(ResolvedName),
    Poisoned(Poison),
    Unperformed,
    NotApplicable,
}

impl VisitOutcome {
    pub(super) fn merge(self, other: Self) -> Result<Self, NameResolutionError> {
        match (self, other) {
            (Self::Poisoned(left), Self::Poisoned(right)) => {
                Ok(Self::Poisoned(left.merge(&right)?))
            }
            (Self::Poisoned(poison), _) | (_, Self::Poisoned(poison)) => Ok(Self::Poisoned(poison)),
            (Self::Unperformed, _) | (_, Self::Unperformed) => Ok(Self::Unperformed),
            (Self::NotApplicable, value) | (value, Self::NotApplicable) => Ok(value),
            (Self::Resolved(_), Self::Resolved(_)) => Ok(Self::Known),
            (Self::Resolved(_), Self::Known) | (Self::Known, Self::Resolved(_)) => Ok(Self::Known),
            (Self::Known, Self::Known) => Ok(Self::Known),
        }
    }
}

pub(super) enum VisitFold {
    Empty,
    Known,
    Resolved(ResolvedName),
    Poisoned(OwnedPoisonUnion),
    Unperformed,
    NotApplicable,
}

impl VisitFold {
    pub(super) fn include(self, next: VisitOutcome) -> Result<Self, NameResolutionError> {
        use VisitOutcome as Outcome;

        Ok(match (self, next) {
            (Self::Poisoned(pending), Outcome::Poisoned(next)) => {
                Self::Poisoned(pending.include(next)?)
            }
            (Self::Poisoned(pending), _) => Self::Poisoned(pending),
            (_, Outcome::Poisoned(poison)) => Self::Poisoned(OwnedPoisonUnion::new(poison)),
            (Self::Unperformed, _) | (_, Outcome::Unperformed) => Self::Unperformed,
            (Self::Empty | Self::NotApplicable, Outcome::Known) => Self::Known,
            (Self::Empty | Self::NotApplicable, Outcome::Resolved(value)) => Self::Resolved(value),
            (Self::Empty | Self::NotApplicable, Outcome::NotApplicable) => Self::NotApplicable,
            (Self::Known, Outcome::Known | Outcome::Resolved(_) | Outcome::NotApplicable) => {
                Self::Known
            }
            (Self::Resolved(value), Outcome::NotApplicable) => Self::Resolved(value),
            (Self::Resolved(_), Outcome::Known | Outcome::Resolved(_)) => Self::Known,
        })
    }

    pub(super) fn finish(self) -> VisitOutcome {
        match self {
            Self::Empty | Self::Known => VisitOutcome::Known,
            Self::Resolved(value) => VisitOutcome::Resolved(value),
            Self::Poisoned(pending) => VisitOutcome::Poisoned(pending.finish()),
            Self::Unperformed => VisitOutcome::Unperformed,
            Self::NotApplicable => VisitOutcome::NotApplicable,
        }
    }
}
