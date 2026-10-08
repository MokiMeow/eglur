use std::fmt::Write;

use super::super::*;
use crate::{DefId, SubstId, TypeStoreError};

mod fixture;
mod recursive;

use fixture::{declaration, root_proof, warm_chain, with_hasher};

const LEAF_SOURCE: &str = "trait Leaf {}\nimpl Leaf for i64 {}\nfn consume[T: Leaf](value: T) -> Unit = ()\nfn direct() -> Unit = consume(0)\n";

fn id(index: usize) -> EvidenceProofId {
    EvidenceProofId::from_index(index).unwrap()
}

fn node(template: &EvidenceSource, implementation: &[usize], traits: &[usize]) -> EvidenceSource {
    let EvidenceSource::Implementation {
        implementation: target,
        substitution,
        ..
    } = template
    else {
        panic!("source template is an implementation")
    };
    EvidenceSource::Implementation {
        implementation: target.clone(),
        substitution: *substitution,
        implementation_bounds: implementation.iter().copied().map(id).collect(),
        trait_bounds: traits.iter().copied().map(id).collect(),
    }
}

fn injected<R>(
    build: impl FnOnce(
        &EvidenceSource,
        &DeclarationCollection,
        &DeclarationSignatures,
    ) -> Vec<EvidenceSource>,
    inspect: impl FnOnce(&mut NodeHasher<'_, '_>, &DeclarationCollection) -> R,
) -> R {
    with_hasher(
        LEAF_SOURCE,
        "direct",
        |_, _, _, _| {},
        |evidence, declarations, signatures| {
            let template = evidence.proof(id(0)).unwrap().clone();
            evidence.replace_proofs_for_test(build(&template, declarations, signatures));
        },
        inspect,
    )
}

fn chain_source(depth: usize) -> String {
    assert!(depth > 0);
    let mut source = String::from("trait Hop0 {}\nimpl Hop0 for i64 {}\n");
    for level in 1..depth {
        writeln!(
            source,
            "trait Hop{level} {{}}\nimpl[T: Hop{}] Hop{level} for T {{}}",
            level - 1
        )
        .unwrap();
    }
    writeln!(
        source,
        "fn consume[T: Hop{}](value: T) -> Unit = ()\nfn direct() -> Unit = consume(0)",
        depth - 1
    )
    .unwrap();
    source
}

fn compare(
    hasher: &mut NodeHasher<'_, '_>,
    root: EvidenceProofId,
) -> Result<FingerprintDigest, SemanticFingerprintError> {
    let original_proofs = hasher.proofs.clone();
    let original_active = hasher.active_proofs.clone();
    let expected = hasher.recursive_stable_proof(root);
    let expected_proofs = std::mem::replace(&mut hasher.proofs, original_proofs);
    let expected_active = std::mem::replace(&mut hasher.active_proofs, original_active);
    let actual = hasher.stable_proof(root);
    assert_eq!(actual, expected);
    assert_eq!(hasher.proofs, expected_proofs);
    assert_eq!(hasher.active_proofs, expected_active);
    actual
}

#[test]
fn source_completed_chains_match_the_recursive_digest_and_cache_oracle() {
    for depth in [1, 2, 8, 24] {
        with_hasher(
            &chain_source(depth),
            "direct",
            |declarations, signatures, environment, owner| {
                warm_chain(declarations, signatures, environment, owner, depth);
            },
            |_, _, _| {},
            |hasher, declarations| {
                let root = root_proof(hasher, declarations, &format!("Hop{}", depth - 1));
                compare(hasher, root).unwrap();
                assert_eq!(hasher.proofs.len(), depth);
                assert!(hasher.active_proofs.is_empty());
            },
        );
    }
}

