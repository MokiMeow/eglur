use std::collections::HashSet;

use super::source_obligation_key;
use crate::{CanonicalTarget, DefId, ExternalDefId, ModuleId};

#[test]
fn target_projection_is_injective_across_modules_locals_and_external_ids() {
    let mut targets = Vec::new();
    for module in [0, 1, 12] {
        for local in [0, 1, 23] {
            targets.push(CanonicalTarget::Source(
                DefId::from_index(ModuleId::from_index(module).unwrap(), local).unwrap(),
            ));
        }
    }
    for external in [
        "0:1",
        "source:0:1",
        "external:source:0:1",
        "std.Mark.Item",
        "0:01",
    ] {
        targets.push(CanonicalTarget::External(
            ExternalDefId::new(external).unwrap(),
        ));
    }
    let keys = targets
        .iter()
        .map(source_obligation_key)
        .collect::<HashSet<_>>();
    assert_eq!(keys.len(), targets.len());
    for target in targets {
        assert_eq!(
            source_obligation_key(&target),
            source_obligation_key(&target.clone())
        );
    }
}
