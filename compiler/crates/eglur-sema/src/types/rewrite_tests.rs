use super::tests::SUBSTITUTION_READS;
use super::*;

fn parameter(owner: &str) -> GenericParameterId {
    GenericParameterId::Trusted {
        owner: ExternalDefId::new(owner).unwrap(),
        index: 0,
    }
}

fn target(key: &str) -> CanonicalTarget {
    CanonicalTarget::External(ExternalDefId::new(key).unwrap())
}

fn copy_store(store: &CanonicalTypeStore) -> CanonicalTypeStore {
    CanonicalTypeStore {
        never: store.never,
        types: store.types.clone(),
        may_need_generic_substitution: store.may_need_generic_substitution.clone(),
        ownership_presence: store.ownership_presence.clone(),
        type_ids: store.type_ids.clone(),
        substitutions: store.substitutions.clone(),
        substitution_ownership_presence: store.substitution_ownership_presence.clone(),
        substitution_ids: store.substitution_ids.clone(),
        nominal_arities: store.nominal_arities.clone(),
    }
}

fn assert_store_parity(actual: &CanonicalTypeStore, expected: &CanonicalTypeStore) {
    assert_eq!(actual.never, expected.never);
    assert_eq!(actual.types, expected.types);
    assert_eq!(
        actual.may_need_generic_substitution,
        expected.may_need_generic_substitution
    );
    assert_eq!(actual.type_ids, expected.type_ids);
    assert_eq!(actual.ownership_presence, expected.ownership_presence);
    assert_eq!(
        actual.substitution_ownership_presence,
        expected.substitution_ownership_presence
    );
    assert_eq!(actual.substitutions, expected.substitutions);
    assert_eq!(actual.substitution_ids, expected.substitution_ids);
    assert_eq!(actual.nominal_arities, expected.nominal_arities);
}

// Bounded recursive oracles independently check traversal and admission order.
impl CanonicalTypeStore {
    fn recursive_substitute(
        &mut self,
        root: TypeId,
        replacements: &BTreeMap<GenericParameterId, TypeId>,
        memo: &mut BTreeMap<TypeId, TypeId>,
    ) -> Result<TypeId, TypeStoreError> {
        if let Some(&resolved) = memo.get(&root) {
            return Ok(resolved);
        }
        let kind = self.kind(root)?.clone();
        let resolved = match kind {
            TypeKind::Never | TypeKind::Primitive(_) | TypeKind::SelfType(_) => root,
            TypeKind::GenericParameter(parameter) => {
                replacements.get(&parameter).copied().unwrap_or(root)
            }
            TypeKind::Nominal { head, substitution } => {
                let arguments = self.substitution(substitution)?.to_vec();
                let arguments = self.recursive_substitute_many(&arguments, replacements, memo)?;
                self.nominal(head, arguments)?
            }
            TypeKind::Reference {
                mutability,
                referent,
            } => {
                let referent = self.recursive_substitute(referent, replacements, memo)?;
                self.reference(mutability, referent)?
            }
            TypeKind::RawPointer {
                mutability,
                referent,
            } => {
                let referent = self.recursive_substitute(referent, replacements, memo)?;
                self.raw_pointer(mutability, referent)?
            }
            TypeKind::Tuple(elements) => {
                let elements = self.recursive_substitute_many(&elements, replacements, memo)?;
                self.tuple(elements)?
            }
            TypeKind::Callable { parameters, result } => {
                let parameters = self.recursive_substitute_many(&parameters, replacements, memo)?;
                let result = self.recursive_substitute(result, replacements, memo)?;
                self.callable(parameters, result)?
            }
            TypeKind::ErrorUnion(members) => {
                let members = self.recursive_substitute_many(&members, replacements, memo)?;
                self.error_union(members)?
            }
            TypeKind::AssociatedProjection(projection) => {
                let base = self.recursive_substitute(projection.base, replacements, memo)?;
                let arguments = self
                    .substitution(projection.trait_ref.substitution)?
                    .to_vec();
                let arguments = self.recursive_substitute_many(&arguments, replacements, memo)?;
                let substitution = self.intern_substitution(arguments)?;
                self.associated_projection(AssociatedProjection::new(
                    base,
                    TraitRef::new(projection.trait_ref.declaration, substitution),
                    projection.associated,
                ))?
            }
        };
        memo.insert(root, resolved);
        Ok(resolved)
    }

    fn recursive_substitute_many(
        &mut self,
        values: &[TypeId],
        replacements: &BTreeMap<GenericParameterId, TypeId>,
        memo: &mut BTreeMap<TypeId, TypeId>,
    ) -> Result<Vec<TypeId>, TypeStoreError> {
        values
            .iter()
            .map(|&value| self.recursive_substitute(value, replacements, memo))
            .collect()
    }

