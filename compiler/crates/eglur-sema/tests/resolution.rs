use std::collections::BTreeSet;

use eglur_diagnostics::{DiagnosticSnapshot, DiagnosticStore, SourceLabel, SourceSpan};
use eglur_hir::{HirFile, lower};
use eglur_sema::{
    CanonicalTarget, CapabilityDemand, CompileCapability, CompileSupport, DeclarationCollection,
    DeclarationKind, DemandDisposition, ExternalDeclaration, ExternalDefId,
    ExternalNameEnvironment, FactState, ImportAnalysisUnit, ImportResolutionError, ModuleAddress,
    NameKey, NameResolution, PhaseProduct, ResolutionKey, ResolveCapability, ResolvedImports,
    ResolvedName, SemanticFact, SemanticSource, SourceInputError, TypecheckCapability,
    collect_declarations, plan_imports, resolve_imports, resolve_names,
};
use eglur_source::{FileId, LineIndex, SourceFile};
use eglur_syntax::parse_validated;

#[path = "resolution/traversal.rs"]
mod traversal;

#[path = "resolution/pattern_bindings.rs"]
mod pattern_bindings;

struct PreparedSource {
    file: FileId,
    module: ModuleAddress,
    hir: HirFile,
    spans: Vec<(eglur_hir::OriginId, SourceSpan)>,
}

struct CapabilitySet(BTreeSet<CompileCapability>);

impl CapabilitySet {
    fn resolver(include_external: bool) -> Self {
        let mut capabilities = [
            ResolveCapability::CollectDeclarations,
            ResolveCapability::ResolveFileSetImports,
            ResolveCapability::ResolveLexicalValues,
            ResolveCapability::ResolveModuleValues,
            ResolveCapability::ResolveQualifiedValues,
            ResolveCapability::ResolvePatternScopes,
        ]
        .map(CompileCapability::Resolve)
        .into_iter()
        .collect::<BTreeSet<_>>();
        if include_external {
            capabilities.insert(CompileCapability::Resolve(
                ResolveCapability::ResolveTrustedExternalNames,
            ));
        }
        Self(capabilities)
    }

    fn none() -> Self {
        Self(BTreeSet::new())
    }
}

impl CompileSupport for CapabilitySet {
    fn supports(&self, capability: CompileCapability) -> bool {
        self.0.contains(&capability)
    }
}

struct Analysis {
    declarations: DeclarationCollection,
    imports: ResolvedImports,
    names: NameResolution,
    diagnostics: DiagnosticSnapshot,
    demands: Vec<CapabilityDemand>,
}

fn prepare(mut files: Vec<(&str, &str)>) -> Vec<PreparedSource> {
    files.sort_by_key(|(path, _)| *path);
    files
        .into_iter()
        .enumerate()
        .map(|(index, (path, text))| {
            let file = FileId::new(u32::try_from(index).unwrap());
            let source_file = SourceFile::new(file, text.as_bytes().to_vec());
            let validated = source_file.validate().unwrap();
            let lines = LineIndex::new(validated);
            let parse = parse_validated(validated);
            assert!(
                parse.diagnostics().is_empty(),
                "test source {path} did not parse: {:?}",
                parse.diagnostics()
            );
            let hir = lower(&parse);
            assert!(
                hir.invalid_occurrences().is_empty(),
                "test source {path} lowered invalid HIR"
            );
            let label = SourceLabel::new(path).unwrap();
            let spans = hir
                .origins()
                .map(|(origin, value)| {
                    (
                        origin,
                        SourceSpan::from_origin(label.clone(), value, &lines).unwrap(),
                    )
                })
                .collect();
            let module = ModuleAddress::new(
                path.strip_suffix(".eglur")
                    .unwrap()
                    .split('/')
                    .map(Box::<str>::from),
            )
            .unwrap();
            PreparedSource {
                file,
                module,
                hir,
                spans,
            }
        })
        .collect()
}

fn semantic_sources(prepared: &[PreparedSource]) -> Vec<SemanticSource<'_>> {
    prepared
        .iter()
        .map(|source| {
            SemanticSource::new(
                source.file,
                source.module.clone(),
                Some(&source.hir),
                source.spans.clone(),
                [],
                None,
            )
            .unwrap()
        })
        .collect()
}

