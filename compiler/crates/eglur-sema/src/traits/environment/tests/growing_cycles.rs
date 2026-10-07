use super::*;
use crate::{IntrinsicNominal, NominalHead};

const GROWING: &str = concat!(
    "trait Loop[A] {}\n",
    "impl[A, T: Loop[Opt[A]]] Loop[A] for T {}\n",
    "fn direct() -> Unit = ()\n",
    "fn bounded[X: Loop[Opt[Opt[i64]]]](x: X) -> X = x\n",
    "fn unrelated[X: Loop[Bool]](x: X) -> X = x\n",
);

fn required(
    fixture: &mut Fixture,
    subject: TypeId,
    name: &str,
    argument: TypeId,
) -> TraitObligation {
    let target = CanonicalTarget::Source(fixture.declaration(name));
    let substitution = fixture
        .signatures
        .types_mut()
        .intern_substitution([argument])
        .unwrap();
    TraitObligation::new(subject, TraitRef::new(target, substitution))
}

fn integer(fixture: &mut Fixture) -> TypeId {
    fixture
        .signatures
        .types_mut()
        .primitive(PrimitiveType::I64)
        .unwrap()
}

fn opt(fixture: &mut Fixture, inner: TypeId) -> TypeId {
    fixture
        .signatures
        .types_mut()
        .nominal(NominalHead::Intrinsic(IntrinsicNominal::Opt), [inner])
        .unwrap()
}

#[test]
fn growing_cycle_rejects_the_requested_root_without_generating_wrappers() {
    let mut fixture = fixture_from_source(GROWING);
    let owner = fixture.declaration("direct");
    let integer = integer(&mut fixture);
    let root = required(&mut fixture, integer, "Loop", integer);
    let facts = fixture.environment.fact_count();
    let types = fixture.signatures.types().type_count();
    for _ in 0..2 {
        assert!(matches!(
            fixture
                .environment
                .prove_for_owner(owner, &mut fixture.signatures, root.clone())
                .unwrap(),
            TraitProof::Unsatisfied(found) if found == root
        ));
    }
    assert_eq!(fixture.environment.fact_count(), facts);
    assert_eq!(fixture.signatures.types().type_count(), types);
    assert_eq!(fixture.environment.selection_cache.len(), 1);
    assert!(fixture.diagnostics.finalize(None).diagnostics().is_empty());
}

#[test]
fn impl_argument_violations_take_priority_over_cycle_failure() {
    let mut fixture = fixture_from_source(GROWING);
    let owner = fixture.declaration("direct");
    let integer = integer(&mut fixture);
    let ownership = fixture
        .signatures
        .types_mut()
        .nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [integer])
        .unwrap();
    let reference = fixture
        .signatures
        .types_mut()
        .reference(crate::ReferenceMutability::Shared, integer)
        .unwrap();
    for (subject, argument, violation) in [
        (
            integer,
            ownership,
            ImplArgumentViolation::Ownership(ownership),
        ),
        (
            integer,
            reference,
            ImplArgumentViolation::Reference(reference),
        ),
        (
            reference,
            ownership,
            ImplArgumentViolation::Ownership(ownership),
        ),
    ] {
        let root = required(&mut fixture, subject, "Loop", argument);
        assert!(
            matches!(fixture.environment.prove_for_owner(owner, &mut fixture.signatures, root).unwrap(), TraitProof::InvalidArgument(found) if found == violation)
        );
    }
}

#[test]
fn multiple_preserved_argument_slots_share_a_productive_cycle() {
    let mut fixture = fixture_from_source(concat!(
        "trait Loop[A, B] {}\n",
        "impl[A, B, T: Loop[A, Opt[Opt[B]]]] Loop[A, B] for T {}\n",
        "fn direct() -> Unit = ()\n",
    ));
    let owner = fixture.declaration("direct");
    let integer = integer(&mut fixture);
    let boolean = fixture
        .signatures
        .types_mut()
        .primitive(PrimitiveType::Bool)
        .unwrap();
    let target = CanonicalTarget::Source(fixture.declaration("Loop"));
    let arguments = fixture
        .signatures
        .types_mut()
        .intern_substitution([integer, boolean])
        .unwrap();
    let root = TraitObligation::new(integer, TraitRef::new(target, arguments));
    assert!(
        matches!(fixture.environment.prove_for_owner(owner, &mut fixture.signatures, root.clone()).unwrap(), TraitProof::Unsatisfied(found) if found == root)
    );
}

