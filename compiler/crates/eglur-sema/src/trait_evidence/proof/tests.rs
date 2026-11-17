use std::fmt::Write;

use super::*;
use crate::{
    CanonicalTarget, DeclarationSignature, FactState, GenericParameterId, PrimitiveType, SubstId,
    TraitRef,
};

mod fixture;
mod recursive;

use fixture::{Fixture, fixture_from_source};

fn id(index: usize) -> EvidenceProofId {
    EvidenceProofId::from_index(index).unwrap()
}

fn obligation(fixture: &mut Fixture, name: &str, arguments: usize) -> TraitObligation {
    let target = CanonicalTarget::Source(fixture.declaration(name));
    let integer = fixture
        .signatures
        .types_mut()
        .primitive(PrimitiveType::I64)
        .unwrap();
    let substitution = fixture
        .signatures
        .types_mut()
        .intern_substitution(std::iter::repeat_n(integer, arguments))
        .unwrap();
    TraitObligation::new(integer, TraitRef::new(target, substitution))
}

fn run(
    fixture: &mut Fixture,
    arena: &mut ProofArena,
    owner: DefId,
    obligation: TraitObligation,
    selected: TraitProof,
    recursive: bool,
) -> Result<EvidenceProofId, TraitEvidenceError> {
    if recursive {
        arena.recursive_materialize(
            owner,
            obligation,
            selected,
            &mut fixture.signatures,
            &mut fixture.environment,
        )
    } else {
        arena.materialize(
            owner,
            obligation,
            selected,
            &mut fixture.signatures,
            &mut fixture.environment,
        )
    }
}

fn select(fixture: &mut Fixture, owner: DefId, obligation: &TraitObligation) -> TraitProof {
    fixture
        .environment
        .prove_for_owner(owner, &mut fixture.signatures, obligation.clone())
        .unwrap()
}

fn parity(
    text: &str,
    operation: impl Fn(
        &mut Fixture,
        &mut ProofArena,
        bool,
    ) -> Result<EvidenceProofId, TraitEvidenceError>,
) {
    let mut iterative = fixture_from_source(text);
    let mut recursive = fixture_from_source(text);
    let mut actual = ProofArena::default();
    let mut expected = ProofArena::default();
    assert_eq!(
        operation(&mut iterative, &mut actual, false),
        operation(&mut recursive, &mut expected, true)
    );
    assert_eq!(actual.nodes, expected.nodes);
    assert_eq!(actual.ids, expected.ids);
    assert_eq!(actual.by_obligation, expected.by_obligation);
    assert_eq!(actual.active, expected.active);
    let actual_types = iterative.signatures.types();
    let expected_types = recursive.signatures.types();
    assert_eq!(
        actual_types.types().collect::<Vec<_>>(),
        expected_types.types().collect::<Vec<_>>()
    );
    assert_eq!(
        actual_types.substitution_count(),
        expected_types.substitution_count()
    );
    for index in 0..actual_types.substitution_count() {
        let id = SubstId::from_index(index).unwrap();
        assert_eq!(
            actual_types.substitution(id).unwrap(),
            expected_types.substitution(id).unwrap()
        );
    }
}

fn chain_source(depth: usize) -> String {
    assert!(depth > 0);
    let mut text = String::from("trait Hop0 {}\nimpl Hop0 for i64 {}\n");
    for level in 1..depth {
        writeln!(
            text,
            "trait Hop{level} {{}}\nimpl[T: Hop{}] Hop{level} for T {{}}",
            level - 1
        )
        .unwrap();
    }
    text.push_str("fn direct() -> Unit = ()\n");
    text
}

fn completed_chain(fixture: &mut Fixture, depth: usize) -> (DefId, TraitObligation, TraitProof) {
    let owner = fixture.declaration("direct");
    let mut last = None;
    for level in 0..depth {
        let obligation = obligation(fixture, &format!("Hop{level}"), 0);
        let selected = select(fixture, owner, &obligation);
        assert!(matches!(selected, TraitProof::Implementation(_)));
        last = Some((owner, obligation, selected));
    }
    last.unwrap()
}

#[test]
fn bounded_chains_match_recursive_proof_and_store_order() {
    for depth in [1, 2, 8, 24] {
        parity(&chain_source(depth), |fixture, arena, recursive| {
            let (owner, obligation, selected) = completed_chain(fixture, depth);
            let result = run(fixture, arena, owner, obligation, selected, recursive);
            assert_eq!(result, Ok(id(depth - 1)));
            assert!(arena.active.is_empty());
            result
        });
    }
}

