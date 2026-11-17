use eglur_hir::{InvalidCategory, ItemKind, SourceOrigin, Visibility};
use eglur_syntax::SyntaxKind;

use super::LoweredDeclarations;

#[test]
fn malformed_declarations_preserve_the_invalid_region_and_following_sentinel() {
    let before = "const Before: BeforeType = 803";
    let after = "pub const After: AfterType = 809";
    for malformed in [
        "fn broken(arg:) -> Result = 1",
        "extern \"C\" fn broken(: Arg)",
        "import names::{Thing as}",
        "struct Broken { field value:\n}",
        "trait Broken { type\n}",
        "impl Contract for {}",
        "enum Broken { Named { missing: }\n}",
        "#[bad(flag =)] fn broken() = 1",
    ] {
        let text = format!("{before}\n{malformed}\n{after}\n");
        let source = LoweredDeclarations::parse(&text);
        assert!(!source.parsed.is_success(), "{malformed}");
        assert_eq!(source.parsed.diagnostics().len(), 1, "{malformed}");
        let regions: Vec<_> = source
            .parsed
            .syntax()
            .children()
            .filter(|node| node.kind() == SyntaxKind::ErrorNode)
            .collect();
        assert!(!regions.is_empty(), "{malformed}");
        assert_eq!(source.hir.roots().len(), regions.len() + 2, "{malformed}");
        for (index, spelling, name, ty, value, visibility) in [
            (
                0,
                before,
                "Before",
                "BeforeType",
                "803",
                Visibility::Private,
            ),
            (
                regions.len() + 1,
                after,
                "After",
                "AfterType",
                "809",
                Visibility::Public,
            ),
        ] {
            let item = source.item(
                source.hir.roots()[index],
                SyntaxKind::ConstDecl,
                spelling,
                visibility,
            );
            assert!(item.attributes().is_empty());
            let ItemKind::Const(constant) = item.kind() else {
                panic!("expected untouched sentinel: {malformed}");
            };
            source.name(&constant.name, name);
            source.ty(constant.ty, ty);
            source.integer(constant.body, constant.value, value);
        }

        for (index, region) in regions.iter().enumerate() {
            let invalid = source.hir.item(source.hir.roots()[index + 1]);
            assert!(matches!(invalid.kind(), ItemKind::Invalid));
            assert_eq!(
                source.hir.origin(invalid.origin()),
                SourceOrigin::Written(region.span())
            );
            assert!(invalid.attributes().is_empty());
        }

        let causes: Vec<_> = source.hir.syntax_causes().collect();
        let [(cause, span)] = causes.as_slice() else {
            panic!("expected one syntax cause");
        };
        let diagnostic = &source.parsed.diagnostics()[0];
        assert_eq!(*span, diagnostic.span());
        let occurrences = source.hir.invalid_occurrences();
        assert_eq!(occurrences.len(), regions.len() + 1);
        for (category, expected) in regions
            .iter()
            .map(|region| (InvalidCategory::Item, region.span()))
            .chain(std::iter::once((
                InvalidCategory::ReportedSyntax,
                diagnostic.span(),
            )))
        {
            let matching: Vec<_> = occurrences
                .iter()
                .filter(|entry| {
                    entry.category() == category
                        && source.hir.origin(entry.origin()) == SourceOrigin::Written(expected)
                })
                .collect();
            assert_eq!(matching.len(), 1);
            assert_eq!(
                source.hir.origin(matching[0].origin()),
                SourceOrigin::Written(expected)
            );
            assert_eq!(matching[0].syntax_causes(), &[*cause]);
        }
    }
}
