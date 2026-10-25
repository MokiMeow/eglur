use std::collections::BTreeMap;
use std::mem::size_of;

use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticStore, Poison, PrimarySpan, Severity,
};
use eglur_hir::{BodyId, Mutability, lower};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, parse};

use super::*;
use crate::{ControlRegionKey, DefId, ModuleId, TypeId};

fn bodies() -> Vec<BodyId> {
    bodies_with_count(3)
}

fn bodies_with_count(count: usize) -> Vec<BodyId> {
    let text = (0..count)
        .map(|index| format!("fn f{index}() -> Unit = ()\n"))
        .collect::<String>();
    let source = SourceFile::new(FileId::new(0), text.into_bytes());
    let FrontendOutcome::Parsed(parsed) = parse(&source) else {
        panic!("fixture must parse")
    };
    assert!(parsed.is_success());
    lower(&parsed).bodies().map(|(id, _)| id).collect()
}

#[test]
fn many_bodies_allocate_local_rows_only_for_recorded_owners() {
    let poisons = poisons();
    for count in [1, 16, 256] {
        let bodies = bodies_with_count(count);
        for stride in [1, 16] {
            let mut table = LocalTypeFacts::new();
            let selected = (0..count).step_by(stride).collect::<Vec<_>>();
            for &index in selected.iter().rev() {
                table
                    .insert(binding(0, bodies[index], 0), fact(index, &poisons))
                    .unwrap();
            }
            assert_eq!(table.len(), selected.len());
            let directory = &table.slots[0];
            assert_eq!(directory.iter().flatten().count(), selected.len());
            assert_eq!(
                directory
                    .iter()
                    .flatten()
                    .map(|row| row.positions.len())
                    .sum::<usize>(),
                selected.len()
            );
            for (index, &body) in bodies.iter().enumerate() {
                assert_eq!(
                    table.recorded_state(&binding(0, body, 0)).is_some(),
                    index.is_multiple_of(stride)
                );
            }
            println!(
                "local-body-slots bodies={count} stride={stride} recorded={} owner_rows={} local_capacity={}",
                table.len(),
                directory.len(),
                directory
                    .iter()
                    .flatten()
                    .map(|row| row.positions.capacity())
                    .sum::<usize>()
            );
        }
    }
}

fn binding(module: usize, body: BodyId, local: usize) -> LocalBindingId {
    LocalBindingId::from_index(ModuleId::from_index(module).unwrap(), body, local).unwrap()
}

fn poisons() -> [Poison; 2] {
    let mut store = DiagnosticStore::new();
    let mut root = |index| {
        let diagnostic = Diagnostic::new(
            DiagnosticCode::new(format!("ROOT_{index}")).unwrap(),
            Severity::Error,
            DiagnosticPhase::new("type").unwrap(),
            "independent failure",
            PrimarySpan::Unavailable,
        );
        store
            .record_failure(DiagnosticFailure::Root(
                DiagnosticRoot::boundary(
                    BoundaryRootKey::new(format!("root-{index}")).unwrap(),
                    diagnostic,
                )
                .unwrap(),
            ))
            .unwrap()
    };
    let first = root(0);
    let both = first.merge(&root(1)).unwrap();
    [first, both]
}

fn fact(index: usize, poisons: &[Poison; 2]) -> SemanticFact<LocalTypeFact> {
    match index % 5 {
        0 | 1 => SemanticFact::Known(LocalTypeFact {
            ty: TypeId::from_index(17 + index % 2).unwrap(),
            mutability: if index.is_multiple_of(2) {
                Mutability::Immutable
            } else {
                Mutability::Mutable
            },
            region: ControlRegionKey::Callable(
                DefId::from_index(ModuleId::from_index(index % 3).unwrap(), index % 7).unwrap(),
            ),
        }),
        2 | 3 => SemanticFact::Poisoned(poisons[index % 5 - 2].clone()),
        4 => SemanticFact::NotApplicable,
        _ => unreachable!(),
    }
}

fn image(state: Option<FactState<'_, LocalTypeFact>>) -> String {
    format!("{state:?}")
}

fn shape(table: &LocalTypeFacts) -> (usize, usize, usize, usize, usize) {
    let bodies = table.slots.iter().map(Vec::capacity).sum();
    let locals = table
        .slots
        .iter()
        .flatten()
        .flatten()
        .map(|row| row.positions.capacity())
        .sum();
    (
        table.slots.len(),
        table.slots.capacity(),
        bodies,
        locals,
        table.records.capacity(),
    )
}

#[test]
fn qualified_queries_match_the_ordered_fact_table_after_shuffled_insertions() {
    let bodies = bodies();
    let poisons = poisons();
    let mut keys = (0..3)
        .flat_map(|module| {
            bodies
                .iter()
                .flat_map(move |&body| (0..129).map(move |local| binding(module, body, local)))
        })
        .collect::<Vec<_>>();
    let mut seed = 0x5eed_u64;
    for index in (1..keys.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        keys.swap(index, seed as usize % (index + 1));
    }
    let mut table = LocalTypeFacts::new();
    let mut oracle = crate::FactTable::new();

    for (index, &key) in keys.iter().enumerate() {
        table.insert(key, fact(index, &poisons)).unwrap();
        oracle.insert(key, fact(index, &poisons)).unwrap();
        assert_eq!(table.len(), oracle.iter().len());
        assert_eq!(
            image(table.recorded_state(&key)),
            image(oracle.recorded_state(&key))
        );
    }
    for key in &keys {
        assert_eq!(
            image(Some(table.state(key))),
            image(Some(oracle.state(key)))
        );
    }
    assert_eq!(table.has_poisoned(), oracle.has_poisoned());
    assert_eq!(table.facts().len(), table.len());
    assert_eq!(
        table
            .entries()
            .map(|(key, fact)| (key, format!("{fact:?}")))
            .collect::<Vec<_>>(),
        oracle
            .iter()
            .map(|(key, fact)| (*key, format!("{fact:?}")))
            .collect::<Vec<_>>()
    );
}

