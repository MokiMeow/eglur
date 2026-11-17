use eglur_hir::{InvalidCategory, ItemKind, Mutability, SourceOrigin, TypeRefKind};
use eglur_syntax::SyntaxKind;

use super::LoweredTypes;

const AFTER: &str =
    "type After = pkg::Pair[&mut Recovered, Fn(RecoveredInput) -> RecoveredResult]\n";

fn assert_recovery_causes(source: &LoweredTypes) {
    let diagnostics = source.parsed.diagnostics();
    assert!(!diagnostics.is_empty());
    let causes: Vec<_> = source.hir.syntax_causes().collect();
    assert_eq!(causes.len(), diagnostics.len());

    let reported: Vec<_> = source
        .hir
        .invalid_occurrences()
        .iter()
        .filter(|occurrence| occurrence.category() == InvalidCategory::ReportedSyntax)
        .collect();
    assert_eq!(reported.len(), diagnostics.len());
    for ((cause, span), diagnostic) in causes.iter().zip(diagnostics) {
        assert_eq!(*span, diagnostic.span());
        let matching: Vec<_> = reported
            .iter()
            .filter(|occurrence| occurrence.syntax_causes() == [*cause])
            .collect();
        assert_eq!(matching.len(), 1);
        assert_eq!(
            source.hir.origin(matching[0].origin()),
            SourceOrigin::Written(diagnostic.span())
        );
    }

    let error_regions: Vec<_> = source
        .parsed
        .syntax()
        .children()
        .filter(|node| node.kind() == SyntaxKind::ErrorNode)
        .map(|node| SourceOrigin::Written(node.span()))
        .collect();
    assert!(!error_regions.is_empty());
    let invalid_roots: Vec<_> = source
        .hir
        .roots()
        .iter()
        .map(|&id| source.hir.item(id))
        .filter(|item| matches!(item.kind(), ItemKind::Invalid))
        .map(|item| source.hir.origin(item.origin()))
        .collect();
    assert_eq!(invalid_roots, error_regions);

    let invalid: Vec<_> = source
        .hir
        .invalid_occurrences()
        .iter()
        .filter(|occurrence| occurrence.category() != InvalidCategory::ReportedSyntax)
        .collect();
    assert_eq!(invalid.len(), error_regions.len());
    for origin in error_regions {
        let matching: Vec<_> = invalid
            .iter()
            .filter(|occurrence| source.hir.origin(occurrence.origin()) == origin)
            .collect();
        assert_eq!(matching.len(), 1);
        let occurrence = matching[0];
        assert_eq!(occurrence.category(), InvalidCategory::Item);
        let SourceOrigin::Written(span) = origin else {
            panic!("invalid source region must retain a written origin");
        };
        let [cause] = occurrence.syntax_causes() else {
            panic!("each disjoint invalid region must retain its own syntax cause");
        };
        let ((expected_cause, _), diagnostic) = causes
            .iter()
            .zip(diagnostics)
            .find(|(_, diagnostic)| {
                let recovery = diagnostic.recovery_span();
                span.file() == recovery.file()
                    && recovery.range().start() <= span.range().start()
                    && span.range().end() <= recovery.range().end()
            })
            .expect("invalid region should stay inside one recovery region");
        assert_eq!(cause, expected_cause);
        assert_eq!(source.hir.syntax_cause(*cause), diagnostic.span());
    }
}

fn assert_following_type(source: &LoweredTypes) {
    let arguments = source.path(
        source.alias("After"),
        "pkg::Pair[&mut Recovered, Fn(RecoveredInput) -> RecoveredResult]",
        "pkg::Pair",
    );
    assert_eq!(arguments.len(), 2);
    let TypeRefKind::Reference {
        mutability,
        referent,
    } = source.kind(arguments[0], SyntaxKind::RefType, "&mut Recovered")
    else {
        panic!("following alias must retain its reference argument");
    };
    assert_eq!(*mutability, Mutability::Mutable);
    source.leaf(*referent, "Recovered");

    let TypeRefKind::Function { parameters, result } = source.kind(
        arguments[1],
        SyntaxKind::FnType,
        "Fn(RecoveredInput) -> RecoveredResult",
    ) else {
        panic!("following alias must retain its function argument");
    };
    assert_eq!(parameters.len(), 1);
    source.leaf(parameters[0], "RecoveredInput");
    source.leaf(*result, "RecoveredResult");
}

fn alias_names(source: &LoweredTypes) -> Vec<&str> {
    source
        .hir
        .roots()
        .iter()
        .filter_map(|&id| match source.hir.item(id).kind() {
            ItemKind::TypeAlias(alias) => alias.name.text(),
            ItemKind::Invalid => None,
            actual => panic!("unexpected recovered declaration: {actual:?}"),
        })
        .collect()
}

#[test]
fn balanced_type_errors_preserve_the_following_heterogeneous_alias() {
    for invalid in [
        "(Alpha,, Beta)",
        "Fn(Alpha, &) -> Beta",
        "Fn(Alpha) Beta",
        "* Value",
        "dyn &Thing",
        "Box[Alpha, Fn(Beta) -> ]",
        "()",
    ] {
        let text = format!("type Before = Original\ntype Broken = {invalid}\n{AFTER}");
        let source = LoweredTypes::new(&text);
        assert!(!source.parsed.is_success(), "{invalid}");
        assert_eq!(alias_names(&source), ["Before", "After"], "{invalid}");
        source.leaf(source.alias("Before"), "Original");
        assert_recovery_causes(&source);
        assert_following_type(&source);
    }
}

#[test]
fn independent_type_errors_do_not_share_recovery_causes() {
    let text = format!(
        "type Before = Original\ntype BadFunction = Fn(Arg) MissingArrow\ntype Middle = Between\ntype BadTuple = (Left,, Right)\n{AFTER}"
    );
    let source = LoweredTypes::new(&text);
    assert!(!source.parsed.is_success());
    assert_eq!(source.parsed.diagnostics().len(), 2);
    assert_eq!(alias_names(&source), ["Before", "Middle", "After"]);
    source.leaf(source.alias("Before"), "Original");
    source.leaf(source.alias("Middle"), "Between");
    assert_recovery_causes(&source);
    assert_following_type(&source);
}

#[test]
fn recovery_does_not_invent_a_trusted_top_level_declaration_boundary() {
    for (invalid, separator) in [("Box[Wrong", "\n"), ("()", " ")] {
        let text = format!("type Before = Original\ntype Broken = {invalid}{separator}{AFTER}");
        let source = LoweredTypes::new(&text);
        assert!(!source.parsed.is_success());
        assert_eq!(alias_names(&source), ["Before"]);
        source.leaf(source.alias("Before"), "Original");
        assert_recovery_causes(&source);
        let recovery = source.parsed.diagnostics().last().unwrap().recovery_span();
        assert!(source.spelling(recovery).contains(AFTER.trim_end()));
    }
}