    fn recursive_replace_self(
        &mut self,
        root: TypeId,
        owner: &CanonicalTarget,
        replacement: TypeId,
        memo: &mut BTreeMap<TypeId, TypeId>,
    ) -> Result<TypeId, TypeStoreError> {
        if let Some(&resolved) = memo.get(&root) {
            return Ok(resolved);
        }
        let kind = self.kind(root)?.clone();
        let resolved = match kind {
            TypeKind::SelfType(found) if &found == owner => replacement,
            TypeKind::Nominal { head, substitution } => {
                let arguments = self.substitution(substitution)?.to_vec();
                let arguments = arguments
                    .into_iter()
                    .map(|argument| self.recursive_replace_self(argument, owner, replacement, memo))
                    .collect::<Result<Vec<_>, _>>()?;
                self.nominal(head, arguments)?
            }
            TypeKind::Reference {
                mutability,
                referent,
            } => {
                let referent = self.recursive_replace_self(referent, owner, replacement, memo)?;
                self.reference(mutability, referent)?
            }
            TypeKind::RawPointer {
                mutability,
                referent,
            } => {
                let referent = self.recursive_replace_self(referent, owner, replacement, memo)?;
                self.raw_pointer(mutability, referent)?
            }
            TypeKind::Tuple(elements) => {
                let elements = elements
                    .iter()
                    .map(|&element| self.recursive_replace_self(element, owner, replacement, memo))
                    .collect::<Result<Vec<_>, _>>()?;
                self.tuple(elements)?
            }
            TypeKind::Callable { parameters, result } => {
                let parameters = parameters
                    .iter()
                    .map(|&parameter| {
                        self.recursive_replace_self(parameter, owner, replacement, memo)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let result = self.recursive_replace_self(result, owner, replacement, memo)?;
                self.callable(parameters, result)?
            }
            TypeKind::ErrorUnion(members) => {
                let members = members
                    .iter()
                    .map(|&member| self.recursive_replace_self(member, owner, replacement, memo))
                    .collect::<Result<Vec<_>, _>>()?;
                self.error_union(members)?
            }
            TypeKind::AssociatedProjection(projection) => {
                let base =
                    self.recursive_replace_self(projection.base, owner, replacement, memo)?;
                let arguments = self
                    .substitution(projection.trait_ref.substitution)?
                    .to_vec();
                let arguments = arguments
                    .into_iter()
                    .map(|argument| self.recursive_replace_self(argument, owner, replacement, memo))
                    .collect::<Result<Vec<_>, _>>()?;
                let substitution = self.intern_substitution(arguments)?;
                self.associated_projection(AssociatedProjection::new(
                    base,
                    TraitRef::new(projection.trait_ref.declaration, substitution),
                    projection.associated,
                ))?
            }
            TypeKind::Never
            | TypeKind::Primitive(_)
            | TypeKind::GenericParameter(_)
            | TypeKind::SelfType(_) => root,
        };
        memo.insert(root, resolved);
        Ok(resolved)
    }
}

#[test]
fn nonempty_substitution_matches_the_recursive_body_on_shared_graphs() {
    for a in 0..4 {
        for b in 0..9 {
            for c in 0..16 {
                let mut store = CanonicalTypeStore::new().unwrap();
                let integer = store.primitive(PrimitiveType::I64).unwrap();
                let text = store.primitive(PrimitiveType::Str).unwrap();
                let p = parameter("std::P");
                let q = parameter("std::Q");
                let p_type = store.generic_parameter(p.clone()).unwrap();
                let q_type = store.generic_parameter(q.clone()).unwrap();
                let self_type = store.self_type(target("std::Node")).unwrap();
                let closed = store.tuple([integer, text]).unwrap();
                let owned = store
                    .nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [p_type])
                    .unwrap();
                let reference = store
                    .reference(ReferenceMutability::Mutable, q_type)
                    .unwrap();
                let pointer = store
                    .raw_pointer(RawPointerMutability::Const, p_type)
                    .unwrap();
                let arguments = store
                    .intern_substitution([pointer, reference, q_type])
                    .unwrap();
                let projection = store
                    .associated_projection(AssociatedProjection::new(
                        integer,
                        TraitRef::new(target("std::Linked"), arguments),
                        target("std::Linked::Item"),
                    ))
                    .unwrap();
                let callable = store
                    .callable([owned, reference, projection], self_type)
                    .unwrap();
                let union = store.error_union([callable, integer, text]).unwrap();
                let all = store
                    .tuple([p_type, q_type, pointer, union, closed])
                    .unwrap();
                let mut choices = vec![closed, all];
                for (choice, width) in [(a, 2), (b, 3), (c, 4)] {
                    let next = store
                        .tuple([choices[choice / width], choices[choice % width]])
                        .unwrap();
                    choices.push(next);
                }
                let mut roots = vec![
                    store.never(),
                    integer,
                    text,
                    p_type,
                    q_type,
                    self_type,
                    closed,
                    owned,
                    reference,
                    pointer,
                    projection,
                    callable,
                    union,
                    all,
                ];
                roots.extend(choices);

                for replacements in [
                    BTreeMap::from([(p.clone(), integer)]),
                    BTreeMap::from([(p.clone(), closed)]),
                    BTreeMap::from([(q.clone(), text)]),
                    BTreeMap::from([(p.clone(), integer), (q.clone(), text)]),
                    BTreeMap::from([(parameter("std::Unused"), closed)]),
                    BTreeMap::from([(p.clone(), q_type), (q.clone(), integer)]),
                    BTreeMap::from([(p.clone(), store.never()), (q.clone(), text)]),
                ] {
                    for &root in &roots {
                        let mut expected_store = copy_store(&store);
                        let mut actual_store = copy_store(&store);
                        let expected = expected_store.recursive_substitute(
                            root,
                            &replacements,
                            &mut BTreeMap::new(),
                        );
                        assert_eq!(actual_store.substitute(root, &replacements), expected);
                        assert_store_parity(&actual_store, &expected_store);
                    }
                }

                for owner in [target("std::Node"), target("std::Other")] {
                    for replacement in [integer, closed, p_type, self_type] {
                        for &root in &roots {
                            let mut expected_store = copy_store(&store);
                            let mut actual_store = copy_store(&store);
                            let expected = expected_store.recursive_replace_self(
                                root,
                                &owner,
                                replacement,
                                &mut BTreeMap::new(),
                            );
                            assert_eq!(
                                actual_store.replace_self_type(root, &owner, replacement),
                                expected,
                            );
                            assert_store_parity(&actual_store, &expected_store);
                        }
                    }
                }
            }
        }
    }
}

fn layer(store: &mut CanonicalTypeStore, child: TypeId, scalar: TypeId, index: usize) -> TypeId {
    match index % 8 {
        0 => store.tuple([child, scalar, child]),
        1 => store.nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [child]),
        2 => store.reference(ReferenceMutability::Mutable, child),
        3 => store.raw_pointer(RawPointerMutability::Const, child),
        4 => store.callable([child, scalar], child),
        5 => {
            let substitution = store.intern_substitution([child, scalar]).unwrap();
            store.associated_projection(AssociatedProjection::new(
                child,
                TraitRef::new(target("std::Linked"), substitution),
                target("std::Linked::Item"),
            ))
        }
        6 => store.error_union([child, scalar]),
        7 => store.nominal(
            NominalHead::Intrinsic(IntrinsicNominal::Res),
            [child, scalar],
        ),
        _ => unreachable!(),
    }
    .unwrap()
}