fn analyze(
    files: Vec<(&str, &str)>,
    units: &[&[u32]],
    support: &CapabilitySet,
    external: Option<&ExternalNameEnvironment>,
) -> Analysis {
    let prepared = prepare(files);
    let sources = semantic_sources(&prepared);
    let mut demands = Vec::new();
    let (product, phase_demands) = collect_declarations(&sources, support)
        .unwrap()
        .into_parts();
    demands.extend(phase_demands);
    let PhaseProduct::Performed(declarations) = product else {
        panic!("test profile must collect declarations")
    };

    let mut diagnostics = DiagnosticStore::new();
    let (product, phase_demands) =
        plan_imports(&declarations, &sources, support, external, &mut diagnostics)
            .unwrap()
            .into_parts();
    demands.extend(phase_demands);
    let PhaseProduct::Performed(plan) = product else {
        panic!("import planning always returns its partial fact table")
    };
    let units = units
        .iter()
        .map(|members| {
            ImportAnalysisUnit::new(members.iter().map(|&file| FileId::new(file))).unwrap()
        })
        .collect::<Vec<_>>();
    let (product, phase_demands) = resolve_imports(
        &declarations,
        &plan,
        &units,
        &sources,
        support,
        external,
        &mut diagnostics,
    )
    .unwrap()
    .into_parts();
    demands.extend(phase_demands);
    let PhaseProduct::Performed(imports) = product else {
        panic!("import resolution always returns its partial fact table")
    };
    let (product, phase_demands) = resolve_names(
        &declarations,
        &imports,
        &sources,
        support,
        external,
        &mut diagnostics,
    )
    .unwrap()
    .into_parts();
    demands.extend(phase_demands);
    let PhaseProduct::Performed(names) = product else {
        panic!("name resolution always returns its partial fact table")
    };
    Analysis {
        declarations,
        imports,
        names,
        diagnostics: diagnostics.finalize(None),
        demands,
    }
}

fn empty_external() -> ExternalNameEnvironment {
    ExternalNameEnvironment::new([], [], []).unwrap()
}

fn module(parts: &[&str]) -> ModuleAddress {
    ModuleAddress::new(parts.iter().copied().map(Box::<str>::from)).unwrap()
}

fn codes(snapshot: &DiagnosticSnapshot) -> Vec<&str> {
    snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect()
}