#[test]
fn deep_source_completed_proof_dag_hashes_without_native_recursion() {
    let depth = 4096;
    with_hasher(
        &chain_source(depth),
        "direct",
        |declarations, signatures, environment, owner| {
            warm_chain(declarations, signatures, environment, owner, depth);
        },
        |_, _, _| {},
        |hasher, declarations| {
            let root = root_proof(hasher, declarations, &format!("Hop{}", depth - 1));
            hasher.stable_proof(root).unwrap();
            assert_eq!(hasher.proofs.len(), depth);
            assert!(hasher.active_proofs.is_empty());
        },
    );
}

#[test]
fn source_shared_diamond_reuses_nodes_without_suppressing_edges() {
    with_hasher(
        "trait Leaf {}\ntrait Left {}\ntrait Right {}\ntrait Root {}\nimpl Leaf for i64 {}\nimpl[T: Leaf] Left for T {}\nimpl[T: Leaf] Right for T {}\nimpl[T: Left + Right] Root for T {}\nfn consume[T: Root](value: T) -> Unit = ()\nfn direct() -> Unit = consume(0)\n",
        "direct",
        |_, _, _, _| {},
        |_, _, _| {},
        |hasher, declarations| {
            let root = root_proof(hasher, declarations, "Root");
            compare(hasher, root).unwrap();
            assert_eq!(hasher.proofs.len(), 4);
            assert!(hasher.active_proofs.is_empty());
        },
    );
}

#[test]
fn source_staged_bounds_preserve_both_family_encodings() {
    for parameters in ["T: Left, A: Right", "A: Right, T: Left"] {
        let text = format!(
            "trait Left {{}}\ntrait Right {{}}\nimpl Left for i64 {{}}\nimpl Right for i64 {{}}\ntrait Task[A: Right] {{}}\nimpl[{parameters}] Task[A] for T {{}}\nfn consume[T: Task[A], A: Right](value: T, argument: A) -> Unit = ()\nfn direct() -> Unit = consume(0, 0)\n"
        );
        with_hasher(
            &text,
            "direct",
            |_, _, _, _| {},
            |_, _, _| {},
            |hasher, declarations| {
                let root = root_proof(hasher, declarations, "Task");
                let EvidenceSource::Implementation {
                    implementation_bounds,
                    trait_bounds,
                    ..
                } = hasher.trait_evidence.proof(root).unwrap()
                else {
                    panic!("implementation expected")
                };
                assert_eq!(implementation_bounds.len(), 2);
                assert_eq!(trait_bounds.len(), 1);
                compare(hasher, root).unwrap();
                assert_eq!(hasher.proofs.len(), 3);
                assert!(hasher.active_proofs.is_empty());
            },
        );
    }
}

#[test]
fn source_wide_bounds_retain_repeated_shared_digests() {
    for width in [1, 8, 64] {
        let names = (0..width)
            .map(|index| format!("A{index}"))
            .collect::<Vec<_>>();
        let parameters = names
            .iter()
            .map(|name| format!("{name}: Leaf"))
            .collect::<Vec<_>>()
            .join(", ");
        let values = names
            .iter()
            .enumerate()
            .map(|(index, name)| format!("x{index}: {name}"))
            .collect::<Vec<_>>()
            .join(", ");
        let text = format!(
            "trait Leaf {{}}\nimpl Leaf for i64 {{}}\ntrait Wide[{}] {{}}\nimpl[T: Leaf, {parameters}] Wide[{}] for T {{}}\nfn consume[T: Wide[{}], {parameters}](value: T, {values}) -> Unit = ()\nfn direct() -> Unit = consume({})\n",
            names.join(", "),
            names.join(", "),
            names.join(", "),
            vec!["0"; width + 1].join(", ")
        );
        with_hasher(
            &text,
            "direct",
            |_, _, _, _| {},
            |_, _, _| {},
            |hasher, declarations| {
                let root = root_proof(hasher, declarations, "Wide");
                let EvidenceSource::Implementation {
                    implementation_bounds,
                    ..
                } = hasher.trait_evidence.proof(root).unwrap()
                else {
                    panic!("implementation expected")
                };
                assert_eq!(implementation_bounds.as_ref(), vec![id(0); width + 1]);
                compare(hasher, root).unwrap();
                assert_eq!(hasher.proofs.len(), 2);
            },
        );
    }
}