#[test]
fn future_exact_assumptions_are_not_cut_and_owner_guards_are_isolated() {
    let mut fixture = fixture_from_source(GROWING);
    let owner = fixture.declaration("bounded");
    let unrelated = fixture.declaration("unrelated");
    let assumption = fixture.environment.assumptions[&owner][0].clone();
    let unrelated_assumption = fixture.environment.assumptions[&unrelated][0].clone();
    let integer = integer(&mut fixture);
    let root = required(
        &mut fixture,
        assumption.obligation.subject(),
        "Loop",
        integer,
    );
    let unrelated_root = required(
        &mut fixture,
        unrelated_assumption.obligation.subject(),
        "Loop",
        integer,
    );
    for _ in 0..2 {
        assert!(matches!(
            fixture
                .environment
                .prove_for_owner(owner, &mut fixture.signatures, root.clone())
                .unwrap(),
            TraitProof::Implementation(_)
        ));
        assert!(matches!(
            fixture
                .environment
                .prove_for_owner(
                    owner,
                    &mut fixture.signatures,
                    assumption.obligation.clone()
                )
                .unwrap(),
            TraitProof::Bound(_)
        ));
        // Do not solve the unclassified divergent path: a same-subject assumption
        // conservatively disables the certificate even when it is unreachable.
        let resolved = fixture
            .environment
            .normalized_owner_assumptions(unrelated, &mut fixture.signatures)
            .unwrap()
            .unwrap();
        assert!(
            !fixture
                .environment
                .is_unprovable_growing_cycle(
                    Some(unrelated),
                    &fixture.signatures,
                    &unrelated_root,
                    &resolved
                )
                .unwrap()
        );
        assert!(
            fixture
                .environment
                .is_unprovable_growing_cycle(
                    Some(owner),
                    &fixture.signatures,
                    &unrelated_root,
                    std::slice::from_ref(&assumption)
                )
                .unwrap()
        );
    }
}

#[test]
fn local_query_guards_reset_with_their_assumption_environment() {
    let mut fixture = fixture_from_source(GROWING);
    let integer = integer(&mut fixture);
    let one = opt(&mut fixture, integer);
    let two = opt(&mut fixture, one);
    let root = required(&mut fixture, integer, "Loop", integer);
    let future = required(&mut fixture, integer, "Loop", two);
    let assumptions = [TraitAssumption {
        obligation: future,
        evidence: TraitAssumptionEvidence::Trusted,
    }];

    fixture.environment.local_query_cache = Some(LocalQueryCache::default());
    assert!(matches!(
        fixture
            .environment
            .select_impl(
                None,
                &mut fixture.signatures,
                root.clone(),
                &assumptions,
                &mut BTreeSet::new()
            )
            .unwrap(),
        SelectionOutcome::Selected(_)
    ));

    fixture.environment.local_query_cache = Some(LocalQueryCache::default());
    assert!(
        matches!(fixture.environment.select_impl(None, &mut fixture.signatures, root.clone(), &[], &mut BTreeSet::new()).unwrap(), SelectionOutcome::Unsatisfied(found) if found == root)
    );

    fixture.environment.local_query_cache = Some(LocalQueryCache::default());
    assert!(matches!(
        fixture
            .environment
            .select_impl(
                None,
                &mut fixture.signatures,
                root,
                &assumptions,
                &mut BTreeSet::new()
            )
            .unwrap(),
        SelectionOutcome::Selected(_)
    ));
}

