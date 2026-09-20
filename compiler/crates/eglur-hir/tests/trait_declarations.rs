use eglur_hir::{ItemKind, Visibility, lower};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, parse};

#[test]
fn closed_is_a_modifier_only_before_the_trait_keyword() {
    let cases = [
        ("trait Open {}\n", "Open", false, Visibility::Private),
        ("trait closed {}\n", "closed", false, Visibility::Private),
        ("pub trait closed {}\n", "closed", false, Visibility::Public),
        ("trait closed[T] {}\n", "closed", false, Visibility::Private),
        (
            "trait Open[closed] {}\n",
            "Open",
            false,
            Visibility::Private,
        ),
        (
            "trait Open { type closed\n}\n",
            "Open",
            false,
            Visibility::Private,
        ),
        (
            "trait Open { fn closed() -> Unit\n}\n",
            "Open",
            false,
            Visibility::Private,
        ),
        (
            "pub /* closed */ trait closed {}\n",
            "closed",
            false,
            Visibility::Public,
        ),
        ("closed trait Mark {}\n", "Mark", true, Visibility::Private),
        (
            "closed trait closed {}\n",
            "closed",
            true,
            Visibility::Private,
        ),
        (
            "pub closed /* prefix */ trait /* name */ closed[T] {}\n",
            "closed",
            true,
            Visibility::Public,
        ),
    ];

    for (text, name, closed, visibility) in cases {
        let source = SourceFile::new(FileId::new(0), text.as_bytes().to_vec());
        let FrontendOutcome::Parsed(parse) = parse(&source) else {
            panic!("test source should reach the parser");
        };
        assert!(parse.is_success(), "{text}: {:?}", parse.diagnostics());

        let hir = lower(&parse);
        assert!(hir.invalid_occurrences().is_empty(), "{text}");
        assert_eq!(hir.roots().len(), 1);
        let item = hir.item(hir.roots()[0]);
        let ItemKind::Trait(declaration) = item.kind() else {
            panic!("test source should lower to a trait");
        };

        assert_eq!(declaration.closed, closed, "{text}");
        assert_eq!(declaration.name.text(), Some(name), "{text}");
        assert_eq!(item.visibility(), visibility, "{text}");
        let range = hir.origin(declaration.name.origin()).source().range();
        assert_eq!(
            &text[range.start().as_usize()..range.end().as_usize()],
            name
        );
        assert!(range.start().as_usize() > text.find("trait").unwrap());
    }
}