#[test]
fn deep_completed_selection_chain_materializes_without_native_recursion() {
    for depth in [256, 4096] {
        let mut fixture = fixture_from_source(&chain_source(depth));
        let (owner, obligation, selected) = completed_chain(&mut fixture, depth);
        let mut arena = ProofArena::default();
        assert_eq!(
            run(&mut fixture, &mut arena, owner, obligation, selected, false),
            Ok(id(depth - 1))
        );
        assert_eq!(arena.nodes.len(), depth);
        assert_eq!(arena.by_obligation.len(), depth);
        assert!(arena.active.is_empty());
        for (index, node) in arena.nodes.iter().enumerate() {
            let EvidenceSource::Implementation {
                implementation_bounds,
                trait_bounds,
                ..
            } = node
            else {
                panic!("a closed implementation chain has no bound witness")
            };
            let expected = if index == 0 {
                vec![]
            } else {
                vec![id(index - 1)]
            };
            assert_eq!(implementation_bounds.as_ref(), expected);
            assert!(trait_bounds.is_empty());
        }
    }
}

fn staged_source(parameters: &str) -> String {
    format!(
        "trait Left {{}}\ntrait Right {{}}\nimpl Left for i64 {{}}\nimpl Right for i64 {{}}\ntrait Task[A: Right] {{}}\nimpl[{parameters}] Task[A] for T {{}}\nfn direct() -> Unit = ()\n"
    )
}

#[test]
fn declaration_order_and_trait_stage_reuse_match_recursive_proofs() {
    for (parameters, right) in [("T: Left, A: Right", 1), ("A: Right, T: Left", 0)] {
        parity(&staged_source(parameters), |fixture, arena, recursive| {
            let owner = fixture.declaration("direct");
            let obligation = obligation(fixture, "Task", 1);
            let selected = select(fixture, owner, &obligation);
            let result = run(fixture, arena, owner, obligation, selected, recursive);
            assert_eq!(result, Ok(id(2)));
            let EvidenceSource::Implementation {
                implementation_bounds,
                trait_bounds,
                ..
            } = &arena.nodes[2]
            else {
                panic!("implementation expected")
            };
            assert_eq!(implementation_bounds.as_ref(), [id(0), id(1)]);
            assert_eq!(trait_bounds.as_ref(), [id(right)]);
            result
        });
    }
}

#[test]
fn declared_multi_bound_order_is_not_canonical_sort_order() {
    parity(
        "trait Alpha {}\ntrait Zulu {}\ntrait Root {}\nimpl Alpha for i64 {}\nimpl Zulu for i64 {}\nimpl[T: Zulu + Alpha] Root for T {}\nfn direct() -> Unit = ()\n",
        |fixture, arena, recursive| {
            let owner = fixture.declaration("direct");
            let root = obligation(fixture, "Root", 0);
            let zulu = obligation(fixture, "Zulu", 0);
            let alpha = obligation(fixture, "Alpha", 0);
            let selected = select(fixture, owner, &root);
            let result = run(fixture, arena, owner, root, selected, recursive);
            assert_eq!(arena.by_obligation.get(&(owner, zulu)), Some(&id(0)));
            assert_eq!(arena.by_obligation.get(&(owner, alpha)), Some(&id(1)));
            result
        },
    );
}

#[test]
fn repeated_wide_bounds_share_evidence_without_changing_edge_order() {
    for width in [1, 8, 64] {
        let names = (0..width).map(|i| format!("A{i}")).collect::<Vec<_>>();
        let bounds = names
            .iter()
            .map(|name| format!("{name}: Leaf"))
            .collect::<Vec<_>>()
            .join(", ");
        let text = format!(
            "trait Leaf {{}}\nimpl Leaf for i64 {{}}\ntrait Wide[{}] {{}}\nimpl[T: Leaf, {bounds}] Wide[{}] for T {{}}\nfn direct() -> Unit = ()\n",
            names.join(", "),
            names.join(", ")
        );
        parity(&text, |fixture, arena, recursive| {
            let owner = fixture.declaration("direct");
            let root = obligation(fixture, "Wide", width);
            let selected = select(fixture, owner, &root);
            let result = run(fixture, arena, owner, root, selected, recursive);
            assert_eq!(result, Ok(id(1)));
            let EvidenceSource::Implementation {
                implementation_bounds,
                trait_bounds,
                ..
            } = &arena.nodes[1]
            else {
                panic!("implementation expected")
            };
            assert_eq!(implementation_bounds.as_ref(), vec![id(0); width + 1]);
            assert!(trait_bounds.is_empty());
            result
        });
    }
}

