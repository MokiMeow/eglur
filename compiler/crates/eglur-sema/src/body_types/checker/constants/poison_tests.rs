use super::*;
use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticSnapshot, PrimarySpan, Severity, SourceLabel,
};
use eglur_source::{LineIndex, SourceFile};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    inputs: usize,
    completed: usize,
}
thread_local! { static WORK:Cell<[Work;2]> = const {Cell::new([Work {inputs:0,completed:0};2])}; }
pub(in crate::body_types::checker) enum Step {
    Input(usize),
    Complete(usize),
}
pub(in crate::body_types::checker) fn observe(step: Step) {
    WORK.update(|mut w| {
        match step {
            Step::Input(stage) => w[stage].inputs += 1,
            Step::Complete(stage) => w[stage].completed += 1,
        };
        w
    });
}
fn reset() {
    WORK.set([Work::default(); 2]);
}
fn work(stage: usize) -> Work {
    WORK.get()[stage]
}
struct Support;
impl CompileSupport for Support {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}
fn performed<T>(phase: PhaseAnalysis<T>) -> T {
    let PhaseProduct::Performed(value) = phase.into_parts().0 else {
        panic!("phase not performed")
    };
    value
}
fn with_checker(text: &str, inspect: impl FnOnce(&mut BodyChecker<'_, '_, '_>)) {
    let file = FileId::new(0);
    let raw = SourceFile::new(file, text.as_bytes().to_vec());
    let validated = raw.validate().unwrap();
    let lines = LineIndex::new(validated);
    let parsed = eglur_syntax::parse_validated(validated);
    assert!(parsed.diagnostics().is_empty());
    let hir = eglur_hir::lower(&parsed);
    assert!(hir.invalid_occurrences().is_empty());
    let label = SourceLabel::new("main.eglur").unwrap();
    let source = SemanticSource::new(
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
    .unwrap();
    let sources = [source];
    let mut diagnostics = DiagnosticStore::new();
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    let collection = performed(crate::collect_declarations(&sources, &Support).unwrap());
    let plan = performed(
        crate::plan_imports(
            &collection,
            &sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let unit = crate::ImportAnalysisUnit::new([file]).unwrap();
    let imports = performed(
        crate::resolve_imports(
            &collection,
            &plan,
            &[unit],
            &sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let names = performed(
        crate::resolve_names(
            &collection,
            &imports,
            &sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let mut declarations = performed(
        crate::resolve_declaration_signatures(
            &collection,
            &imports,
            &sources,
            &Support,
            crate::SignatureResolutionInputs::new(Some(&external), None),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let mut trait_environment = performed(
        crate::build_trait_environment(
            &collection,
            &sources,
            &Support,
            declarations.signatures_mut(),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let (signatures, inhabitance) = declarations.parts_mut();
    let mut checker = BodyChecker {
        collection: &collection,
        imports: &imports,
        names: &names,
        external_names: Some(&external),
        sources: BTreeMap::from([(file, &sources[0])]),
        signatures,
        inhabitance,
        trait_environment: &mut trait_environment,
        diagnostics: &mut diagnostics,
        recorder: DemandRecorder::new(&Support),
        expressions: FactTable::new(),
        const_eligibility: BTreeMap::new(),
        preparing_numeric_tree: false,
        evaluate_constant_declarations: true,
        locals: FactTable::new(),
        members: FactTable::new(),
        invocations: FactTable::new(),
        callable_values: FactTable::new(),
        reduce_by_commitments: FactTable::new(),
        regions: FactTable::new(),
        checked_numeric: FactTable::new(),
        ownership_operations: FactTable::new(),
        bodies: FactTable::new(),
        transfers: Vec::new(),
        callable_instances: Vec::new(),
        callable_instance_ids: BTreeMap::new(),
        obligations: Vec::new(),
        method_dispatches: Vec::new(),
        region_stack: Vec::new(),
        block_region_overrides: Vec::new(),
        result_contexts: Vec::new(),
        question_floors: vec![0],
        checked_contexts: Vec::new(),
        control_path_barriers: Vec::new(),
        current_return: None,
        current_owner: None,
    };
    inspect(&mut checker);
}
fn ids(checker: &BodyChecker<'_, '_, '_>) -> Vec<DefId> {
    checker
        .collection
        .modules()
        .flat_map(|m| m.declarations())
        .filter(|d| d.kind() == DeclarationKind::Const)
        .map(|d| d.id())
        .collect()
}
fn root(store: &mut DiagnosticStore, i: usize) -> Poison {
    let diagnostic = Diagnostic::new(
        DiagnosticCode::new(format!("ROOT_{i:03}")).unwrap(),
        Severity::Error,
        DiagnosticPhase::new("type").unwrap(),
        "independent failure",
        PrimarySpan::Unavailable,
    );
    let root = DiagnosticRoot::boundary(
        BoundaryRootKey::new(format!("root-{i}")).unwrap(),
        diagnostic,
    )
    .unwrap();
    store
        .record_failure(eglur_diagnostics::DiagnosticFailure::Root(root))
        .unwrap()
}
fn union(inputs: &[Poison], indices: impl IntoIterator<Item = usize>) -> Poison {
    let mut pending = eglur_diagnostics::PoisonAccumulator::new();
    for i in indices {
        pending.include(&inputs[i]).unwrap();
    }
    pending.finish().unwrap()
}

fn declaration(checker: &BodyChecker<'_, '_, '_>, name: &str) -> DefId {
    checker
        .collection
        .modules()
        .flat_map(|m| m.declarations())
        .find(|d| d.name().is_some_and(|n| n.text() == name))
        .unwrap()
        .id()
}
fn body_poison(checker: &BodyChecker<'_, '_, '_>, declaration: DefId) -> Poison {
    let FactState::Poisoned(poison) = checker.bodies.state(&declaration) else {
        panic!("poisoned body expected")
    };
    poison.clone()
}
fn assert_selected(
    checker: &mut BodyChecker<'_, '_, '_>,
    poison: &Poison,
    all: &Poison,
    count: usize,
    code: &str,
) {
    checker
        .diagnostics
        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
        .unwrap();
    checker
        .diagnostics
        .select_preceding_roots(poison, all)
        .unwrap();
    let store = std::mem::replace(checker.diagnostics, DiagnosticStore::new());
    let snapshot = store.finalize(None);
    assert!(matches!(snapshot, DiagnosticSnapshot::Rejected(_)));
    assert_eq!(snapshot.diagnostics().len(), count);
    assert!(
        snapshot
            .diagnostics()
            .iter()
            .all(|d| d.code().as_str() == code)
    );
}
#[test]
fn known_constant_folds_do_not_complete_provenance() {
    with_checker(
        "const Base: i8 = 0\nconst Sink: i8 = Base\nconst After: i8 = 1\n",
        |checker| {
            reset();
            let declarations = ids(checker);
            checker.check_constant_declarations(&declarations).unwrap();
            assert_eq!(WORK.get(), [Work::default(); 2]);
            for d in declarations {
                assert!(matches!(checker.bodies.state(&d), FactState::Known(_)));
            }
        },
    );
}
#[test]
fn initializer_name_failures_complete_once_before_value_typing() {
    for n in [8_usize, 32, 128] {
        for mode in ["distinct", "repeated", "reversed", "mixed"] {
            let mut names = (0..n)
                .map(|i| {
                    if mode == "repeated" {
                        "missing".into()
                    } else if mode == "mixed" && i % 2 == 0 {
                        "1".into()
                    } else {
                        format!("missing{i}")
                    }
                })
                .collect::<Vec<String>>();
            if mode == "reversed" {
                names.reverse();
            }
            let text = format!(
                "const Sink: i8 = {}\nconst After: i8 = 1\n",
                names.join(" + ")
            );
            with_checker(&text, |checker| {
                let sink = declaration(checker, "Sink");
                let after = declaration(checker, "After");
                let all = checker
                    .names
                    .facts()
                    .filter_map(|(_, v)| {
                        if let SemanticFact::Poisoned(p) = v {
                            Some(p.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                let expected = if mode == "mixed" { n / 2 } else { n };
                assert_eq!(all.len(), expected);
                reset();
                checker.check_constant_declarations(&ids(checker)).unwrap();
                assert_eq!(
                    work(0),
                    Work {
                        inputs: expected,
                        completed: 1
                    }
                );
                assert_eq!(work(1), Work::default());
                let p = body_poison(checker, sink);
                assert!(matches!(checker.bodies.state(&after), FactState::Known(_)));
                let (_, _, hir, item) = checker.source_item(sink).unwrap();
                let ItemKind::Const(value) = item.kind() else {
                    panic!("const")
                };
                let key = ExpressionKey::new(sink.module(), value.value);
                assert!(matches!(
                    checker.expressions.state(&key),
                    FactState::Unperformed
                ));
                assert!(hir.body(value.body).expression(value.value).is_some());
                let all = union(&all, 0..all.len());
                assert_selected(checker, &p, &all, expected, "NAME_NOT_FOUND");
            });
        }
    }
}
#[test]
fn seeded_dependency_failures_complete_once_and_preserve_later_body() {
    for n in [8_usize, 32, 128] {
        for mode in ["distinct", "repeated", "overlap", "wide", "reversed"] {
            let mut references = (0..n).map(|i| format!("D{i}")).collect::<Vec<_>>();
            if mode == "reversed" {
                references.reverse();
            }
            let text = format!(
                "{}const Sink: i8 = {}\nconst After: i8 = 1\n",
                (0..n)
                    .map(|i| format!("const D{i}: i8 = 0\n"))
                    .collect::<String>(),
                references.join(" + ")
            );
            with_checker(&text, |checker| {
                let declarations = ids(checker);
                let sink = declarations[n];
                let roots = (0..n)
                    .map(|i| root(checker.diagnostics, i))
                    .collect::<Vec<_>>();
                let all = union(&roots, 0..n);
                let left = union(&roots, 0..n / 2 + 1);
                let right = union(&roots, n / 2..n);
                for (i, &d) in declarations[..n].iter().enumerate() {
                    let poison = match mode {
                        "repeated" => roots[0].clone(),
                        "wide" => all.clone(),
                        "overlap" => {
                            if i % 2 == 0 {
                                left.clone()
                            } else {
                                right.clone()
                            }
                        }
                        _ => roots[i].clone(),
                    };
                    checker
                        .bodies
                        .insert(d, SemanticFact::Poisoned(poison))
                        .unwrap();
                }
                reset();
                checker.check_constant_declarations(&declarations).unwrap();
                assert_eq!(work(0), Work::default());
                assert_eq!(
                    work(1),
                    Work {
                        inputs: n,
                        completed: 1
                    }
                );
                assert!(matches!(
                    checker.bodies.state(&declarations[n + 1]),
                    FactState::Known(_)
                ));
                let p = body_poison(checker, sink);
                let selected = if mode == "repeated" { 1 } else { n };
                checker
                    .diagnostics
                    .record_failure(DiagnosticFailure::Derivative(p.clone()))
                    .unwrap();
                checker
                    .diagnostics
                    .select_preceding_roots(&p, &all)
                    .unwrap();
                let store = std::mem::replace(checker.diagnostics, DiagnosticStore::new());
                let snapshot = store.finalize(None);
                assert_eq!(
                    snapshot
                        .diagnostics()
                        .iter()
                        .map(|d| d.code().as_str().to_owned())
                        .collect::<Vec<_>>(),
                    (0..selected)
                        .map(|i| format!("ROOT_{i:03}"))
                        .collect::<Vec<_>>()
                );
            });
        }
    }
}
#[test]
fn foreign_dependency_failure_keeps_prior_facts_and_skips_completion() {
    for n in [8_usize, 32, 128] {
        for foreign_first in [false, true] {
            let text = format!(
                "{}const Sink: i8 = {}\nconst After: i8 = 1\n",
                (0..n)
                    .map(|i| format!("const D{i}: i8 = 0\n"))
                    .collect::<String>(),
                (0..n)
                    .map(|i| format!("D{i}"))
                    .collect::<Vec<_>>()
                    .join(" + ")
            );
            with_checker(&text, |checker| {
                let declarations = ids(checker);
                let mut foreign = DiagnosticStore::new();
                let own = root(checker.diagnostics, 0);
                let other = root(&mut foreign, 1);
                for (i, &d) in declarations[..n].iter().enumerate() {
                    let poison = if i == usize::from(!foreign_first) {
                        other.clone()
                    } else {
                        own.clone()
                    };
                    checker
                        .bodies
                        .insert(d, SemanticFact::Poisoned(poison))
                        .unwrap();
                }
                reset();
                let result = checker.check_constant_declarations(&declarations);
                assert!(matches!(
                    result,
                    Err(BodyTypingError::DiagnosticStore(
                        eglur_diagnostics::DiagnosticStoreError::ForeignPoison
                    ))
                ));
                assert_eq!(work(0), Work::default());
                assert_eq!(
                    work(1),
                    Work {
                        inputs: 2,
                        completed: 0
                    }
                );
                assert!(matches!(
                    checker.bodies.state(&declarations[n]),
                    FactState::Unperformed
                ));
                assert!(matches!(
                    checker.bodies.state(&declarations[n + 1]),
                    FactState::Unperformed
                ));
                for (i, &d) in declarations[..n].iter().enumerate() {
                    let p = body_poison(checker, d);
                    let result = checker
                        .diagnostics
                        .record_failure(DiagnosticFailure::Derivative(p));
                    if i == usize::from(!foreign_first) {
                        assert!(matches!(
                            result,
                            Err(eglur_diagnostics::DiagnosticStoreError::ForeignPoison)
                        ));
                    } else {
                        assert!(result.is_ok());
                    }
                }
            });
        }
    }
}
#[test]
fn inherited_cycles_precede_forward_references_and_preserve_later_constants() {
    for n in [8_usize, 32, 128] {
        let text = format!(
            "{}const Sink: i8 = {} + Later\nconst Later: i8 = 1\nconst After: i8 = 1\n",
            (0..n)
                .map(|i| format!("const C{i}: i8 = C{}\n", (i + 1) % n))
                .collect::<String>(),
            (0..n)
                .map(|i| format!("C{i}"))
                .collect::<Vec<_>>()
                .join(" + ")
        );
        with_checker(&text, |checker| {
            reset();
            checker.check_constant_declarations(&ids(checker)).unwrap();
            assert_eq!(work(0), Work::default());
            assert_eq!(
                work(1),
                Work {
                    inputs: n,
                    completed: 1
                }
            );
            assert!(matches!(
                checker.bodies.state(&declaration(checker, "After")),
                FactState::Known(_)
            ));
            let sink = body_poison(checker, declaration(checker, "Sink"));
            let roots = (0..n)
                .map(|i| body_poison(checker, declaration(checker, &format!("C{i}"))))
                .collect::<Vec<_>>();
            let all = union(&roots, 0..n);
            assert_selected(checker, &sink, &all, n, "CONST_CYCLE");
        });
    }
}
#[test]
fn forward_reference_priority_discards_inherited_roots_after_original_gate() {
    for n in [8_usize, 32, 128] {
        let text = format!(
            "const Sink: i8 = {}\n{}const After: i8 = 1\n",
            (0..n)
                .map(|i| format!("D{i}"))
                .collect::<Vec<_>>()
                .join(" + "),
            (0..n)
                .map(|i| format!("const D{i}: i8 = 127 + 1\n"))
                .collect::<String>()
        );
        with_checker(&text, |checker| {
            reset();
            checker.check_constant_declarations(&ids(checker)).unwrap();
            assert_eq!(work(0), Work::default());
            assert_eq!(
                work(1),
                Work {
                    inputs: n,
                    completed: 1
                }
            );
            assert!(matches!(
                checker.bodies.state(&declaration(checker, "After")),
                FactState::Known(_)
            ));
            let sink = body_poison(checker, declaration(checker, "Sink"));
            let mut all = (0..n)
                .map(|i| body_poison(checker, declaration(checker, &format!("D{i}"))))
                .collect::<Vec<_>>();
            all.push(sink.clone());
            let all = union(&all, 0..all.len());
            assert_selected(checker, &sink, &all, 1, "CONST_FORWARD_REFERENCE");
        });
    }
}