#[test]
fn source_witness_uses_its_owner_binders_not_the_current_body_binders() {
    with_hasher(
        "trait Leaf {}\nfn consume[T: Leaf](value: T) -> Unit = ()\nfn owner[A, Value: Leaf](unused: A, value: Value) -> Unit = consume(value)\nfn direct() -> Unit = ()\n",
        "direct",
        |_, _, _, _| {},
        |_, _, _| {},
        |hasher, declarations| {
            let root = root_proof(hasher, declarations, "Leaf");
            let EvidenceSource::Bound(witness) = hasher.trait_evidence.proof(root).unwrap() else {
                panic!("bound witness expected")
            };
            assert_eq!(witness.owner(), declaration(declarations, "owner"));
            assert_ne!(witness.owner(), declaration(declarations, "direct"));
            compare(hasher, root).unwrap();
            assert_eq!(hasher.proofs.len(), 1);
        },
    );
}

#[test]
fn source_generic_impl_substitution_retains_body_owner_ordinals() {
    for prefix in ["", "Unused, ", "First, Second, "] {
        let unused = match prefix {
            "" => "",
            "Unused, " => "unused: Unused, ",
            _ => "first: First, second: Second, ",
        };
        let text = format!(
            "trait Leaf {{}}\ntrait Root {{}}\nstruct Wrap[T] {{}}\nimpl[T: Leaf] Root for Wrap[T] {{}}\nfn consume[T: Root](value: T) -> Unit = ()\nfn owner[{prefix}Value: Leaf]({unused}value: Wrap[Value]) -> Unit = consume(value)\n"
        );
        with_hasher(
            &text,
            "owner",
            |_, _, _, _| {},
            |_, _, _| {},
            |hasher, declarations| {
                let root = root_proof(hasher, declarations, "Root");
                compare(hasher, root).unwrap();
                assert_eq!(hasher.proofs.len(), 2);
                assert!(hasher.active_proofs.is_empty());
            },
        );
    }
}

#[test]
fn injected_family_order_is_normalized_but_boundaries_and_multiplicity_are_not() {
    let shapes = [
        (vec![0, 1], vec![0]),
        (vec![1, 0], vec![0]),
        (vec![0], vec![0, 1]),
        (vec![0, 0], vec![]),
        (vec![0], vec![]),
        (vec![], vec![0]),
        (vec![], vec![]),
    ];
    let mut digests = Vec::new();
    for (implementation, traits) in shapes {
        digests.push(injected(
            |template, _, _| {
                vec![
                    template.clone(),
                    node(template, &[0], &[]),
                    node(template, &implementation, &traits),
                ]
            },
            |hasher, _| {
                let digest = compare(hasher, id(2)).unwrap();
                assert!(hasher.active_proofs.is_empty());
                digest
            },
        ));
    }
    assert_eq!(digests[0], digests[1]);
    assert_ne!(digests[0], digests[2]);
    assert_ne!(digests[3], digests[4]);
    assert_ne!(digests[4], digests[5]);
    assert_ne!(digests[4], digests[6]);
}

#[test]
fn injected_unknown_proof_is_active_and_a_retry_is_cyclic() {
    injected(
        |_, _, _| vec![],
        |hasher, _| {
            assert_eq!(
                compare(hasher, id(0)),
                Err(SemanticFingerprintError::UnknownEvidenceProof)
            );
            assert_eq!(hasher.active_proofs, BTreeSet::from([id(0)]));
            assert!(hasher.proofs.is_empty());
            assert_eq!(
                compare(hasher, id(0)),
                Err(SemanticFingerprintError::CyclicEvidenceProof)
            );
        },
    );
}

