use super::*;
use crate::{ExternalDefId, PrimitiveType};

#[test]
fn structural_unions_never_bind_members_and_wait_for_independent_positions() {
    for parameter_first in [true, false] {
        let mut types = CanonicalTypeStore::new().unwrap();
        let parameter = GenericParameterId::Trusted {
            owner: ExternalDefId::new("fixture::union").unwrap(),
            index: 0,
        };
        let (variable, fixed) = if parameter_first {
            let variable = types.generic_parameter(parameter.clone()).unwrap();
            (variable, types.primitive(PrimitiveType::Str).unwrap())
        } else {
            let fixed = types.primitive(PrimitiveType::Str).unwrap();
            (types.generic_parameter(parameter.clone()).unwrap(), fixed)
        };
        let boolean = types.primitive(PrimitiveType::Bool).unwrap();
        let integer = types.primitive(PrimitiveType::I64).unwrap();
        let template = types.error_union([variable, fixed]).unwrap();
        let actual = types.error_union([boolean, fixed]).unwrap();
        let parameters = BTreeSet::from([parameter.clone()]);
        let mut replacements = BTreeMap::new();
        assert!(
            match_inference_patterns(
                &mut types,
                [(template, actual, true)],
                &parameters,
                &mut replacements
            )
            .unwrap()
        );
        assert!(replacements.is_empty());

        for independent_first in [true, false] {
            let mut roots = vec![(template, actual, true), (variable, boolean, true)];
            if independent_first {
                roots.reverse();
            }
            let mut replacements = BTreeMap::new();
            assert!(
                match_inference_patterns(&mut types, roots, &parameters, &mut replacements)
                    .unwrap()
            );
            assert_eq!(replacements[&parameter], boolean);
        }

        assert!(
            !match_inference_patterns(
                &mut types,
                [(template, actual, true), (variable, integer, true)],
                &parameters,
                &mut BTreeMap::new()
            )
            .unwrap()
        );
        assert!(
            match_inference_patterns(
                &mut types,
                [(template, fixed, true), (variable, fixed, true)],
                &parameters,
                &mut BTreeMap::new()
            )
            .unwrap()
        );

        let mut replacements = BTreeMap::new();
        assert!(
            match_inference_patterns(
                &mut types,
                [(variable, actual, true)],
                &parameters,
                &mut replacements
            )
            .unwrap()
        );
        assert_eq!(replacements[&parameter], actual);
    }
}

#[test]
fn deferred_unions_share_substitution_and_unbound_parameter_graphs() {
    let mut types = CanonicalTypeStore::new().unwrap();
    let parameter = GenericParameterId::Trusted {
        owner: ExternalDefId::new("fixture::shared_union").unwrap(),
        index: 0,
    };
    let mut template = types.generic_parameter(parameter.clone()).unwrap();
    let variable = template;
    let boolean = types.primitive(PrimitiveType::Bool).unwrap();
    let mut actual = boolean;
    let fixed = types.primitive(PrimitiveType::Str).unwrap();
    let mut roots = vec![(variable, boolean, true)];
    for _ in 0..64 {
        template = types.tuple([template, template]).unwrap();
        actual = types.tuple([actual, actual]).unwrap();
        roots.push((
            types.error_union([template, fixed]).unwrap(),
            types.error_union([actual, fixed]).unwrap(),
            true,
        ));
    }
    let parameters = BTreeSet::from([parameter.clone()]);
    let mut replacements = BTreeMap::new();
    assert!(match_inference_patterns(&mut types, roots, &parameters, &mut replacements).unwrap());
    assert_eq!(replacements[&parameter], boolean);

    let mut memo = BTreeMap::new();
    assert!(
        has_unbound_parameter(&types, template, &parameters, &BTreeMap::new(), &mut memo).unwrap()
    );
    let visited = memo.len();
    assert!(
        has_unbound_parameter(&types, variable, &parameters, &BTreeMap::new(), &mut memo).unwrap()
    );
    assert_eq!(memo.len(), visited);
}
