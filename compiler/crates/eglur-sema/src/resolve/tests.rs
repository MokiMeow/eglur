use std::cell::Cell;

use super::*;
use crate::{ImportAnalysisUnit, collect_declarations, plan_imports, resolve_imports};
use eglur_diagnostics::SourceLabel;
use eglur_hir::lower;
use eglur_source::{LineIndex, SourceFile};
use eglur_syntax::parse_validated;

mod callable_patterns;
mod fixture;

pub(super) use fixture::with_resolver;

thread_local! {
    static ORIGIN_VISITS: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn record_origin_visits(count: usize) {
    ORIGIN_VISITS.set(ORIGIN_VISITS.get() + count);
}

struct AllCapabilities;

impl CompileSupport for AllCapabilities {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

fn resolve_pattern(pattern: &str) -> NameResolution {
    let text = format!("fn f[T](value: T) -> T = match value {{ {pattern} => value }}\n");
    let file = FileId::new(0);
    let input = SourceFile::new(file, text.into_bytes());
    let validated = input.validate().unwrap();
    let lines = LineIndex::new(validated);
    let parse = parse_validated(validated);
    assert!(parse.diagnostics().is_empty(), "{:?}", parse.diagnostics());
    let hir = lower(&parse);
    assert!(hir.invalid_occurrences().is_empty());
    let label = SourceLabel::new("main.eglur").unwrap();
    let spans = hir.origins().map(|(origin, value)| {
        (
            origin,
            SourceSpan::from_origin(label.clone(), value, &lines).unwrap(),
        )
    });
    let sources = [SemanticSource::new(
        file,
        ModuleAddress::new([Box::<str>::from("main")]).unwrap(),
        Some(&hir),
        spans,
        [],
        None,
    )
    .unwrap()];
    let mut diagnostics = DiagnosticStore::new();
    let PhaseProduct::Performed(declarations) = collect_declarations(&sources, &AllCapabilities)
        .unwrap()
        .into_parts()
        .0
    else {
        panic!("declarations unavailable")
    };
    let PhaseProduct::Performed(plan) = plan_imports(
        &declarations,
        &sources,
        &AllCapabilities,
        None,
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("import plan unavailable")
    };
    let PhaseProduct::Performed(imports) = resolve_imports(
        &declarations,
        &plan,
        &[ImportAnalysisUnit::new([file]).unwrap()],
        &sources,
        &AllCapabilities,
        None,
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("imports unavailable")
    };

    ORIGIN_VISITS.set(0);
    let PhaseProduct::Performed(names) = resolve_names(
        &declarations,
        &imports,
        &sources,
        &AllCapabilities,
        None,
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("name resolution unavailable")
    };
    assert!(diagnostics.finalize(None).diagnostics().is_empty());
    names
}

#[test]
fn alias_binding_origins_are_canonicalized_once() {
    for depth in [0, 8, 32, 128, 254] {
        for repeated in [false, true] {
            let mut pattern = String::new();
            for index in 0..depth {
                pattern.push_str(&format!("x{} @ ", if repeated { 0 } else { index }));
            }
            pattern.push_str("leaf");
            let names = resolve_pattern(&pattern);
            assert_eq!(ORIGIN_VISITS.get(), depth + 2, "depth {depth}, {repeated}");
            assert_eq!(
                names
                    .locals()
                    .iter()
                    .map(|local| local.origins().len())
                    .sum::<usize>(),
                depth + 2,
            );
            assert_eq!(
                names.locals().len(),
                if repeated && depth > 0 { 3 } else { depth + 2 },
            );
        }
    }
}

#[test]
fn skew_tuple_bindings_are_canonicalized_once_in_source_order() {
    for depth in [0, 8, 32, 128, 254] {
        let mut pattern = "leaf".to_owned();
        for index in (0..depth).rev() {
            pattern = format!("(x{index}, {pattern})");
        }
        let names = resolve_pattern(&pattern);
        assert_eq!(ORIGIN_VISITS.get(), depth + 2, "depth {depth}");
        let mut expected = vec!["value".to_owned()];
        expected.extend((0..depth).map(|index| format!("x{index}")));
        expected.push("leaf".to_owned());
        assert_eq!(
            names
                .locals()
                .iter()
                .map(|local| local.name().spelling())
                .collect::<Vec<_>>(),
            expected,
        );
    }
}