#[test]
fn completed_cache_precedes_active_and_does_not_clean_the_active_marker() {
    injected(
        |template, _, _| vec![template.clone()],
        |hasher, _| {
            let first = compare(hasher, id(0)).unwrap();
            hasher.active_proofs.insert(id(0));
            assert_eq!(compare(hasher, id(0)), Ok(first));
            assert_eq!(hasher.active_proofs, BTreeSet::from([id(0)]));
        },
    );
}

#[test]
fn injected_self_and_mutual_cycles_never_cache_unfinished_nodes() {
    for mutual in [false, true] {
        injected(
            |template, _, _| {
                if mutual {
                    vec![node(template, &[1], &[]), node(template, &[], &[0])]
                } else {
                    vec![node(template, &[0], &[])]
                }
            },
            |hasher, _| {
                assert_eq!(
                    compare(hasher, id(0)),
                    Err(SemanticFingerprintError::CyclicEvidenceProof)
                );
                assert!(hasher.proofs.is_empty());
                let expected = if mutual {
                    BTreeSet::from([id(0), id(1)])
                } else {
                    BTreeSet::from([id(0)])
                };
                assert_eq!(hasher.active_proofs, expected);
            },
        );
    }
}

#[test]
fn injected_metadata_failure_precedes_all_child_visits() {
    for missing_target in [true, false] {
        injected(
            |template, declarations, signatures| {
                let mut root = node(template, &[0], &[99]);
                let EvidenceSource::Implementation {
                    implementation,
                    substitution,
                    ..
                } = &mut root
                else {
                    unreachable!()
                };
                if missing_target {
                    *implementation = CanonicalTarget::Source(
                        DefId::from_index(declaration(declarations, "direct").module(), 100_000)
                            .unwrap(),
                    );
                }
                *substitution =
                    SubstId::from_index(signatures.types().substitution_count() + 10).unwrap();
                vec![template.clone(), root]
            },
            |hasher, declarations| {
                let error = if missing_target {
                    SemanticFingerprintError::UnknownStableDeclaration(
                        DefId::from_index(declaration(declarations, "direct").module(), 100_000)
                            .unwrap(),
                    )
                } else {
                    SemanticFingerprintError::TypeStore(TypeStoreError::UnknownSubstitution(
                        SubstId::from_index(hasher.signatures.types().substitution_count() + 10)
                            .unwrap(),
                    ))
                };
                assert_eq!(compare(hasher, id(1)), Err(error));
                assert_eq!(hasher.active_proofs, BTreeSet::from([id(1)]));
                assert!(hasher.proofs.is_empty());
            },
        );
    }
}

#[test]
fn injected_first_descendant_failure_preserves_only_the_completed_prefix() {
    injected(
        |template, _, _| {
            vec![
                template.clone(),
                node(template, &[99], &[]),
                node(template, &[0], &[1, 100]),
                node(template, &[2, 101], &[102]),
            ]
        },
        |hasher, _| {
            assert_eq!(
                compare(hasher, id(3)),
                Err(SemanticFingerprintError::UnknownEvidenceProof)
            );
            assert_eq!(hasher.proofs.keys().copied().collect::<Vec<_>>(), [id(0)]);
            assert_eq!(
                hasher.active_proofs,
                BTreeSet::from([id(1), id(2), id(3), id(99)])
            );
        },
    );
}

#[test]
fn injected_unknown_witness_precedes_owner_metadata_lookup() {
    injected(
        |_, declarations, _| {
            let owner =
                DefId::from_index(declaration(declarations, "direct").module(), 100_000).unwrap();
            vec![EvidenceSource::Bound(
                WitnessSlotId::from_index(owner, 0).unwrap(),
            )]
        },
        |hasher, _| {
            assert_eq!(
                compare(hasher, id(0)),
                Err(SemanticFingerprintError::UnknownBoundWitness)
            );
            assert_eq!(hasher.active_proofs, BTreeSet::from([id(0)]));
            assert!(hasher.proofs.is_empty());
        },
    );
}
