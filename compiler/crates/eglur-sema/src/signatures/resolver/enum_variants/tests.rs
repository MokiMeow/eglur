use std::cell::Cell;

use super::*;
use crate::signatures::resolver::tests::with_resolver;
use crate::signatures::resolver::{Resolution, SignatureResolutionError};
use crate::{DeclarationKind, DeclarationSignature, EnumVariantPayloadSignature};

#[derive(Clone, Copy, Debug, Default)]
struct Work {
    items: usize,
    variants: usize,
    queries: usize,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work { items: 0, variants: 0, queries: 0 }) };
}

pub(in super::super) fn observe_item() {
    WORK.update(|mut work| {
        work.items += 1;
        work
    });
}

pub(in super::super) fn observe_variant() {
    WORK.update(|mut work| {
        work.variants += 1;
        work
    });
}

pub(in super::super) fn observe_query() {
    WORK.update(|mut work| {
        work.queries += 1;
        work
    });
}

fn lower(file: FileId, text: &str) -> HirFile {
    let source = eglur_source::SourceFile::new(file, text.as_bytes().to_vec());
    let parsed = eglur_syntax::parse_validated(source.validate().unwrap());
    assert!(parsed.diagnostics().is_empty());
    let hir = eglur_hir::lower(&parsed);
    assert!(hir.invalid_occurrences().is_empty());
    hir
}

fn text(count: usize, reversed: bool) -> String {
    let mut positions = (0..count).collect::<Vec<_>>();
    if reversed {
        positions.reverse();
    }
    let variants = positions
        .iter()
        .map(|index| match index % 3 {
            0 => format!("V{index}"),
            1 => format!("V{index}(i64, Bool)"),
            _ => format!("V{index} {{ first:i64, second:Bool }}"),
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "fn before(value: Bool) -> Bool = value\nenum First {{ {variants} }}\nenum Second {{ {variants} }}\n"
    )
}

#[test]
fn native_hir_origin_lookup_is_prepared_once_and_keeps_file_item_and_query_order() {
    for count in [1, 8, 32, 128] {
        for reversed in [false, true] {
            let source = text(count, reversed);
            let hirs = [
                lower(FileId::new(0), &source),
                lower(FileId::new(1), &source),
            ];
            let expected_items = hirs.iter().map(|hir| hir.items().len()).sum::<usize>();
            let expected_variants = hirs
                .iter()
                .flat_map(HirFile::items)
                .filter_map(|(_, item)| match item.kind() {
                    ItemKind::Enum(value) => Some(value.variants.len()),
                    _ => None,
                })
                .sum::<usize>();

            WORK.set(Work::default());
            let index = SourceEnumVariantIndex::new(&hirs);
            assert!(index.by_location.len() <= expected_variants);
            let mut queries = 0;
            for hir in &hirs {
                let non_enum = hir.roots()[0];
                let missing_origin = hir.item(non_enum).origin();
                for (item, value) in hir.items() {
                    let ItemKind::Enum(value) = value.kind() else {
                        continue;
                    };
                    for variant in value.variants.iter().rev() {
                        let expected = value
                            .variants
                            .iter()
                            .find(|candidate| candidate.origin == variant.origin)
                            .unwrap();
                        let actual = index.get(hir.file(), item, variant.origin).unwrap();
                        assert!(std::ptr::eq(actual, expected));
                        assert!(index.get(hir.file(), non_enum, variant.origin).is_none());
                        assert!(index.get(FileId::new(9), item, variant.origin).is_none());
                        assert!(index.get(hir.file(), item, missing_origin).is_none());
                        queries += 4;
                    }
                }
            }
            let work = WORK.get();
            assert!(work.items <= expected_items);
            assert!(work.variants <= expected_variants);
            assert!(work.queries <= queries);
        }
    }
}

#[test]
fn duplicate_origins_retain_the_first_variant_without_changing_name_or_payload() {
    let hir = lower(
        FileId::new(0),
        "enum E { First(i64)\n Second { value:Bool } }\n",
    );
    let item = hir.roots()[0];
    let ItemKind::Enum(value) = hir.item(item).kind() else {
        panic!("enum fixture");
    };
    let mut variants = value.variants.to_vec();
    variants[1].origin = variants[0].origin;
    let mut index = SourceEnumVariantIndex::new([]);
    index.insert_enum(hir.file(), item, &variants);

    assert!(std::ptr::eq(
        index.get(hir.file(), item, variants[0].origin).unwrap(),
        &variants[0]
    ));
    assert_eq!(index.by_location.len(), 1);
}

#[test]
fn resolver_uses_prepared_origins_and_keeps_each_declarations_payload() {
    for count in [1, 8, 32, 128] {
        for reversed in [false, true] {
            let source = text(count, reversed);
            WORK.set(Work::default());
            with_resolver(&[&source], |resolver, sources| {
                let hir = sources[0].hir().unwrap();
                let variants = resolver
                    .collection
                    .modules()
                    .flat_map(|module| module.declarations())
                    .filter(|declaration| declaration.kind() == DeclarationKind::EnumVariant)
                    .collect::<Vec<_>>();
                for declaration in variants.iter().rev() {
                    let Resolution::Known(DeclarationSignature::EnumVariant {
                        name, payload, ..
                    }) = resolver
                        .resolve_source_enum_variant(declaration.id(), &sources[0], hir)
                        .unwrap()
                    else {
                        panic!("known native variant");
                    };
                    assert_eq!(&name, declaration.name().unwrap());
                    let ordinal = name
                        .text()
                        .strip_prefix('V')
                        .unwrap()
                        .parse::<usize>()
                        .unwrap();
                    assert!(matches!(
                        (ordinal % 3, payload),
                        (0, EnumVariantPayloadSignature::Absent)
                            | (1, EnumVariantPayloadSignature::Tuple(_))
                            | (2, EnumVariantPayloadSignature::Named(_))
                    ));
                }
                let work = WORK.get();
                assert!(work.items <= hir.items().len());
                assert!(work.variants <= variants.len());
                assert!(work.queries <= variants.len());
            });
        }
    }
}

#[test]
fn wrong_parent_hir_kind_is_refused_before_variant_lookup() {
    with_resolver(
        &["enum E { Item }\n", "const E: Bool = true\n"],
        |resolver, sources| {
            let declaration = resolver
                .collection
                .modules()
                .flat_map(|module| module.declarations())
                .find(|declaration| declaration.kind() == DeclarationKind::EnumVariant)
                .unwrap();
            let parent = declaration.parent().unwrap();
            WORK.set(Work::default());
            assert_eq!(
                resolver
                    .resolve_source_enum_variant(
                        declaration.id(),
                        &sources[1],
                        sources[1].hir().unwrap()
                    )
                    .err(),
                Some(SignatureResolutionError::WrongDeclarationItem(parent))
            );
            assert_eq!(WORK.get().queries, 0);
        },
    );
}
