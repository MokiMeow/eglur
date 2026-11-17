use std::fmt::Debug;

use super::*;

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Snapshot {
    tables: Vec<BTreeMap<String, String>>,
    backing: [Vec<String>; 4],
}

fn table<K: Ord + Debug, T: Debug>(facts: &FactTable<K, T>) -> BTreeMap<String, String> {
    facts
        .iter()
        .map(|(key, fact)| (format!("{key:?}"), format!("{fact:?}")))
        .collect()
}

fn sequence<T: Debug>(values: &[T]) -> Vec<String> {
    values.iter().map(|value| format!("{value:?}")).collect()
}

macro_rules! snapshot {
    ($facts:expr) => {{
        let facts = $facts;
        Snapshot {
            tables: vec![
                table(&facts.expressions),
                table(&facts.locals),
                table(&facts.members),
                table(&facts.invocations),
                table(&facts.callable_values),
                table(&facts.reduce_by_commitments),
                table(&facts.regions),
                table(&facts.checked_numeric),
                table(&facts.ownership_operations),
                table(&facts.bodies),
            ],
            backing: [
                sequence(&facts.transfers),
                sequence(&facts.callable_instances),
                sequence(&facts.obligations),
                sequence(&facts.method_dispatches),
            ],
        }
    }};
}

impl Snapshot {
    pub(super) fn body(facts: &BodyTypes) -> Self {
        snapshot!(facts)
    }

    pub(super) fn prefix(facts: &BodyTypingPrefix) -> Self {
        snapshot!(facts)
    }

    pub(super) fn checker(facts: &BodyChecker<'_, '_, '_>) -> Self {
        snapshot!(facts)
    }

    pub(super) fn assert_retained_from(&self, complete: &Self) {
        for (prefix, complete) in self.tables.iter().zip(&complete.tables) {
            for (key, fact) in prefix {
                assert_eq!(Some(fact), complete.get(key), "changed prefix fact {key}");
            }
        }
        for (prefix, complete) in self.backing.iter().zip(&complete.backing) {
            for value in prefix {
                assert!(complete.contains(value), "unbacked prefix record {value}");
            }
        }
        // Callable-instance IDs index this sequence, so set membership alone is insufficient.
        assert!(complete.backing[1].starts_with(&self.backing[1]));
    }
}