#[test]
fn poisoning_a_later_cycle_member_invalidates_cached_failure_certificates() {
    let mut fixture = fixture_from_source(concat!(
        "trait First[A] {}\ntrait Second[A] {}\n",
        "impl[A, T: Second[Opt[A]]] First[A] for T {}\n",
        "impl[A, T: First[A]] Second[A] for T {}\n",
        "fn direct() -> Unit = ()\n",
    ));
    let owner = fixture.declaration("direct");
    let integer = integer(&mut fixture);
    let root = required(&mut fixture, integer, "First", integer);
    assert!(matches!(
        fixture
            .environment
            .prove_for_owner(owner, &mut fixture.signatures, root.clone())
            .unwrap(),
        TraitProof::Unsatisfied(found) if found == root
    ));
    let second = CanonicalTarget::Source(fixture.declaration("Second"));
    let implementation = fixture
        .environment
        .implementations
        .iter()
        .find(|(_, implementation)| implementation.header.trait_ref().declaration() == &second)
        .unwrap()
        .0
        .clone();
    let mut fresh = TraitEnvironment::from_checked_parts(
        fixture.environment.implementations.clone(),
        fixture.environment.buckets.clone(),
        fixture.environment.assumptions.clone(),
    );
    let poison = crate::diagnostic::record_name_not_found(
        &mut fixture.diagnostics,
        fixture.span,
        "invalidated_cycle",
    )
    .unwrap();
    fixture
        .environment
        .poison_implementation(&implementation, poison.clone());
    fresh.poison_implementation(&implementation, poison);
    for environment in [&mut fixture.environment, &mut fresh] {
        assert!(matches!(
            environment
                .prove_for_owner(owner, &mut fixture.signatures, root.clone())
                .unwrap(),
            TraitProof::Poisoned(_)
        ));
    }
}

#[test]
fn unsupported_templates_do_not_produce_negative_certificates() {
    let cases = [
        "trait Loop[A] {}\nimpl[A, T: Loop[A]] Loop[A] for T {}\n",
        "trait Loop[A] {}\nimpl[A: Loop[A], T] Loop[A] for T {}\n",
        "trait Loop[A] {}\nimpl[T: Loop[Opt[T]]] Loop[T] for T {}\n",
        "trait Loop[A, B] {}\nimpl[A, B, T: Loop[Opt[B], A]] Loop[A, B] for T {}\n",
        "trait Loop[A, B] {}\nimpl[A, B, T: Loop[Opt[A], A]] Loop[A, B] for T {}\n",
        "trait Loop[A] {}\nimpl[A, T: Loop[Opt[A]]] Loop[A] for Opt[T] {}\n",
        "trait Other {}\ntrait Loop[A] {}\nimpl[A, T: Loop[Opt[A]] + Other] Loop[A] for T {}\n",
        "trait Other {}\nimpl[A: Other] Other for Opt[A] {}\ntrait Loop[A: Other] {}\nimpl[A: Other, T: Loop[Opt[A]]] Loop[A] for T {}\n",
        "trait Loop[A] { type Item }\nimpl[A, T: Loop[Opt[A]]] Loop[A] for T { type Item = i64 }\n",
        "trait Loop[A] {}\nimpl[A, T: Loop[Bool]] Loop[A] for T {}\n",
        "trait Loop[A] {}\nimpl[A, T: Loop[Opt[A]]] Loop[Opt[A]] for T {}\n",
        "trait Loop[A] {}\nimpl[A, T: Loop[Opt[A]]] Loop[A] for T {}\nimpl[A] Loop[A] for i64 {}\n",
    ];
    for source in cases {
        let mut fixture = fixture_from_source(&format!("{source}\nfn direct() -> Unit = ()\n"));
        let integer = integer(&mut fixture);
        let target = CanonicalTarget::Source(fixture.declaration("Loop"));
        let arity = match fixture.signatures.declaration(&target) {
            FactState::Known(DeclarationSignature::Trait {
                generic_parameters, ..
            }) => generic_parameters.len(),
            _ => panic!("missing trait: {source}"),
        };
        let substitution = fixture
            .signatures
            .types_mut()
            .intern_substitution(vec![integer; arity])
            .unwrap();
        let root = TraitObligation::new(integer, TraitRef::new(target, substitution));
        assert!(
            !fixture
                .environment
                .is_unprovable_growing_cycle(None, &fixture.signatures, &root, &[])
                .unwrap(),
            "{source}"
        );
    }
}