#[test]
fn deep_generic_and_self_rewrites_preserve_postorder_without_recursion() {
    for depth in [64, 1024, 20_000] {
        for owned_self in [false, true] {
            let mut store = CanonicalTypeStore::new().unwrap();
            let scalar = store.primitive(PrimitiveType::I64).unwrap();
            let owner = target("std::Node");
            let p = parameter("std::P");
            let mut root = if owned_self {
                store.self_type(owner.clone()).unwrap()
            } else {
                store.generic_parameter(p.clone()).unwrap()
            };
            for index in 0..depth {
                root = layer(&mut store, root, scalar, index);
            }

            let mut expected_store = copy_store(&store);
            let mut expected = scalar;
            for index in 0..depth {
                expected = layer(&mut expected_store, expected, scalar, index);
            }
            let replacements = BTreeMap::from([(p, scalar)]);
            SUBSTITUTION_READS.set(0);
            let actual = if owned_self {
                store.replace_self_type(root, &owner, scalar)
            } else {
                store.substitute(root, &replacements)
            };
            assert_eq!(actual, Ok(expected));
            assert_store_parity(&store, &expected_store);
            if !owned_self {
                assert_eq!(SUBSTITUTION_READS.get(), depth + 2);
            }

            let repeated = if owned_self {
                store.replace_self_type(root, &owner, scalar)
            } else {
                store.substitute(root, &replacements)
            };
            assert_eq!(repeated, actual);
            assert_store_parity(&store, &expected_store);
        }
    }
}

