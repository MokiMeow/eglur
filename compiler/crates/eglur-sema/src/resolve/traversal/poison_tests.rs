use super::*;
use crate::owned_poison::test_support::{Work, reset, work};
use crate::{
    ExternalNameEnvironment, FactTable, ImportAnalysisUnit, collect_declarations, plan_imports,
    resolve_imports,
};
use eglur_diagnostics::{DiagnosticStore, SourceLabel, SourceSpan};
use eglur_hir::lower;
use eglur_source::{FileId, LineIndex, SourceFile};
use eglur_syntax::parse_validated;

mod states;

struct AllCapabilities;

impl crate::CompileSupport for AllCapabilities {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

fn with_resolver(text: &str, inspect: impl FnOnce(&mut Resolver<'_, '_>, &SemanticSource<'_>)) {
    let file = FileId::new(0);
    let input = SourceFile::new(file, text.as_bytes().to_vec());
    let validated = input.validate().unwrap();
    let lines = LineIndex::new(validated);
    let parse = parse_validated(validated);
    assert!(parse.diagnostics().is_empty(), "{:?}", parse.diagnostics());
    let hir = lower(&parse);
    assert!(hir.invalid_occurrences().is_empty());
    let label = SourceLabel::new("main.eglur").unwrap();
    let sources = [SemanticSource::new(
        file,
        crate::ModuleAddress::new([Box::<str>::from("main")]).unwrap(),
        Some(&hir),
        hir.origins().map(|(origin, value)| {
            (
                origin,
                SourceSpan::from_origin(label.clone(), value, &lines).unwrap(),
            )
        }),
        [],
        None,
    )
    .unwrap()];
    let support = AllCapabilities;
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    let mut diagnostics = DiagnosticStore::new();
    let crate::PhaseProduct::Performed(collection) = collect_declarations(&sources, &support)
        .unwrap()
        .into_parts()
        .0
    else {
        panic!("declarations unavailable")
    };
    let crate::PhaseProduct::Performed(plan) = plan_imports(
        &collection,
        &sources,
        &support,
        Some(&external),
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("import plan unavailable")
    };
    let crate::PhaseProduct::Performed(imports) = resolve_imports(
        &collection,
        &plan,
        &[ImportAnalysisUnit::new([file]).unwrap()],
        &sources,
        &support,
        Some(&external),
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("imports unavailable")
    };
    let mut resolver = Resolver {
        collection: &collection,
        imports: &imports,
        external: Some(&external),
        diagnostics: &mut diagnostics,
        recorder: crate::DemandRecorder::new(&support),
        facts: FactTable::new(),
        deferred: Default::default(),
        scopes: Vec::new(),
        locals: Vec::new(),
        local_by_origin: Default::default(),
        active_scopes: Vec::new(),
        current_owner: None,
        next_scope: Default::default(),
        next_local: Default::default(),
    };
    inspect(&mut resolver, &sources[0]);
}

fn source(mode: &str, count: usize) -> String {
    let names = (0..count)
        .map(|index| format!("missing_{index}"))
        .collect::<Vec<_>>();
    let tuple = if count == 0 {
        "()".to_owned()
    } else {
        format!("({},)", names.join(","))
    };
    let body = match mode {
        "tuple" => format!("= {tuple}"),
        "array" => format!("= [{}]", names.join(",")),
        "map" => format!(
            "= [{}]",
            names
                .iter()
                .enumerate()
                .map(|(index, name)| format!("{index}: {name}"))
                .collect::<Vec<_>>()
                .join(",")
        ),
        "block" => format!("{{ {}\nyield () }}", names.join("\n")),
        "shared" => format!(
            "{{ let bad = missing\nyield ({},) }}",
            vec!["bad"; count.max(1)].join(",")
        ),
        "healthy" => format!("= ({},)", vec!["0"; count.max(1)].join(",")),
        "call" => {
            return format!(
                "fn callee({}) -> Unit = ()\nfn probe() -> Unit = callee({})\nfn after(x: i64) -> i64 = x\n",
                (0..count)
                    .map(|index| format!("a{index}: i64"))
                    .collect::<Vec<_>>()
                    .join(","),
                names.join(",")
            );
        }
        "struct" => {
            return format!(
                "struct S {{ {} }}\nfn probe() -> S = S {{ {} }}\nfn after(x: i64) -> i64 = x\n",
                (0..count)
                    .map(|index| format!("field f{index}: i64"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                (0..count)
                    .map(|index| format!("f{index}: missing_{index}"))
                    .collect::<Vec<_>>()
                    .join(",")
            );
        }
        "match" => {
            return format!(
                "fn probe(x: i64) -> i64 = match x {{ {}_ => 0 }}\nfn after(x: i64) -> i64 = x\n",
                (0..count)
                    .map(|index| format!("{index} => missing_{index},"))
                    .collect::<String>()
            );
        }
        _ => panic!("unknown source family"),
    };
    format!("fn probe() -> Unit {body}\nfn after(x: i64) -> i64 = x\n")
}

#[test]
fn source_sequences_complete_many_roots_once_and_keep_child_facts() {
    for count in [3usize, 0, 1, 2, 8, 32, 128] {
        for mode in [
            "tuple", "array", "map", "block", "shared", "healthy", "call", "struct", "match",
        ] {
            with_resolver(&source(mode, count), |resolver, source| {
                reset();
                let module = resolver.collection.module_for_file(source.file()).unwrap();
                let hir = source.hir().unwrap();
                for &item in hir.roots() {
                    resolver.visit_item(module, source, hir, item).unwrap();
                }
                let poison_count = if mode == "healthy" {
                    0
                } else if mode == "shared" {
                    count.max(1)
                } else {
                    count
                };
                let expected = Work {
                    pairs: usize::from(poison_count > 1) + usize::from(mode == "shared"),
                    includes: if poison_count > 2 {
                        poison_count - 1
                    } else {
                        0
                    },
                    finishes: usize::from(poison_count > 2),
                };
                assert_eq!(work(), expected, "{mode}/{count}");
                assert!(resolver.active_scopes.is_empty());
                let after = resolver
                    .locals
                    .iter()
                    .rev()
                    .find(|local| local.name().text() == "x")
                    .unwrap();
                assert!(resolver.facts.iter().any(|(_, fact)| matches!(fact, crate::SemanticFact::Known(ResolvedName::Local(id)) if *id == after.id())));
                if poison_count != 0 {
                    let expected_roots = if mode == "shared" { 1 } else { count };
                    let poison_facts = resolver
                        .facts
                        .iter()
                        .filter(|(_, fact)| matches!(fact, crate::SemanticFact::Poisoned(_)))
                        .count();
                    assert!(poison_facts >= expected_roots);
                    let snapshot = std::mem::replace(resolver.diagnostics, DiagnosticStore::new())
                        .finalize(None);
                    assert_eq!(
                        snapshot
                            .diagnostics()
                            .iter()
                            .map(|d| d.code().as_str())
                            .collect::<Vec<_>>(),
                        vec!["NAME_NOT_FOUND"; expected_roots]
                    );
                }
            });
        }
    }
}