#[test]
fn collection_has_distinct_module_declaration_and_bang_name_identities() {
    assert!(NameKey::new("value!", false).is_none());
    assert!(ModuleAddress::new([Box::<str>::from("main::nested")]).is_err());
    let external = empty_external();
    let analysis = analyze(
        vec![(
            "main.eglur",
            concat!(
                "fn value() -> i64 = 1\n",
                "fn value!() -> i64 = 2\n",
                "struct Counter { field count: i64 }\n",
            ),
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    let module = analysis.declarations.modules().next().unwrap();
    let declarations = module.declarations().collect::<Vec<_>>();
    assert!(declarations.len() >= 4);
    assert_ne!(declarations[0].id(), declarations[1].id());
    assert_eq!(declarations[0].name().unwrap().spelling(), "value");
    assert_eq!(declarations[1].name().unwrap().spelling(), "value!");
}

#[test]
fn duplicate_callable_parameter_reports_redeclaration_without_an_internal_error() {
    let external = empty_external();
    let analysis = analyze(
        vec![(
            "main.eglur",
            "fn duplicate(value: i64, value: i64) -> i64 = value\n",
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert_eq!(
        codes(&analysis.diagnostics),
        ["LOCAL_BINDING_REDECLARATION"]
    );
}

#[test]
fn duplicate_lambda_parameter_reports_redeclaration_without_an_internal_error() {
    let external = empty_external();
    let analysis = analyze(
        vec![(
            "main.eglur",
            "fn main!() -> Unit {\n  let duplicate = {|value, value| value}\n  yield ()\n}\n",
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert_eq!(
        codes(&analysis.diagnostics),
        ["LOCAL_BINDING_REDECLARATION"]
    );
}

#[test]
fn missing_collection_capability_leaves_the_phase_unperformed() {
    let prepared = prepare(vec![
        ("first.eglur", "fn first() -> Unit = ()\n"),
        ("second.eglur", "fn second() -> Unit = ()\n"),
    ]);
    let sources = prepared
        .iter()
        .map(|source| {
            SemanticSource::new(
                source.file,
                source.module.clone(),
                Some(&source.hir),
                source.spans.clone(),
                [],
                None,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let analysis = collect_declarations(&sources, &CapabilitySet::none()).unwrap();

    assert!(matches!(analysis.product(), PhaseProduct::Unperformed));
    assert_eq!(analysis.demands().len(), 2);
    assert!(
        analysis
            .demands()
            .iter()
            .all(|demand| demand.disposition() == DemandDisposition::Missing)
    );
}

#[test]
fn semantic_source_rejects_duplicate_and_incomplete_origin_projections() {
    let prepared = prepare(vec![("main.eglur", "fn main() -> Unit = ()\n")]);
    let source = &prepared[0];
    let mut duplicate = source.spans.clone();
    duplicate.push(source.spans[0].clone());

    let duplicate_error = SemanticSource::new(
        source.file,
        source.module.clone(),
        Some(&source.hir),
        duplicate,
        [],
        None,
    )
    .err();
    assert_eq!(duplicate_error, Some(SourceInputError::DuplicateOriginSpan));

    let incomplete_error = SemanticSource::new(
        source.file,
        source.module.clone(),
        Some(&source.hir),
        source.spans.iter().skip(1).cloned(),
        [],
        None,
    )
    .err();
    assert_eq!(incomplete_error, Some(SourceInputError::OriginSpanCoverage));
}

#[test]
fn later_local_is_not_hoisted_when_external_names_are_complete() {
    let external = empty_external();
    let analysis = analyze(
        vec![(
            "main.eglur",
            "fn main!() -> Unit {\n  _ = value\n  let value = 1\n}\n",
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert_eq!(codes(&analysis.diagnostics), ["NAME_NOT_FOUND"]);
    assert_eq!(analysis.names.locals().len(), 1);
}

#[test]
fn nested_operators_preserve_scope_and_sibling_diagnostics() {
    let external = empty_external();
    let nested = format!("{}x{}", "x + (".repeat(64), ")".repeat(64));
    let source = format!(
        "fn f(x: i64) -> i64 = {nested}\nfn g(x: i64) -> i64 = ({{ let x = 2\nyield x }}) + x\n"
    );
    let analysis = analyze(
        vec![("main.eglur", &source)],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );
    assert!(analysis.diagnostics.diagnostics().is_empty());
    assert_eq!(analysis.names.locals().len(), 3);

    let source = format!("fn f(x: i64) -> i64 = missing_left + ({nested}) + missing_right\n");
    let analysis = analyze(
        vec![("main.eglur", &source)],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );
    assert_eq!(
        codes(&analysis.diagnostics),
        ["NAME_NOT_FOUND", "NAME_NOT_FOUND"]
    );
}

#[test]
fn redeclaration_precedes_and_suppresses_initializer_lookup() {
    let external = empty_external();
    let analysis = analyze(
        vec![(
            "main.eglur",
            concat!(
                "fn main!() -> Unit {\n",
                "  let value = 1\n",
                "  let value = missing\n",
                "}\n",
            ),
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert_eq!(
        codes(&analysis.diagnostics),
        ["LOCAL_BINDING_REDECLARATION"]
    );
    let diagnostic = &analysis.diagnostics.diagnostics()[0];
    assert_eq!(diagnostic.related().len(), 1);
    assert_eq!(
        diagnostic.related()[0].label(),
        "first binding declared here"
    );
}

#[test]
fn absent_external_environment_is_a_missing_capability_not_a_false_name_error() {
    let analysis = analyze(
        vec![("main.eglur", "fn main() -> i64 = missing\n")],
        &[&[0]],
        &CapabilitySet::resolver(false),
        None,
    );

    assert!(codes(&analysis.diagnostics).is_empty());
    assert!(analysis.demands.iter().any(|demand| {
        demand.capability()
            == CompileCapability::Resolve(ResolveCapability::ResolveTrustedExternalNames)
            && demand.disposition() == DemandDisposition::Missing
    }));
}

#[test]
fn local_import_cycle_uses_one_analysis_unit_and_resolves_collected_targets() {
    let external = empty_external();
    let analysis = analyze(
        vec![
            (
                "a.eglur",
                concat!("import b::right\n", "pub fn left() -> i64 = right()\n",),
            ),
            (
                "b.eglur",
                concat!("import a::left\n", "pub fn right() -> i64 = left()\n",),
            ),
        ],
        &[&[0, 1]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(codes(&analysis.diagnostics).is_empty());
    assert_eq!(analysis.declarations.import_count(), 2);
    for module in analysis.declarations.modules() {
        let import = module.imports().next().unwrap();
        assert!(matches!(
            analysis.imports.fact(import.id()),
            FactState::Known(_)
        ));
    }
    assert!(
        analysis
            .names
            .facts()
            .any(|(_, fact)| { matches!(fact, eglur_sema::SemanticFact::Known(_)) })
    );
}

#[test]
fn import_cycle_cannot_be_split_across_driver_analysis_units() {
    let external = empty_external();
    let prepared = prepare(vec![
        (
            "a.eglur",
            "import b::right\npub fn left() -> i64 = right()\n",
        ),
        (
            "b.eglur",
            "import a::left\npub fn right() -> i64 = left()\n",
        ),
    ]);
    let sources = semantic_sources(&prepared);
    let PhaseProduct::Performed(declarations) =
        collect_declarations(&sources, &CapabilitySet::resolver(true))
            .unwrap()
            .into_parts()
            .0
    else {
        panic!("test profile must collect declarations")
    };
    let mut diagnostics = DiagnosticStore::new();
    let PhaseProduct::Performed(plan) = plan_imports(
        &declarations,
        &sources,
        &CapabilitySet::resolver(true),
        Some(&external),
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("test profile must plan imports")
    };
    let units = [
        ImportAnalysisUnit::new([FileId::new(0)]).unwrap(),
        ImportAnalysisUnit::new([FileId::new(1)]).unwrap(),
    ];

    let error = resolve_imports(
        &declarations,
        &plan,
        &units,
        &sources,
        &CapabilitySet::resolver(true),
        Some(&external),
        &mut diagnostics,
    )
    .err()
    .expect("split cycle must be rejected as an invalid driver schedule");

    assert!(matches!(
        error,
        ImportResolutionError::InvalidScheduleOrder { .. }
    ));
}

#[test]
fn local_declaration_shadows_a_valid_import_binding() {
    let external = empty_external();
    let analysis = analyze(
        vec![
            ("dep.eglur", "pub fn value() -> i64 = 1\n"),
            (
                "main.eglur",
                concat!(
                    "import dep::value\n",
                    "fn value() -> i64 = 2\n",
                    "fn main() -> i64 = value()\n",
                ),
            ),
        ],
        &[&[0], &[1]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(codes(&analysis.diagnostics).is_empty());
}

#[test]
fn duplicate_imported_binding_is_ambiguous_at_use() {
    let external = empty_external();
    let analysis = analyze(
        vec![
            ("left.eglur", "pub fn value() -> i64 = 1\n"),
            (
                "main.eglur",
                concat!(
                    "import left::value as selected\n",
                    "import right::value as selected\n",
                    "fn main() -> i64 = selected()\n",
                ),
            ),
            ("right.eglur", "pub fn value() -> i64 = 2\n"),
        ],
        &[&[0], &[2], &[1]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert_eq!(codes(&analysis.diagnostics), ["NAME_AMBIGUOUS"]);
}

#[test]
fn private_cross_module_import_reports_visibility_once() {
    let external = empty_external();
    let analysis = analyze(
        vec![
            ("dep.eglur", "fn value() -> i64 = 1\n"),
            (
                "main.eglur",
                "import dep::value\nfn main() -> i64 = value\n",
            ),
        ],
        &[&[0], &[1]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert_eq!(codes(&analysis.diagnostics), ["VISIBILITY_VIOLATION"]);
}

#[test]
fn bang_import_alias_preserves_the_effect_marker() {
    let external = empty_external();
    let analysis = analyze(
        vec![
            ("dep.eglur", "pub fn write!() -> Unit = ()\n"),
            (
                "main.eglur",
                "import dep::write! as write\nfn main() -> Unit = ()\n",
            ),
        ],
        &[&[0], &[1]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert_eq!(
        codes(&analysis.diagnostics),
        ["IMPORT_BANG_ALIAS_MUST_BANG"]
    );
}

#[test]
fn enum_variant_path_falls_back_after_module_prefixes_are_exhausted() {
    let external = empty_external();
    let analysis = analyze(
        vec![(
            "main.eglur",
            "enum Choice {\n  Yes\n  No\n}\nfn main() -> Choice = Choice::Yes\n",
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(codes(&analysis.diagnostics).is_empty());
    assert!(analysis.names.facts().any(|(_, fact)| {
        matches!(
            fact,
            SemanticFact::Known(eglur_sema::ResolvedName::Declarations(targets))
                if targets.iter().any(|target| matches!(target, CanonicalTarget::Source(id)
                    if analysis
                        .declarations
                        .declaration(*id)
                        .is_some_and(|declaration| declaration.kind() == DeclarationKind::EnumVariant)))
        )
    }));
}

#[test]
fn match_patterns_resolve_heads_and_share_arm_local_binding_identity() {
    let external = empty_external();
    let analysis = analyze(
        vec![(
            "main.eglur",
            concat!(
                "enum Maybe {\n  Some(i64)\n  None\n}\n",
                "fn value(input: Maybe) -> i64 = match input {\n",
                "  Maybe::Some(inner) => inner,\n",
                "  Maybe::None => 0,\n",
                "}\n",
            ),
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(
        codes(&analysis.diagnostics).is_empty(),
        "unexpected diagnostics: {:?}",
        codes(&analysis.diagnostics)
    );
    assert!(analysis.names.facts().any(|(key, fact)| {
        matches!(key, ResolutionKey::Pattern { .. })
            && matches!(
                fact,
                SemanticFact::Known(ResolvedName::Declarations(targets))
                    if targets.iter().any(|target| matches!(target, CanonicalTarget::Source(id)
                        if analysis
                            .declarations
                            .declaration(*id)
                            .is_some_and(|declaration| declaration.kind() == DeclarationKind::EnumVariant)))
            )
    }));
    let inner = analysis
        .names
        .locals()
        .iter()
        .find(|binding| binding.name().spelling() == "inner")
        .expect("variant payload binding is allocated");
    assert_eq!(inner.origins().len(), 1);
    assert!(analysis.names.facts().any(|(_, fact)| {
        matches!(fact, SemanticFact::Known(ResolvedName::Local(binding)) if binding == &inner.id())
    }));
}

#[test]
fn every_or_pattern_origin_resolves_to_the_shared_local_identity() {
    let external = empty_external();
    let analysis = analyze(
        vec![(
            "main.eglur",
            concat!(
                "enum Choice {\n  Left(i64)\n  Right(i64)\n}\n",
                "fn value(input: Choice) -> i64 = match input {\n",
                "  Choice::Left(value) | Choice::Right(value) => value,\n",
                "}\n",
            ),
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(
        codes(&analysis.diagnostics).is_empty(),
        "unexpected diagnostics: {:?}",
        codes(&analysis.diagnostics)
    );
    let binding = analysis
        .names
        .locals()
        .iter()
        .find(|binding| binding.name().spelling() == "value")
        .expect("or-pattern binding is allocated");
    assert_eq!(binding.origins().len(), 2);
    assert!(binding.origins().iter().all(|&origin| {
        analysis
            .names
            .local_binding(binding.id().module(), binding.id().body(), origin)
            == Some(binding.id())
    }));
}

#[test]
fn imported_enum_alias_preserves_qualified_variant_identity() {
    let external = empty_external();
    let analysis = analyze(
        vec![
            ("dep.eglur", "pub enum Choice {\n  Yes\n  No\n}\n"),
            (
                "main.eglur",
                "import dep::Choice as Selected\nfn main() -> Selected = Selected::Yes\n",
            ),
        ],
        &[&[0], &[1]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(codes(&analysis.diagnostics).is_empty());
    assert!(analysis.names.facts().any(|(_, fact)| {
        matches!(
            fact,
            SemanticFact::Known(eglur_sema::ResolvedName::Declarations(targets))
                if targets.iter().any(|target| matches!(target, CanonicalTarget::Source(id)
                    if analysis
                        .declarations
                        .declaration(*id)
                        .is_some_and(|declaration| declaration.kind() == DeclarationKind::EnumVariant)))
        )
    }));
}

#[test]
fn qualified_braced_forms_resolve_direct_structs_and_owned_variants() {
    let external = empty_external();
    let analysis = analyze(
        vec![
            (
                "dep.eglur",
                concat!(
                    "pub struct Record { pub field value: i64 }\n",
                    "pub enum Choice { Named { value: i64 } }\n",
                ),
            ),
            (
                "main.eglur",
                concat!(
                    "fn record() -> dep::Record = dep::Record { value: 1 }\n",
                    "fn choice() -> dep::Choice = dep::Choice::Named { value: 1 }\n",
                ),
            ),
        ],
        &[&[0], &[1]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(codes(&analysis.diagnostics).is_empty());
    let resolved_kinds = analysis
        .names
        .facts()
        .filter_map(|(_, fact)| match fact {
            SemanticFact::Known(eglur_sema::ResolvedName::Declarations(targets)) => targets.first(),
            _ => None,
        })
        .filter_map(|target| match target {
            CanonicalTarget::Source(id) => analysis.declarations.declaration(*id),
            CanonicalTarget::External(_) => None,
        })
        .map(|declaration| declaration.kind())
        .collect::<BTreeSet<_>>();
    assert!(resolved_kinds.contains(&DeclarationKind::Struct));
    assert!(resolved_kinds.contains(&DeclarationKind::EnumVariant));
}

#[test]
fn type_import_alias_cannot_introduce_reserved_never_name() {
    let external = empty_external();
    let analysis = analyze(
        vec![
            ("dep.eglur", "pub enum Choice {\n  Yes\n}\n"),
            (
                "main.eglur",
                "import dep::Choice as Never\nfn main() -> Unit = ()\n",
            ),
        ],
        &[&[0], &[1]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert_eq!(codes(&analysis.diagnostics), ["RESERVED_BUILTIN_NAME"]);
}

#[test]
fn value_import_alias_may_use_never_spelling() {
    let external = empty_external();
    let analysis = analyze(
        vec![
            ("dep.eglur", "pub fn value() -> i64 = 1\n"),
            (
                "main.eglur",
                "import dep::value as Never\nfn main() -> i64 = Never()\n",
            ),
        ],
        &[&[0], &[1]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(codes(&analysis.diagnostics).is_empty());
}

#[test]
fn qualified_module_lookup_observes_public_reexports() {
    let external = empty_external();
    let analysis = analyze(
        vec![
            ("bridge.eglur", "pub import origin::value\n"),
            ("main.eglur", "fn main() -> i64 = bridge::value()\n"),
            ("origin.eglur", "pub fn value() -> i64 = 1\n"),
        ],
        &[&[2], &[0], &[1]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(codes(&analysis.diagnostics).is_empty());
    assert!(analysis.names.facts().any(|(_, fact)| {
        matches!(
            fact,
            SemanticFact::Known(eglur_sema::ResolvedName::Declarations(targets))
                if matches!(targets.as_ref(), [CanonicalTarget::Source(_)])
        )
    }));
}

#[test]
fn resolver_does_not_claim_declaration_signature_work() {
    let external = empty_external();
    let analysis = analyze(
        vec![("main.eglur", "fn main() -> i64 = 1\n")],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(!analysis.demands.iter().any(|demand| {
        demand.capability()
            == CompileCapability::Typecheck(TypecheckCapability::ResolveDeclarationSignatures)
            && demand.disposition() == DemandDisposition::Missing
    }));
}

#[test]
fn reduce_by_candidate_visits_each_shared_expression_once() {
    let external = empty_external();
    let analysis = analyze(
        vec![(
            "main.eglur",
            concat!(
                "fn id(value: i64) -> i64 = value\n",
                "fn main() -> i64 {\n",
                "  let schema = reduce{ h w -> h }.reduce_by((w, id))\n",
                "  yield 1\n",
                "}\n",
            ),
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(codes(&analysis.diagnostics).is_empty());
    assert!(!analysis.demands.iter().any(|demand| {
        demand.capability()
            == CompileCapability::Typecheck(TypecheckCapability::AnalyzeTensorSource)
    }));
}

#[test]
fn failed_import_poison_suppresses_a_followup_name_error() {
    let external = empty_external();
    let analysis = analyze(
        vec![
            ("dep.eglur", "pub fn present() -> i64 = 1\n"),
            (
                "main.eglur",
                "import dep::missing\nfn main() -> i64 = missing\n",
            ),
        ],
        &[&[0], &[1]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert_eq!(codes(&analysis.diagnostics), ["IMPORT_NOT_FOUND"]);
}

#[test]
fn failed_import_poison_dominates_a_successful_same_spelling_import() {
    let external = empty_external();
    let analysis = analyze(
        vec![
            ("dep.eglur", "pub fn present() -> i64 = 1\n"),
            (
                "main.eglur",
                concat!(
                    "import dep::missing as selected\n",
                    "import dep::present as selected\n",
                    "fn main() -> i64 = selected()\n",
                ),
            ),
        ],
        &[&[0], &[1]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert_eq!(codes(&analysis.diagnostics), ["IMPORT_NOT_FOUND"]);
}

#[test]
fn unperformed_import_planning_does_not_become_a_language_error() {
    let external = empty_external();
    let mut support = CapabilitySet::resolver(true);
    support.0.remove(&CompileCapability::Resolve(
        ResolveCapability::ResolveFileSetImports,
    ));
    let analysis = analyze(
        vec![
            ("dep.eglur", "pub fn value() -> i64 = 1\n"),
            (
                "main.eglur",
                "import dep::value\nfn main() -> i64 = value\n",
            ),
        ],
        &[&[0], &[1]],
        &support,
        Some(&external),
    );

    assert!(codes(&analysis.diagnostics).is_empty());
    assert!(analysis.demands.iter().any(|demand| {
        demand.capability() == CompileCapability::Resolve(ResolveCapability::ResolveFileSetImports)
            && demand.disposition() == DemandDisposition::Missing
    }));
}

#[test]
fn missing_external_catalog_does_not_become_an_import_or_name_error() {
    let analysis = analyze(
        vec![(
            "main.eglur",
            "import std::parse::value\nfn main() -> i64 = value\n",
        )],
        &[&[0]],
        &CapabilitySet::resolver(false),
        None,
    );

    assert!(codes(&analysis.diagnostics).is_empty());
    assert!(analysis.demands.iter().any(|demand| {
        demand.capability()
            == CompileCapability::Resolve(ResolveCapability::ResolveTrustedExternalNames)
            && demand.disposition() == DemandDisposition::Missing
    }));
}

#[test]
fn std_imports_never_fall_back_to_a_same_named_source_module() {
    let external = empty_external();
    let analysis = analyze(
        vec![
            (
                "main.eglur",
                "import std::parse::value\nfn main() -> i64 = value\n",
            ),
            ("std/parse.eglur", "pub fn value() -> i64 = 1\n"),
        ],
        &[&[0], &[1]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert_eq!(codes(&analysis.diagnostics), ["IMPORT_NOT_FOUND"]);
}

#[test]
fn public_self_reexport_cannot_expose_a_private_declaration() {
    let external = empty_external();
    let analysis = analyze(
        vec![(
            "main.eglur",
            concat!(
                "fn value() -> i64 = 1\n",
                "pub import self::value\n",
                "fn main() -> i64 = value()\n",
            ),
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert_eq!(codes(&analysis.diagnostics), ["VISIBILITY_VIOLATION"]);
}

#[test]
fn external_environment_represents_empty_modules_explicitly() {
    let empty = module(&["std", "empty"]);
    let environment = ExternalNameEnvironment::new([empty.clone()], [], []).unwrap();

    assert!(environment.has_module(&empty));
}

#[test]
fn external_callable_families_resolve_without_a_false_ambiguity() {
    let parse_module = module(&["std", "parse"]);
    let first = ExternalDefId::new("std.parse.value.i64").unwrap();
    let second = ExternalDefId::new("std.parse.value.f64").unwrap();
    let declarations = [
        ExternalDeclaration::new(
            first.clone(),
            parse_module.clone(),
            NameKey::new("value", false).unwrap(),
            DeclarationKind::Function,
        )
        .unwrap(),
        ExternalDeclaration::new(
            second.clone(),
            parse_module.clone(),
            NameKey::new("value", false).unwrap(),
            DeclarationKind::Function,
        )
        .unwrap(),
    ];
    let external =
        ExternalNameEnvironment::new([parse_module], declarations, [first, second]).unwrap();
    let analysis = analyze(
        vec![("main.eglur", "fn main() -> i64 = value\n")],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(codes(&analysis.diagnostics).is_empty());
    assert!(analysis.names.facts().any(|(_, fact)| {
        matches!(
            fact,
            SemanticFact::Known(eglur_sema::ResolvedName::Declarations(targets))
                if targets.len() == 2
                    && targets
                        .iter()
                        .all(|target| matches!(target, CanonicalTarget::External(_)))
        )
    }));
}

#[test]
fn two_segment_external_module_path_precedes_variant_fallback() {
    let math = module(&["math"]);
    let sine = ExternalDefId::new("math.sin").unwrap();
    let declaration = ExternalDeclaration::new(
        sine,
        math.clone(),
        NameKey::new("sin", false).unwrap(),
        DeclarationKind::Function,
    )
    .unwrap();
    let external = ExternalNameEnvironment::new([math], [declaration], []).unwrap();
    let analysis = analyze(
        vec![("main.eglur", "fn main() -> i64 = math::sin\n")],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(codes(&analysis.diagnostics).is_empty());
    assert!(analysis.names.facts().any(|(_, fact)| {
        matches!(
            fact,
            SemanticFact::Known(eglur_sema::ResolvedName::Declarations(targets))
                if matches!(targets.as_ref(), [CanonicalTarget::External(_)])
        )
    }));
}

#[test]
fn qualified_braced_external_path_selects_the_type_namespace() {
    let geometry = module(&["geometry"]);
    let point_type = ExternalDefId::new("geometry.Point.type").unwrap();
    let point_value = ExternalDefId::new("geometry.Point.value").unwrap();
    let declarations = [
        ExternalDeclaration::new(
            point_type.clone(),
            geometry.clone(),
            NameKey::new("Point", false).unwrap(),
            DeclarationKind::Struct,
        )
        .unwrap(),
        ExternalDeclaration::new(
            point_value,
            geometry.clone(),
            NameKey::new("Point", false).unwrap(),
            DeclarationKind::Function,
        )
        .unwrap(),
    ];
    let external = ExternalNameEnvironment::new([geometry], declarations, []).unwrap();
    let analysis = analyze(
        vec![("main.eglur", "fn main() -> Unit = geometry::Point {}\n")],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(codes(&analysis.diagnostics).is_empty());
    assert!(analysis.names.facts().any(|(_, fact)| {
        matches!(
            fact,
            SemanticFact::Known(eglur_sema::ResolvedName::Declarations(targets))
                if matches!(targets.as_ref(), [CanonicalTarget::External(target)]
                    if target == &point_type)
        )
    }));
}

#[test]
fn callable_family_resolution_does_not_claim_signature_validation() {
    let external = empty_external();
    let analysis = analyze(
        vec![(
            "main.eglur",
            concat!(
                "fn choose(value: i64) -> i64 = value\n",
                "fn choose(value: f64) -> i64 = 0\n",
                "fn main() -> i64 = choose\n",
            ),
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(codes(&analysis.diagnostics).is_empty());
    assert!(!analysis.demands.iter().any(|demand| {
        demand.capability()
            == CompileCapability::Typecheck(TypecheckCapability::ResolveDeclarationSignatures)
            && demand.disposition() == DemandDisposition::Missing
    }));
}

#[test]
fn unresolved_assignment_target_is_not_applicable_to_ordinary_lookup() {
    let external = empty_external();
    let analysis = analyze(
        vec![(
            "main.eglur",
            "fn main!() -> Unit {\n  missing = 1\n  yield ()\n}\n",
        )],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );

    assert!(codes(&analysis.diagnostics).is_empty());
    assert!(
        analysis
            .names
            .facts()
            .any(|(_, fact)| matches!(fact, SemanticFact::NotApplicable))
    );
}