#[test]
fn every_recorded_state_refuses_replacement_without_allocating_or_mutating() {
    let body = bodies()[0];
    let poisons = poisons();
    let key = binding(0, body, 11);
    for first in 0..5 {
        for replacement in 0..5 {
            let mut table = LocalTypeFacts::new();
            table.insert(key, fact(first, &poisons)).unwrap();
            let before = image(table.recorded_state(&key));
            let allocation = shape(&table);

            assert_eq!(
                table.insert(key, fact(replacement, &poisons)),
                Err(DuplicateFact)
            );
            assert_eq!(table.len(), 1);
            assert_eq!(image(table.recorded_state(&key)), before);
            assert_eq!(shape(&table), allocation);
        }
    }
}

#[test]
fn missing_owner_and_hole_queries_allocate_nothing_and_remain_unperformed() {
    let bodies = bodies();
    let poisons = poisons();
    let mut table = LocalTypeFacts::new();
    table
        .insert(binding(0, bodies[1], 64), fact(4, &poisons))
        .unwrap();
    let allocation = shape(&table);
    for key in [
        binding(1, bodies[1], 64),
        binding(0, bodies[0], 64),
        binding(0, bodies[2], 64),
        binding(0, bodies[1], 0),
        binding(0, bodies[1], u32::MAX as usize),
        binding(u32::MAX as usize, bodies[1], 64),
    ] {
        assert!(table.recorded_state(&key).is_none());
        assert!(matches!(table.state(&key), FactState::Unperformed));
        assert_eq!(shape(&table), allocation);
    }
    assert!(matches!(
        table.state(&binding(0, bodies[1], 64)),
        FactState::NotApplicable
    ));
    assert!(!table.has_poisoned());
}

#[test]
fn interrupted_prefix_keeps_the_same_records_and_allocation() {
    let body = bodies()[0];
    let poisons = poisons();
    let mut table = LocalTypeFacts::new();
    let keys = (0..5)
        .map(|index| binding(0, body, index))
        .collect::<Vec<_>>();
    for (index, &key) in keys.iter().enumerate() {
        table.insert(key, fact(index, &poisons)).unwrap();
    }
    let allocation = shape(&table);
    let records = table.records.as_ptr();
    let mut prefix = super::super::BodyTypingPrefix::empty();
    prefix.locals = table;

    assert_eq!(prefix.fact_count(), keys.len());
    assert_eq!(prefix.locals.records.as_ptr(), records);
    assert_eq!(shape(&prefix.locals), allocation);
    for (index, key) in keys.iter().enumerate() {
        let expected = fact(index, &poisons);
        assert_eq!(
            format!("{:?}", prefix.locals.state(key)),
            match &expected {
                SemanticFact::Known(value) => format!("{:?}", FactState::Known(value)),
                SemanticFact::Poisoned(poison) =>
                    format!("{:?}", FactState::<LocalTypeFact>::Poisoned(poison)),
                SemanticFact::NotApplicable =>
                    format!("{:?}", FactState::<LocalTypeFact>::NotApplicable),
            }
        );
    }
}

#[test]
fn native_storage_volume_separates_empty_slots_from_recorded_fact_payloads() {
    let body = bodies()[0];
    let poisons = poisons();
    for width in [64, 1024, 4096] {
        for stride in [1, 16, 256] {
            let mut table = LocalTypeFacts::new();
            let keys = (0..width)
                .step_by(stride)
                .chain([width - 1])
                .collect::<Vec<_>>();
            let mut inserted = BTreeMap::new();
            for &local in keys.iter().rev() {
                let key = binding(0, body, local);
                if inserted.insert(local, ()).is_none() {
                    table.insert(key, fact(local, &poisons)).unwrap();
                }
            }
            let slots = &table.slots[0][body.get() as usize]
                .as_ref()
                .unwrap()
                .positions;
            assert_eq!(slots.len(), width);
            assert_eq!(table.records.len(), inserted.len());
            assert_eq!(slots.iter().flatten().count(), inserted.len());
            assert!(
                slots
                    .iter()
                    .flatten()
                    .all(|position| position.get() <= table.records.len())
            );

            let slot_bytes = slots.capacity() * size_of::<Option<NonZeroUsize>>();
            let record_bytes = table.records.capacity() * size_of::<SemanticFact<LocalTypeFact>>();
            let directory_bytes = table.slots.capacity() * size_of::<Vec<Option<BodyLocalSlots>>>()
                + table
                    .slots
                    .iter()
                    .map(|bodies| bodies.capacity() * size_of::<Option<BodyLocalSlots>>())
                    .sum::<usize>();
            let inline_payload = slots.len() * size_of::<Option<SemanticFact<LocalTypeFact>>>();
            println!(
                "local-slots width={width} stride={stride} records={} slots={slot_bytes} records_bytes={record_bytes} directory={directory_bytes} inline_payload={inline_payload}",
                inserted.len()
            );
        }
    }
}
