use std::cell::Cell;

use super::*;
use crate::traits::overlap::classify_overlap;
use crate::*;
use eglur_diagnostics::{DiagnosticStore, SourceLabel, SourceSpan};
use eglur_source::{FileId, LineIndex, SourceFile};

mod trusted;

thread_local! {
    static COMPARISONS: Cell<usize> = const { Cell::new(0) };
}

pub(in crate::traits) fn observe_comparison() {
    COMPARISONS.update(|count| count + 1);
}

pub(in crate::traits) fn reset_comparisons() {
    COMPARISONS.set(0);
}

pub(in crate::traits) fn comparisons() -> usize {
    COMPARISONS.get()
}

struct Support;

impl CompileSupport for Support {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

fn performed<T>(phase: PhaseAnalysis<T>) -> T {
    let PhaseProduct::Performed(product) = phase.into_parts().0 else {
        panic!("fixture requires a performed phase");
    };
    product
}

pub(in crate::traits) fn with_semantics<T>(
    text: &str,
    external: &ExternalNameEnvironment,
    trusted: Option<&TrustedSignatureEnvironment>,
    inspect: impl FnOnce(
        &DeclarationCollection,
        &mut DeclarationSignatures,
        &[SemanticSource<'_>],
        &mut DiagnosticStore,
        usize,
    ) -> T,
) -> Result<T, SignatureResolutionError> {
    let file = FileId::new(0);
    let source = SourceFile::new(file, text.as_bytes().to_vec());
    let validated = source.validate().unwrap();
    let lines = LineIndex::new(validated);
    let parsed = eglur_syntax::parse_validated(validated);
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    let hir = eglur_hir::lower(&parsed);
    assert!(hir.invalid_occurrences().is_empty());
    let label = SourceLabel::new("main.eglur").unwrap();
    let spans = hir.origins().map(|(origin, value)| {
        (
            origin,
            SourceSpan::from_origin(label.clone(), value, &lines).unwrap(),
        )
    });
    let module = ModuleAddress::new([Box::<str>::from("main")]).unwrap();
    let sources = [SemanticSource::new(file, module, Some(&hir), spans, [], None).unwrap()];
    let declarations = performed(collect_declarations(&sources, &Support).unwrap());
    let mut diagnostics = DiagnosticStore::new();
    let plan = performed(
        plan_imports(
            &declarations,
            &sources,
            &Support,
            Some(external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let unit = ImportAnalysisUnit::new([file]).unwrap();
    let imports = performed(
        resolve_imports(
            &declarations,
            &plan,
            &[unit],
            &sources,
            &Support,
            Some(external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    reset_comparisons();
    let product = performed(resolve_declaration_signatures(
        &declarations,
        &imports,
        &sources,
        &Support,
        SignatureResolutionInputs::new(
            Some(external),
            trusted.map(|signatures| TrustedDeclarationInputs::new(signatures, None)),
        ),
        &mut diagnostics,
    )?);
    let signature_comparisons = comparisons();
    let (mut signatures, _) = product.into_parts();
    Ok(inspect(
        &declarations,
        &mut signatures,
        &sources,
        &mut diagnostics,
        signature_comparisons,
    ))
}

fn empty_external() -> ExternalNameEnvironment {
    ExternalNameEnvironment::new([], [], []).unwrap()
}

pub(in crate::traits) fn all_source_headers(
    declarations: &DeclarationCollection,
    signatures: &DeclarationSignatures,
) -> Vec<TraitImplHeader> {
    declarations
        .modules()
        .flat_map(|module| module.declarations())
        .filter(|declaration| declaration.kind() == DeclarationKind::Impl)
        .map(|declaration| {
            let target = CanonicalTarget::Source(declaration.id());
            match signatures.impl_header(&target) {
                FactState::Known(header) => header.clone(),
                FactState::Poisoned(_) => signatures
                    .rejected_impl_headers()
                    .find(|(key, _)| *key == &target)
                    .unwrap()
                    .1
                    .clone(),
                _ => panic!("fixture has a resolved impl header"),
            }
        })
        .collect()
}

#[test]
fn target_head_partitions_cover_payload_kinds_without_expanding_children() {
    let mut types = CanonicalTypeStore::new().unwrap();
    let scalar = types.primitive(PrimitiveType::I64).unwrap();
    let boolean = types.primitive(PrimitiveType::Bool).unwrap();
    let owner = CanonicalTarget::External(ExternalDefId::new("std::probe::Host").unwrap());
    let other = CanonicalTarget::External(ExternalDefId::new("std::probe::Other").unwrap());
    let parameter = types
        .generic_parameter(GenericParameterId::Trusted {
            owner: ExternalDefId::new("std::probe::impl").unwrap(),
            index: 0,
        })
        .unwrap();
    let nominal_head = NominalHead::Declaration(owner.clone());
    types
        .register_nominal_head(nominal_head.clone(), 1)
        .unwrap();
    let first_nominal = types.nominal(nominal_head.clone(), [scalar]).unwrap();
    let second_nominal = types.nominal(nominal_head.clone(), [boolean]).unwrap();
    let self_type = types.self_type(owner.clone()).unwrap();
    let shared = types
        .reference(ReferenceMutability::Shared, scalar)
        .unwrap();
    let mutable = types
        .reference(ReferenceMutability::Mutable, scalar)
        .unwrap();
    let pointer = types
        .raw_pointer(RawPointerMutability::Const, scalar)
        .unwrap();
    let mutable_pointer = types
        .raw_pointer(RawPointerMutability::Mutable, scalar)
        .unwrap();
    let tuple = types.tuple([scalar, boolean]).unwrap();
    let callable = types.callable([scalar, boolean], scalar).unwrap();
    let union = types.error_union([scalar, boolean]).unwrap();
    let trait_ref = TraitRef::new(other.clone(), types.intern_substitution([]).unwrap());
    let projection = types
        .associated_projection(AssociatedProjection::new(self_type, trait_ref, other))
        .unwrap();
    for (root, expected) in [
        (types.never(), ImplTargetBucket::Never),
        (scalar, ImplTargetBucket::Primitive(PrimitiveType::I64)),
        (boolean, ImplTargetBucket::Primitive(PrimitiveType::Bool)),
        (parameter, ImplTargetBucket::Blanket),
        (
            first_nominal,
            ImplTargetBucket::Nominal(nominal_head.clone()),
        ),
        (second_nominal, ImplTargetBucket::Nominal(nominal_head)),
        (self_type, ImplTargetBucket::SelfType(owner)),
        (
            shared,
            ImplTargetBucket::Reference(ReferenceMutability::Shared),
        ),
        (
            mutable,
            ImplTargetBucket::Reference(ReferenceMutability::Mutable),
        ),
        (
            pointer,
            ImplTargetBucket::RawPointer(RawPointerMutability::Const),
        ),
        (
            mutable_pointer,
            ImplTargetBucket::RawPointer(RawPointerMutability::Mutable),
        ),
        (tuple, ImplTargetBucket::Tuple(2)),
        (callable, ImplTargetBucket::Callable(2)),
        (union, ImplTargetBucket::ErrorUnion(2)),
        (projection, ImplTargetBucket::Projection),
    ] {
        assert_eq!(target_bucket(&types, root).unwrap(), expected);
    }
}

#[test]
fn candidates_preserve_all_conflicts_and_admission_order_under_reordering() {
    let text = "trait Mark[A] {}\ntrait Other {}\nstruct Host[T] {}\nstruct Away {}\n\
        impl[T] Mark[i64] for T {}\n\
        impl Mark[Bool] for Host[i64] {}\n\
        impl[A] Mark[A] for Host[i64] {}\n\
        impl Mark[Bool] for Away {}\n\
        impl Mark[i64] for Host[Bool] {}\n\
        impl Other for Host[i64] {}\n\
        impl[T] Other for Host[T] {}\n\
        impl Mark[Bool] for Host[i64] {}\n";
    with_semantics(
        text,
        &empty_external(),
        None,
        |declarations, signatures, _, _, _| {
            let headers = all_source_headers(declarations, signatures);
            let mut orders = vec![headers.clone()];
            let mut reversed = headers.clone();
            reversed.reverse();
            orders.push(reversed);
            for offset in 1..headers.len() {
                let mut rotated = headers.clone();
                rotated.rotate_left(offset);
                orders.push(rotated);
            }
            for order in orders {
                let mut index = CoherenceIndex::default();
                for incoming in &order {
                    let candidates = index
                        .candidates(signatures.types(), incoming)
                        .unwrap()
                        .collect::<Vec<_>>();
                    assert!(candidates.windows(2).all(|pair| pair[0].0 < pair[1].0));
                    let expected = index.headers().iter().find_map(|prior| {
                        classify_overlap(signatures, prior, incoming)
                            .unwrap()
                            .map(|kind| (prior.declaration(), kind))
                    });
                    let actual = candidates.iter().find_map(|(_, prior)| {
                        classify_overlap(signatures, prior, incoming)
                            .unwrap()
                            .map(|kind| (prior.declaration(), kind))
                    });
                    assert_eq!(actual, expected);
                    for prior in index.headers() {
                        if classify_overlap(signatures, prior, incoming)
                            .unwrap()
                            .is_some()
                        {
                            assert!(candidates.iter().any(
                                |(_, candidate)| candidate.declaration() == prior.declaration()
                            ));
                        }
                    }
                    index.insert(signatures.types(), incoming.clone()).unwrap();
                }
                for query in &headers {
                    let candidates = index
                        .candidates(signatures.types(), query)
                        .unwrap()
                        .collect::<Vec<_>>();
                    for prior in &order {
                        if classify_overlap(signatures, prior, query)
                            .unwrap()
                            .is_some()
                        {
                            assert!(candidates.iter().any(
                                |(_, candidate)| candidate.declaration() == prior.declaration()
                            ));
                        }
                    }
                }
            }
        },
    )
    .unwrap();
}

#[test]
fn native_source_and_checked_passes_only_classify_compatible_head_buckets() {
    for count in [1, 8, 32, 128] {
        for same_head in [false, true] {
            let mut text = String::from("trait Mark[A] {}\nstruct Common {}\n");
            for index in 0..count {
                let head = if same_head {
                    "Common".into()
                } else {
                    format!("Host{index}")
                };
                text.push_str(&format!("struct Arg{index} {{}}\n"));
                if !same_head {
                    text.push_str(&format!("struct {head} {{}}\n"));
                }
                text.push_str(&format!("impl Mark[Arg{index}] for {head} {{}}\n"));
            }
            let expected = if same_head {
                count * (count - 1) / 2
            } else {
                0
            };
            with_semantics(
                &text,
                &empty_external(),
                None,
                |declarations, signatures, sources, diagnostics, signature_work| {
                    assert!(signature_work <= expected);
                    let headers = all_source_headers(declarations, signatures);
                    assert_eq!(headers.len(), count);
                    for header in headers {
                        assert!(matches!(
                            signatures.impl_header(header.declaration()),
                            FactState::Known(_)
                        ));
                        assert!(matches!(
                            signatures.declaration(header.declaration()),
                            FactState::Known(DeclarationSignature::Impl { .. })
                        ));
                    }
                    reset_comparisons();
                    performed(
                        build_trait_environment(
                            declarations,
                            sources,
                            &Support,
                            signatures,
                            diagnostics,
                        )
                        .unwrap(),
                    );
                    assert!(comparisons() <= expected);
                    assert!(
                        std::mem::take(diagnostics)
                            .finalize(None)
                            .diagnostics()
                            .is_empty()
                    );
                },
            )
            .unwrap();
        }
    }
}

#[test]
fn structural_target_candidates_preserve_classifier_conflicts() {
    let targets = [
        "Never",
        "i64",
        "Bool",
        "&i64",
        "&Bool",
        "&mut i64",
        "(i64, Bool)",
        "(Bool, i64)",
        "(i64, Bool, i64)",
        "Fn(i64) -> Bool",
        "Fn(Bool) -> i64",
        "Fn(i64, Bool) -> Unit",
        "Res[Unit, Union[FirstError, SecondError]]",
        "Res[Unit, Union[SecondError, FirstError]]",
        "Res[Unit, Union[FirstError, ThirdError]]",
    ];
    let mut text = String::from(
        "trait Mark[A] {}\nstruct FirstError {}\nstruct SecondError {}\nstruct ThirdError {}\nimpl[T] Mark[i64] for T {}\n",
    );
    for target in targets {
        text.push_str(&format!("impl Mark[i64] for {target} {{}}\n"));
    }
    with_semantics(
        &text,
        &empty_external(),
        None,
        |declarations, signatures, _, _, _| {
            let headers = all_source_headers(declarations, signatures);
            let mut index = CoherenceIndex::default();
            for header in &headers {
                index.insert(signatures.types(), header.clone()).unwrap();
            }
            for query in &headers {
                let candidates = index
                    .candidates(signatures.types(), query)
                    .unwrap()
                    .map(|(_, header)| header)
                    .collect::<Vec<_>>();
                for prior in &headers {
                    if classify_overlap(signatures, prior, query)
                        .unwrap()
                        .is_some()
                    {
                        assert!(
                            candidates
                                .iter()
                                .any(|header| header.declaration() == prior.declaration())
                        );
                    }
                }
            }
        },
    )
    .unwrap();
}

#[test]
fn source_conflict_code_and_related_span_follow_admission_not_bucket_order() {
    let blanket = "impl[T] Mark[i64] for T {}\n";
    let rigid = "impl Mark[Bool] for Host {}\n";
    for (first, second, code) in [
        (blanket, rigid, "TRAIT_IMPL_OVERLAP"),
        (rigid, blanket, "SPECIALIZATION_FORBIDDEN"),
    ] {
        let text = format!(
            "trait Mark[A] {{}}\nstruct Host {{}}\n{first}{second}impl[A] Mark[A] for Host {{}}\n"
        );
        with_semantics(
            &text,
            &empty_external(),
            None,
            |declarations, signatures, sources, diagnostics, _| {
                let first = declarations
                    .modules()
                    .flat_map(|module| module.declarations())
                    .find(|declaration| declaration.kind() == DeclarationKind::Impl)
                    .unwrap();
                let expected_span = sources[0].origin_span(first.origin()).unwrap();
                reset_comparisons();
                performed(
                    build_trait_environment(
                        declarations,
                        sources,
                        &Support,
                        signatures,
                        diagnostics,
                    )
                    .unwrap(),
                );
                let snapshot = std::mem::take(diagnostics).finalize(None);
                assert_eq!(snapshot.diagnostics().len(), 1);
                assert_eq!(snapshot.diagnostics()[0].code().as_str(), code);
                assert_eq!(snapshot.diagnostics()[0].related()[0].span(), expected_span);
            },
        )
        .unwrap();
    }
}

#[test]
fn blanket_interactions_are_counted_in_both_directions_without_unrelated_pairs() {
    for rigid in [1, 8, 32] {
        for blankets in [0_usize, 1, 4, 16] {
            let mut fixed = String::new();
            let mut broad = String::new();
            for index in 0..rigid {
                fixed.push_str(&format!(
                    "struct Host{index} {{}}\nstruct Fixed{index} {{}}\nimpl Mark[Fixed{index}] for Host{index} {{}}\n"
                ));
            }
            for index in 0..blankets {
                broad.push_str(&format!(
                    "struct Broad{index} {{}}\nimpl[T] Mark[Broad{index}] for T {{}}\n"
                ));
            }
            for text in [
                format!("trait Mark[A] {{}}\n{fixed}{broad}"),
                format!("trait Mark[A] {{}}\n{broad}{fixed}"),
            ] {
                let expected = rigid * blankets + blankets * blankets.saturating_sub(1) / 2;
                with_semantics(
                    &text,
                    &empty_external(),
                    None,
                    |declarations, signatures, sources, diagnostics, signature_work| {
                        assert!(signature_work <= expected);
                        let headers = all_source_headers(declarations, signatures);
                        assert_eq!(headers.len(), rigid + blankets);
                        assert!(headers.iter().all(|header| matches!(
                            signatures.impl_header(header.declaration()),
                            FactState::Known(_)
                        )));
                        reset_comparisons();
                        performed(
                            build_trait_environment(
                                declarations,
                                sources,
                                &Support,
                                signatures,
                                diagnostics,
                            )
                            .unwrap(),
                        );
                        assert!(comparisons() <= expected);
                        assert!(
                            std::mem::take(diagnostics)
                                .finalize(None)
                                .diagnostics()
                                .is_empty()
                        );
                    },
                )
                .unwrap();
            }
        }
    }
}

#[test]
fn visibility_rejected_headers_remain_prior_source_candidates_and_checked_poison_is_skipped() {
    let text = "pub trait Mark {}\npub struct Host[T] {}\nstruct Hidden {}\n\
        impl Mark for Host[Hidden] {}\nimpl Mark for Host[Hidden] {}\n";
    with_semantics(
        text,
        &empty_external(),
        None,
        |declarations, signatures, sources, diagnostics, work| {
            assert!(work <= 1);
            let headers = all_source_headers(declarations, signatures);
            for header in &headers {
                assert!(matches!(
                    signatures.impl_header(header.declaration()),
                    FactState::Poisoned(_)
                ));
            }
            reset_comparisons();
            performed(
                build_trait_environment(declarations, sources, &Support, signatures, diagnostics)
                    .unwrap(),
            );
            assert_eq!(comparisons(), 0);
            let snapshot = std::mem::take(diagnostics).finalize(None);
            let codes = snapshot
                .diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.code().as_str())
                .collect::<Vec<_>>();
            assert_eq!(codes, ["VISIBILITY_VIOLATION", "TRAIT_IMPL_DUPLICATE"]);
        },
    )
    .unwrap();
}