#[test]
fn completed_obligation_cache_precedes_active_and_selected_validation() {
    parity(&chain_source(2), |fixture, arena, recursive| {
        let (owner, root, selected) = completed_chain(fixture, 2);
        let first = run(fixture, arena, owner, root.clone(), selected, recursive)?;
        arena.active.insert((owner, root.clone()));
        let result = run(
            fixture,
            arena,
            owner,
            root.clone(),
            TraitProof::Unsatisfied(root),
            recursive,
        );
        assert_eq!(result, Ok(first));
        result
    });
}

#[test]
fn active_obligation_precedes_invalid_selection_and_retains_failure_state() {
    parity(&chain_source(1), |fixture, arena, recursive| {
        let owner = fixture.declaration("direct");
        let root = obligation(fixture, "Hop0", 0);
        arena.active.insert((owner, root.clone()));
        let result = run(
            fixture,
            arena,
            owner,
            root.clone(),
            TraitProof::Unsatisfied(root.clone()),
            recursive,
        );
        assert_eq!(result, Err(TraitEvidenceError::CyclicCheckedProof(root)));
        result
    });
}

#[test]
fn an_active_prerequisite_retains_all_unfinished_ancestors() {
    parity(&chain_source(3), |fixture, arena, recursive| {
        let (owner, root, selected) = completed_chain(fixture, 3);
        let leaf = obligation(fixture, "Hop0", 0);
        let middle = obligation(fixture, "Hop1", 0);
        arena.active.insert((owner, leaf.clone()));
        let result = run(fixture, arena, owner, root.clone(), selected, recursive);
        assert_eq!(
            result,
            Err(TraitEvidenceError::CyclicCheckedProof(leaf.clone()))
        );
        assert_eq!(
            arena.active,
            BTreeSet::from([(owner, root), (owner, middle), (owner, leaf)])
        );
        assert!(arena.nodes.is_empty());
        assert!(arena.by_obligation.is_empty());
        result
    });
}

#[test]
fn unsuccessful_selection_does_not_publish_a_completed_proof() {
    for ambiguous in [false, true] {
        parity(&chain_source(1), |fixture, arena, recursive| {
            let owner = fixture.declaration("direct");
            let root = obligation(fixture, "Hop0", 0);
            let selected = if ambiguous {
                TraitProof::Ambiguous(root.clone())
            } else {
                TraitProof::Unsatisfied(root.clone())
            };
            let result = run(fixture, arena, owner, root.clone(), selected, recursive);
            assert_eq!(
                result,
                Err(TraitEvidenceError::UnmaterializablePrerequisite(
                    root.clone()
                ))
            );
            assert!(arena.nodes.is_empty());
            assert!(arena.by_obligation.is_empty());
            assert_eq!(arena.active, BTreeSet::from([(owner, root)]));
            result
        });
    }
}

#[test]
fn source_owner_bounds_retain_distinct_witness_identity() {
    parity(
        "trait Leaf {}\nfn first[T: Leaf]() -> Unit = ()\nfn second[T: Leaf]() -> Unit = ()\n",
        |fixture, arena, recursive| {
            let mut last = None;
            for name in ["first", "second"] {
                let owner = fixture.declaration(name);
                let FactState::Known(DeclarationSignature::Callable(callable)) =
                    fixture.signatures.source_declaration(owner)
                else {
                    panic!("callable expected")
                };
                let parameter = fixture
                    .signatures
                    .callable(*callable)
                    .unwrap()
                    .generic_parameters()[0]
                    .clone();
                let subject = fixture
                    .signatures
                    .types_mut()
                    .generic_parameter(parameter.clone())
                    .unwrap();
                let FactState::Known(signature) = fixture.signatures.generic_parameter(&parameter)
                else {
                    panic!("generic expected")
                };
                let root = TraitObligation::new(subject, signature.declared_bounds()[0].clone());
                let selected = select(fixture, owner, &root);
                assert!(
                    matches!(&selected, TraitProof::Bound(witness) if witness.owner() == owner)
                );
                last = Some(run(fixture, arena, owner, root, selected, recursive));
            }
            assert_eq!(arena.nodes.len(), 2);
            assert_ne!(arena.nodes[0], arena.nodes[1]);
            last.unwrap()
        },
    );
}

fn poison(fixture: &mut Fixture, target: CanonicalTarget) {
    let poison = crate::diagnostic::record_type_error(
        &mut fixture.diagnostics,
        "TRAIT_BOUND_NOT_SATISFIED",
        "unavailable checked signature",
        fixture.span.clone(),
        BTreeMap::new(),
    )
    .unwrap();
    assert!(fixture.signatures.poison_public_contract(target, poison));
}

