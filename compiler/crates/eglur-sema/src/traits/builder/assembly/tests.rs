use super::*;
use crate::traits::coherence_index::tests::{
    all_source_headers, comparisons, reset_comparisons, with_semantics,
};

fn global_pair_reference(
    signatures: &DeclarationSignatures,
    implementations: &BTreeMap<CanonicalTarget, TraitImpl>,
) -> Result<(), TraitEnvironmentError> {
    let headers = implementations.values().collect::<Vec<_>>();
    for (index, left) in headers.iter().enumerate() {
        for right in &headers[index + 1..] {
            if left.poison.is_none()
                && right.poison.is_none()
                && classify_overlap(signatures, &left.header, &right.header)?.is_some()
            {
                return Err(TraitEnvironmentError::UncheckedCoherence {
                    left: left.header.declaration().clone(),
                    right: right.header.declaration().clone(),
                });
            }
        }
    }
    Ok(())
}

#[test]
fn checked_faults_preserve_left_major_error_order_and_skip_poisoned_headers() {
    let external = crate::ExternalNameEnvironment::new([], [], []).unwrap();
    let text = "trait Mark {}\nstruct First {}\nstruct Second {}\n\
        impl Mark for First {}\nimpl Mark for Second {}\n\
        impl Mark for Second {}\nimpl Mark for First {}\n";
    with_semantics(
        text,
        &external,
        None,
        |declarations, signatures, _, _, _| {
            let headers = all_source_headers(declarations, signatures);
            let FactState::Poisoned(poison) = signatures.impl_header(headers[2].declaration())
            else {
                panic!("fixture has a duplicate-header poison");
            };
            for mask in 0..(1_usize << headers.len()) {
                let implementations = headers
                    .iter()
                    .enumerate()
                    .map(|(index, header)| {
                        (
                            header.declaration().clone(),
                            TraitImpl {
                                header: header.clone(),
                                bindings: BTreeMap::new(),
                                poison: (mask & (1 << index) != 0).then(|| poison.clone()),
                            },
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                let expected = global_pair_reference(signatures, &implementations);
                reset_comparisons();
                assert_eq!(
                    validate_checked_coherence(signatures, &implementations),
                    expected
                );
                let remaining = headers.len() - mask.count_ones() as usize;
                assert!(comparisons() <= remaining * remaining.saturating_sub(1) / 2);
                if mask == 0 {
                    assert_eq!(
                        expected,
                        Err(TraitEnvironmentError::UncheckedCoherence {
                            left: headers[0].declaration().clone(),
                            right: headers[3].declaration().clone(),
                        })
                    );
                }
            }
        },
    )
    .unwrap();
}