#[test]
fn repeated_children_share_one_completed_rewrite() {
    for width in [8, 32, 128, 1024, 16_384] {
        let mut store = CanonicalTypeStore::new().unwrap();
        let scalar = store.primitive(PrimitiveType::I64).unwrap();
        let p = parameter("std::P");
        let mut shared = store.generic_parameter(p.clone()).unwrap();
        let depth = 48;
        for index in 0..depth {
            shared = layer(&mut store, shared, scalar, index);
        }
        let root = store.tuple(std::iter::repeat_n(shared, width)).unwrap();

        let mut expected_store = copy_store(&store);
        let replacements = BTreeMap::from([(p, scalar)]);
        let expected = expected_store
            .recursive_substitute(root, &replacements, &mut BTreeMap::new())
            .unwrap();
        SUBSTITUTION_READS.set(0);
        assert_eq!(store.substitute(root, &replacements), Ok(expected));
        assert_eq!(SUBSTITUTION_READS.get(), depth + 3);
        assert_store_parity(&store, &expected_store);
    }
}

#[test]
fn replacement_values_are_terminal_and_self_owners_remain_distinct() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let scalar = store.primitive(PrimitiveType::I64).unwrap();
    let p = parameter("std::P");
    let q = parameter("std::Q");
    let p_type = store.generic_parameter(p.clone()).unwrap();
    let q_type = store.generic_parameter(q.clone()).unwrap();
    let owner = target("std::Node");
    let owned = store.self_type(owner.clone()).unwrap();
    let foreign = store.self_type(target("std::Other")).unwrap();
    let root = store.tuple([p_type, q_type, owned, foreign]).unwrap();
    let expected = store.tuple([q_type, scalar, owned, foreign]).unwrap();
    assert_eq!(
        store.substitute(root, &BTreeMap::from([(p, q_type), (q, scalar)])),
        Ok(expected),
    );

    let replacement = store.tuple([owned, foreign, p_type]).unwrap();
    let expected = store.tuple([p_type, q_type, replacement, foreign]).unwrap();
    assert_eq!(
        store.replace_self_type(root, &owner, replacement),
        Ok(expected)
    );
    assert_eq!(
        store.replace_self_type(foreign, &owner, replacement),
        Ok(foreign)
    );
}

#[test]
fn malformed_children_preserve_error_priority_and_completed_store_prefix() {
    for owned_self in [false, true] {
        let mut store = CanonicalTypeStore::new().unwrap();
        let scalar = store.primitive(PrimitiveType::I64).unwrap();
        let owner = target("std::Node");
        let p = parameter("std::P");
        let leaf = if owned_self {
            store.self_type(owner.clone()).unwrap()
        } else {
            store.generic_parameter(p.clone()).unwrap()
        };
        let good = store
            .nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [leaf])
            .unwrap();
        let unknown_type = TypeId::from_index(store.type_count() + 100).unwrap();
        let unknown_substitution = SubstId::from_index(store.substitution_count() + 100).unwrap();
        let mut cases = Vec::new();
        for base in [good, unknown_type] {
            let projection = store
                .intern_type(TypeKind::AssociatedProjection(AssociatedProjection::new(
                    base,
                    TraitRef::new(target("std::Linked"), unknown_substitution),
                    target("std::Linked::Item"),
                )))
                .unwrap();
            cases.push(projection);
        }
        cases.push(
            store
                .intern_type(TypeKind::Callable {
                    parameters: vec![good].into_boxed_slice(),
                    result: unknown_type,
                })
                .unwrap(),
        );
        let arguments = store.intern_substitution([good, leaf]).unwrap();
        cases.push(
            store
                .intern_type(TypeKind::Nominal {
                    head: NominalHead::Intrinsic(IntrinsicNominal::Own),
                    substitution: arguments,
                })
                .unwrap(),
        );

        let replacements = BTreeMap::from([(p, scalar)]);
        for root in cases {
            let mut actual_store = copy_store(&store);
            let mut expected_store = copy_store(&store);
            let expected = if owned_self {
                expected_store.recursive_replace_self(root, &owner, scalar, &mut BTreeMap::new())
            } else {
                expected_store.recursive_substitute(root, &replacements, &mut BTreeMap::new())
            };
            assert!(expected.is_err());
            let actual = if owned_self {
                actual_store.replace_self_type(root, &owner, scalar)
            } else {
                actual_store.substitute(root, &replacements)
            };
            assert_eq!(actual, expected);
            assert_store_parity(&actual_store, &expected_store);
        }
    }
}

#[test]
fn self_rewrite_validates_root_before_replacement_without_mutating_store() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let owner = target("std::Node");
    let root = store.self_type(owner.clone()).unwrap();
    let before = copy_store(&store);
    let unknown_root = TypeId::from_index(store.type_count() + 100).unwrap();
    let unknown_replacement = TypeId::from_index(store.type_count() + 101).unwrap();
    assert_eq!(
        store.replace_self_type(unknown_root, &owner, unknown_replacement),
        Err(TypeStoreError::UnknownType(unknown_root)),
    );
    assert_eq!(
        store.replace_self_type(root, &owner, unknown_replacement),
        Err(TypeStoreError::UnknownType(unknown_replacement)),
    );
    assert_store_parity(&store, &before);
}