#[test]
fn failed_later_parameter_retains_only_the_completed_earlier_proof() {
    parity(
        &staged_source("T: Left, A: Right"),
        |fixture, arena, recursive| {
            let owner = fixture.declaration("direct");
            let root = obligation(fixture, "Task", 1);
            let selected = select(fixture, owner, &root);
            let TraitProof::Implementation(evidence) = &selected else {
                panic!("checked implementation expected")
            };
            let (header, _) = fixture
                .environment
                .checked_implementation_contract(evidence.implementation())
                .unwrap();
            let GenericParameterId::Source(parameter) = header.generic_parameters()[1] else {
                panic!("source parameter expected")
            };
            poison(fixture, CanonicalTarget::Source(parameter));
            let result = run(fixture, arena, owner, root.clone(), selected, recursive);
            assert_eq!(
                result,
                Err(TraitEvidenceError::InvalidCheckedProof("generic signature"))
            );
            assert_eq!(arena.nodes.len(), 1);
            assert_eq!(arena.by_obligation.len(), 1);
            assert_eq!(arena.active, BTreeSet::from([(owner, root)]));
            result
        },
    );
}

#[test]
fn failed_trait_signature_is_observed_after_implementation_evidence() {
    parity(
        &staged_source("T: Left, A: Right"),
        |fixture, arena, recursive| {
            let owner = fixture.declaration("direct");
            let root = obligation(fixture, "Task", 1);
            let selected = select(fixture, owner, &root);
            poison(fixture, root.trait_ref().declaration().clone());
            let result = run(fixture, arena, owner, root.clone(), selected, recursive);
            assert_eq!(
                result,
                Err(TraitEvidenceError::InvalidCheckedProof(
                    "selected trait signature"
                ))
            );
            assert_eq!(arena.nodes.len(), 2);
            assert_eq!(arena.by_obligation.len(), 2);
            assert_eq!(arena.active, BTreeSet::from([(owner, root)]));
            result
        },
    );
}

#[test]
fn unknown_selected_implementation_retains_an_unfinished_root() {
    parity(&chain_source(1), |fixture, arena, recursive| {
        let owner = fixture.declaration("direct");
        let root = obligation(fixture, "Hop0", 0);
        let selected = select(fixture, owner, &root);
        let TraitProof::Implementation(evidence) = &selected else {
            panic!("implementation expected")
        };
        let target = evidence.implementation().clone();
        fixture.environment =
            fixture_from_source("trait Hop0 {}\nfn direct() -> Unit = ()\n").environment;
        let result = run(fixture, arena, owner, root.clone(), selected, recursive);
        assert_eq!(
            result,
            Err(TraitEvidenceError::UnknownCheckedImplementation(target))
        );
        assert_eq!(arena.active, BTreeSet::from([(owner, root)]));
        assert!(arena.nodes.is_empty());
        result
    });
}

#[test]
fn a_shared_diamond_completes_dependencies_before_both_parent_edges() {
    parity(
        "trait Leaf {}\ntrait Left {}\ntrait Right {}\ntrait Root {}\nimpl Leaf for i64 {}\nimpl[T: Leaf] Left for T {}\nimpl[T: Leaf] Right for T {}\nimpl[T: Left + Right] Root for T {}\nfn direct() -> Unit = ()\n",
        |fixture, arena, recursive| {
            let owner = fixture.declaration("direct");
            let root = obligation(fixture, "Root", 0);
            let selected = select(fixture, owner, &root);
            let result = run(fixture, arena, owner, root, selected, recursive);
            assert_eq!(result, Ok(id(3)));
            for index in [1, 2] {
                let EvidenceSource::Implementation {
                    implementation_bounds,
                    ..
                } = &arena.nodes[index]
                else {
                    panic!("implementation expected")
                };
                assert_eq!(implementation_bounds.as_ref(), [id(0)]);
            }
            let EvidenceSource::Implementation {
                implementation_bounds,
                ..
            } = &arena.nodes[3]
            else {
                panic!("implementation expected")
            };
            assert_eq!(implementation_bounds.as_ref(), [id(1), id(2)]);
            result
        },
    );
}

#[test]
fn closed_evidence_is_shared_but_obligation_caches_remain_owner_scoped() {
    parity(
        "trait Leaf {}\nimpl Leaf for i64 {}\nfn first() -> Unit = ()\nfn second() -> Unit = ()\n",
        |fixture, arena, recursive| {
            let mut result = Ok(id(0));
            for name in ["first", "second"] {
                let owner = fixture.declaration(name);
                let root = obligation(fixture, "Leaf", 0);
                let selected = select(fixture, owner, &root);
                result = run(fixture, arena, owner, root, selected, recursive);
                assert_eq!(result, Ok(id(0)));
            }
            assert_eq!(arena.nodes.len(), 1);
            assert_eq!(arena.by_obligation.len(), 2);
            result
        },
    );
}
