use eglur_diagnostics::DiagnosticSnapshot;
use eglur_driver::{
    AnalysisDependencyFingerprint, AnalysisFingerprints, CompilerSession, FileInput,
    FileSetRequest, SessionOptions, SupportProfile, SupportProfileId, SupportSchemaVersion,
    TrustedCatalog,
};
use eglur_sema::{
    CompileCapability, ModuleAddress, ModuleSemanticFingerprints, ResolveCapability,
    SemanticFingerprints, TypecheckCapability,
};

#[path = "fingerprints/traversal.rs"]
mod traversal;

#[path = "fingerprints/binder_contexts.rs"]
mod binder_contexts;

#[path = "fingerprints/reexports.rs"]
mod reexports;

#[derive(Clone, Debug, Eq, PartialEq)]
struct FingerprintSnapshot {
    analysis: AnalysisFingerprints,
    semantics: SemanticFingerprints,
}

fn profile() -> SupportProfile {
    SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("fingerprint-tests-v1").unwrap(),
        [
            CompileCapability::Resolve(ResolveCapability::CollectDeclarations),
            CompileCapability::Resolve(ResolveCapability::ResolveFileSetImports),
            CompileCapability::Resolve(ResolveCapability::ResolveLexicalValues),
            CompileCapability::Resolve(ResolveCapability::ResolveModuleValues),
            CompileCapability::Resolve(ResolveCapability::ResolveQualifiedValues),
            CompileCapability::Resolve(ResolveCapability::ResolvePatternScopes),
            CompileCapability::Resolve(ResolveCapability::ResolveTrustedExternalNames),
            CompileCapability::Typecheck(TypecheckCapability::ResolveDeclarationSignatures),
            CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEnvironment),
            CompileCapability::Typecheck(TypecheckCapability::CheckBodies),
            CompileCapability::Typecheck(TypecheckCapability::ResolveMembers),
            CompileCapability::Typecheck(TypecheckCapability::ResolveMethods),
            CompileCapability::Typecheck(TypecheckCapability::ResolveOverloads),
            CompileCapability::Typecheck(TypecheckCapability::AnalyzeInhabitance),
            CompileCapability::Typecheck(TypecheckCapability::CheckFailureFlow),
            CompileCapability::Typecheck(
                TypecheckCapability::CheckSelectedCheckedIntegerArithmetic,
            ),
            CompileCapability::Typecheck(TypecheckCapability::CheckBranching),
            CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEvidence),
            CompileCapability::Typecheck(TypecheckCapability::ComputeEffectSummaries),
        ],
    )
    .unwrap()
}

fn file(path: &str, source: &str) -> FileInput {
    FileInput::new(path, source.as_bytes().to_vec())
}

fn module(name: &str) -> ModuleAddress {
    ModuleAddress::new(name.split("::").map(Box::<str>::from)).unwrap()
}

fn resolve(
    files: impl IntoIterator<Item = FileInput>,
    entry: &str,
    observe: bool,
) -> FingerprintSnapshot {
    let files = files.into_iter().collect::<Vec<_>>();
    let root = (files.len() > 1).then_some(".");
    let mut session = CompilerSession::build(
        FileSetRequest::new(files, entry, root).unwrap(),
        SessionOptions::new(observe),
    )
    .unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let snapshot = {
        let analysis = session.resolve(&profile(), Some(&catalog)).unwrap();
        FingerprintSnapshot {
            analysis: analysis
                .fingerprints()
                .expect("complete semantics produce analysis inputs")
                .clone(),
            semantics: analysis
                .resolution()
                .expect("complete profile resolves semantics")
                .fingerprints()
                .expect("complete semantics produce output fingerprints")
                .clone(),
        }
    };
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
    snapshot
}

fn outputs<'a>(snapshot: &'a FingerprintSnapshot, name: &str) -> &'a ModuleSemanticFingerprints {
    snapshot
        .semantics
        .module(&module(name))
        .unwrap_or_else(|| panic!("missing semantic outputs for {name}"))
}

fn unit_key(snapshot: &FingerprintSnapshot, name: &str) -> eglur_driver::AnalysisInputKey {
    snapshot
        .analysis
        .unit_containing(&module(name))
        .unwrap_or_else(|| panic!("missing analysis unit for {name}"))
        .key()
}

fn assert_global_conformance_dependency(snapshot: &FingerprintSnapshot) {
    let expected = snapshot.analysis.conformance().fingerprint();
    for unit in snapshot.analysis.units() {
        let dependencies = unit
            .dependencies()
            .iter()
            .filter_map(|dependency| match dependency {
                AnalysisDependencyFingerprint::GlobalConformance(fingerprint) => Some(*fingerprint),
                AnalysisDependencyFingerprint::Import(_) => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(dependencies, [expected]);
    }
}

#[test]
fn bound_priority_does_not_change_semantic_contract_or_evidence_fingerprints() {
    let resolve_bounds = |bounds: &str| {
        let source = format!(
            concat!(
                "pub trait Alpha {{ fn alpha(self: &Self) -> i64 }}\n",
                "pub trait Zulu {{ fn zulu(self: &Self) -> i64 }}\n",
                "pub fn choose[T: {}](value: T) -> i64 = value.zulu()\n",
                "pub fn forward[T: {}](value: T) -> i64 = choose(value)\n",
            ),
            bounds, bounds,
        );
        resolve([file("main.eglur", &source)], "main.eglur", false)
    };
    let forward = resolve_bounds("Zulu + Alpha");
    let reverse = resolve_bounds("Alpha + Zulu");
    assert_eq!(forward.semantics, reverse.semantics);
}

#[test]
fn overload_fingerprints_preserve_function_and_receiver_families_under_reordering() {
    let families = [
        [
            "pub overload fn choose(value: Bool) -> Bool = value\n",
            "pub overload fn choose(value: Str) -> Str = value\n",
        ],
        [
            "pub overload fn parse(self: Self, value: Bool) -> Bool = value\n",
            "pub overload fn parse(self: &Self, value: Bool) -> Bool = value\n",
        ],
    ];
    for (index, family) in families.into_iter().enumerate() {
        let source = |members: [&str; 2]| {
            let members = members.concat();
            if index == 0 {
                format!(
                    "{members}\n\
                     pub fn boolean(value: Bool) -> Bool = choose(value)\n\
                     pub fn text(value: Str) -> Str = choose(value)\n"
                )
            } else {
                format!("pub struct Parser {{\n{members}}}\n")
            }
        };
        let ordered = resolve([file("main.eglur", &source(family))], "main.eglur", false);
        let reversed = resolve(
            [file("main.eglur", &source([family[1], family[0]]))],
            "main.eglur",
            false,
        );
        assert_eq!(ordered.semantics, reversed.semantics);
    }
}

#[test]
fn qualified_trait_aliases_share_the_same_body_fingerprint() {
    let resolve_alias = |import: &str, owner: &str| {
        let main = format!(
            concat!(
                "import {import}\nimport dep::Count\n",
                "pub fn score(value: &Count) -> i64 = {owner}.rank(value)\n",
                "pub fn from_value(value: &Count) -> i64 {{\n",
                "  let f: Fn(&Count) -> i64 = {owner}.rank\n",
                "  yield f(value)\n",
                "}}\n",
            ),
            import = import,
            owner = owner,
        );
        resolve(
            [
                file(
                    "dep.eglur",
                    concat!(
                        "pub trait Rank { fn rank(self: &Self) -> i64 }\n",
                        "pub struct Count {}\n",
                        "impl Rank for Count { fn rank(self: &Self) -> i64 = 3 }\n",
                    ),
                ),
                file("bridge.eglur", "pub import dep::Rank as PublicRank\n"),
                file("main.eglur", &main),
            ],
            "main.eglur",
            false,
        )
    };

    let direct = resolve_alias("dep::Rank", "Rank");
    let reexported = resolve_alias("bridge::PublicRank", "PublicRank");
    assert_eq!(
        outputs(&direct, "main").body(),
        outputs(&reexported, "main").body()
    );
}

#[test]
fn normalized_owner_evidence_fingerprints_ignore_declaration_order() {
    let snapshot = |prefix: &str| {
        let source = format!(
            "{prefix}\n\
             pub trait Carries {{ type Item }}\npub struct Carrier {{}}\n\
             impl Carries for Carrier {{ type Item = i64 }}\n\
             pub type Item[T: Carries] = T::Item\n\
             pub fn accept[T: Link[i64]](x: T) -> Unit = ()\n\
             pub fn forward[T: Mark + Link[Item[Carrier]]](x: T) -> Unit = accept(x)\n"
        );
        resolve([file("main.eglur", &source)], "main.eglur", false)
    };
    let first = snapshot("pub trait Mark {}\npub trait Link[A] {}\npub struct Unrelated {}");
    let reordered = snapshot("pub struct Unrelated {}\npub trait Link[A] {}\npub trait Mark {}");
    assert_eq!(first.semantics, reordered.semantics);
}

#[test]
fn grouped_call_fingerprints_preserve_trivia_and_source_label_invariance() {
    let source = "fn id(value: i64) -> i64 = value\npub fn value() -> i64 = ((id))(7)\n";
    let base = resolve([file("main.eglur", source)], "main.eglur", false);
    let labeled = resolve(
        [FileInput::with_source_label(
            "main.eglur",
            "generated:grouped-call",
            source.as_bytes().to_vec(),
        )],
        "main.eglur",
        true,
    );
    assert_eq!(base, labeled);

    let spaced = resolve(
        [file(
            "main.eglur",
            "fn id(value: i64) -> i64 = value\n\npub fn value() -> i64 = (\n ( /* callee */ id )\n)(7)\n",
        )],
        "main.eglur",
        false,
    );
    assert_eq!(base.semantics, spaced.semantics);
    assert_ne!(base.analysis, spaced.analysis);
}

#[test]
fn shuffled_source_insertion_and_observation_do_not_change_fingerprints() {
    let files = [
        file("a.eglur", "pub fn left() -> i64 = 1\n"),
        file("z.eglur", "pub fn right() -> i64 = 2\n"),
    ];
    let mut reversed = files.clone();
    reversed.reverse();

    let canonical = resolve(files, "a.eglur", false);
    let shuffled = resolve(reversed, "a.eglur", true);

    assert_eq!(canonical, shuffled);
}

#[test]
fn receiver_passing_fingerprints_ignore_labels_and_trivia_but_retain_call_semantics() {
    let declarations =
        "pub struct Counter { field value: i64\nfn read(self: &Self) -> i64 = self.value\n}\n";
    let source = format!("{declarations}pub fn inspect(value: Counter) -> i64 = value.read()\n");
    let base = resolve([file("main.eglur", &source)], "main.eglur", false);
    let labeled = resolve(
        [FileInput::with_source_label(
            "main.eglur",
            "generated:receiver",
            source.as_bytes().to_vec(),
        )],
        "main.eglur",
        true,
    );
    assert_eq!(base, labeled);

    let spaced = resolve(
        [file(
            "main.eglur",
            &format!(
                "{declarations}\npub fn inspect(value: Counter) -> i64 = value /* borrow */ .read()\n"
            ),
        )],
        "main.eglur",
        false,
    );
    assert_eq!(base.semantics, spaced.semantics);
    assert_ne!(base.analysis, spaced.analysis);

    let temporary = resolve(
        [file(
            "main.eglur",
            &format!(
                "{declarations}pub fn inspect(value: Counter) -> i64 = (Counter {{ value: value.value }}).read()\n"
            ),
        )],
        "main.eglur",
        false,
    );
    assert_ne!(outputs(&base, "main"), outputs(&temporary, "main"));
}

#[test]
fn diagnostic_source_labels_do_not_change_semantic_fingerprints() {
    let source = "pub fn value() -> i64 = 1\n";
    let default = resolve([file("main.eglur", source)], "main.eglur", false);
    let labeled = resolve(
        [FileInput::with_source_label(
            "main.eglur",
            "generated:core/value",
            source.as_bytes().to_vec(),
        )],
        "main.eglur",
        false,
    );

    assert_eq!(default, labeled);
}

#[test]
fn trivia_changes_exact_inputs_but_not_semantic_outputs() {
    let compact = resolve(
        [file("main.eglur", "pub fn value() -> i64 = 1\n")],
        "main.eglur",
        false,
    );
    let spaced = resolve(
        [file(
            "main.eglur",
            "// retained only by the source input\n\npub fn value() -> i64 = 1\n",
        )],
        "main.eglur",
        false,
    );
    let main = module("main");

    assert_ne!(
        compact.analysis.source(&main).unwrap().fingerprint(),
        spaced.analysis.source(&main).unwrap().fingerprint()
    );
    assert_ne!(unit_key(&compact, "main"), unit_key(&spaced, "main"));
    assert_eq!(outputs(&compact, "main"), outputs(&spaced, "main"));
}

#[test]
fn declaration_allocation_order_does_not_enter_semantic_outputs() {
    let left_first = resolve(
        [file(
            "main.eglur",
            "pub fn left() -> i64 = 1\npub fn right() -> i64 = 2\n",
        )],
        "main.eglur",
        false,
    );
    let right_first = resolve(
        [file(
            "main.eglur",
            "pub fn right() -> i64 = 2\npub fn left() -> i64 = 1\n",
        )],
        "main.eglur",
        false,
    );

    assert_eq!(outputs(&left_first, "main"), outputs(&right_first, "main"));
    assert_ne!(
        unit_key(&left_first, "main"),
        unit_key(&right_first, "main")
    );
}

#[test]
fn private_body_changes_only_the_body_semantic_family() {
    let before = resolve(
        [file(
            "main.eglur",
            "pub fn value() -> i64 = helper()\nfn helper() -> i64 = 1\n",
        )],
        "main.eglur",
        false,
    );
    let after = resolve(
        [file(
            "main.eglur",
            "pub fn value() -> i64 = helper()\nfn helper() -> i64 = 2\n",
        )],
        "main.eglur",
        false,
    );
    let before = outputs(&before, "main");
    let after = outputs(&after, "main");

    assert_eq!(before.interface(), after.interface());
    assert_ne!(before.body(), after.body());
    assert_eq!(before.effect(), after.effect());
}

#[test]
fn written_function_body_form_reaches_only_the_body_fingerprint() {
    let block = resolve(
        [file("main.eglur", "pub fn value() -> i64 { yield 7 }\n")],
        "main.eglur",
        false,
    );
    let expression = resolve(
        [file("main.eglur", "pub fn value() -> i64 = { yield 7 }\n")],
        "main.eglur",
        false,
    );
    let with_trivia = resolve(
        [file(
            "main.eglur",
            "pub fn value() -> i64 =\n/* body */ { yield 7 }\n",
        )],
        "main.eglur",
        false,
    );
    let block = outputs(&block, "main");
    let expression = outputs(&expression, "main");

    assert_eq!(block.interface(), expression.interface());
    assert_eq!(block.effect(), expression.effect());
    assert_ne!(block.body(), expression.body());
    assert_eq!(expression, outputs(&with_trivia, "main"));
}

#[test]
fn private_struct_method_changes_do_not_escape_the_module_interface() {
    let before = resolve(
        [file(
            "main.eglur",
            concat!(
                "pub struct Counter {\n",
                "  fn hidden(self: &Self, value: i64) -> i64 = value\n",
                "}\n",
            ),
        )],
        "main.eglur",
        false,
    );
    let after = resolve(
        [file(
            "main.eglur",
            concat!(
                "pub struct Counter {\n",
                "  fn hidden(self: &Self, value: u64) -> u64 = value\n",
                "}\n",
            ),
        )],
        "main.eglur",
        false,
    );
    let before = outputs(&before, "main");
    let after = outputs(&after, "main");

    assert_eq!(before.interface(), after.interface());
    assert_ne!(before.body(), after.body());
    assert_eq!(before.effect(), after.effect());
}

#[test]
fn interface_changes_flow_through_reexports_only_to_dependent_units() {
    let sources = |result: &str| {
        [
            file("dep.eglur", &format!("pub fn value() -> {result} = 1\n")),
            file("bridge.eglur", "pub import dep::value\n"),
            file(
                "app.eglur",
                "import bridge::value\npub fn marker() -> Unit = ()\n",
            ),
            file("unrelated.eglur", "pub fn untouched() -> Unit = ()\n"),
        ]
    };
    let before = resolve(sources("i64"), "app.eglur", false);
    let after = resolve(sources("u64"), "app.eglur", false);

    assert_ne!(
        outputs(&before, "dep").interface(),
        outputs(&after, "dep").interface()
    );
    assert_ne!(
        outputs(&before, "bridge").interface(),
        outputs(&after, "bridge").interface()
    );
    assert_ne!(unit_key(&before, "app"), unit_key(&after, "app"));
    assert_eq!(
        unit_key(&before, "unrelated"),
        unit_key(&after, "unrelated")
    );
}

#[test]
fn reexported_type_fingerprint_carries_public_member_contracts() {
    let sources = |field_type: &str| {
        [
            file(
                "dep.eglur",
                &format!("pub struct Cell {{ pub field value: {field_type} }}\n"),
            ),
            file("bridge.eglur", "pub import dep::Cell\n"),
            file(
                "app.eglur",
                "import bridge::Cell\npub fn marker() -> Unit = ()\n",
            ),
        ]
    };
    let before = resolve(sources("i64"), "app.eglur", false);
    let after = resolve(sources("u64"), "app.eglur", false);

    assert_ne!(
        outputs(&before, "bridge").interface(),
        outputs(&after, "bridge").interface()
    );
    assert_ne!(unit_key(&before, "app"), unit_key(&after, "app"));
}

#[test]
fn effect_changes_flow_through_reexports_without_changing_interfaces() {
    let sources = |external: &str| {
        [
            file(
                "dep.eglur",
                &format!(
                    "import {external}\npub fn action!() -> Unit {{\n  {}(\"x\")\n  yield ()\n}}\n",
                    external.rsplit("::").next().unwrap()
                ),
            ),
            file("bridge.eglur", "pub import dep::action!\n"),
            file(
                "app.eglur",
                "import bridge::action!\npub fn marker() -> Unit = ()\n",
            ),
            file("unrelated.eglur", "pub fn untouched() -> Unit = ()\n"),
        ]
    };
    let before = resolve(sources("std::test::emit!"), "app.eglur", false);
    let after = resolve(sources("std::io::print!"), "app.eglur", false);

    assert_eq!(
        outputs(&before, "dep").interface(),
        outputs(&after, "dep").interface()
    );
    assert_ne!(
        outputs(&before, "dep").effect(),
        outputs(&after, "dep").effect()
    );
    assert_eq!(
        outputs(&before, "bridge").interface(),
        outputs(&after, "bridge").interface()
    );
    assert_ne!(
        outputs(&before, "bridge").effect(),
        outputs(&after, "bridge").effect()
    );
    assert_ne!(unit_key(&before, "app"), unit_key(&after, "app"));
    assert_eq!(
        unit_key(&before, "unrelated"),
        unit_key(&after, "unrelated")
    );
}

#[test]
fn import_cycle_has_one_nonrecursive_analysis_key() {
    let files = [
        file(
            "a.eglur",
            "import b::right\npub fn left() -> i64 = right()\n",
        ),
        file(
            "b.eglur",
            "import a::left\npub fn right() -> i64 = left()\n",
        ),
    ];
    let mut reversed = files.clone();
    reversed.reverse();
    let canonical = resolve(files, "a.eglur", false);
    let shuffled = resolve(reversed, "a.eglur", false);

    assert_eq!(canonical.analysis.units().len(), 1);
    assert_eq!(
        canonical.analysis.units()[0].members(),
        [module("a"), module("b")]
    );
    assert!(matches!(
        canonical.analysis.units()[0].dependencies(),
        [AnalysisDependencyFingerprint::GlobalConformance(_)]
    ));
    assert_eq!(canonical.analysis.units(), shuffled.analysis.units());
}

#[test]
fn public_impl_contract_changes_invalidate_unrelated_units() {
    let sources = |item: &str| {
        [
            file("traits.eglur", "pub trait Carries { type Item }\n"),
            file(
                "model.eglur",
                &format!(
                    "import traits::Carries\npub struct Holder {{}}\nimpl Carries for Holder {{ type Item = {item} }}\n"
                ),
            ),
            file(
                "app.eglur",
                "import model::Holder\npub fn marker() -> Unit = ()\n",
            ),
            file("unrelated.eglur", "pub fn untouched() -> Unit = ()\n"),
        ]
    };
    let before = resolve(sources("i64"), "app.eglur", false);
    let after = resolve(sources("u64"), "app.eglur", false);

    assert_global_conformance_dependency(&before);
    assert_global_conformance_dependency(&after);
    assert_ne!(
        before.analysis.conformance().fingerprint(),
        after.analysis.conformance().fingerprint()
    );
    assert_eq!(before.analysis.conformance().components().len(), 1);
    assert_eq!(after.analysis.conformance().components().len(), 1);
    assert_eq!(
        before.analysis.conformance().components()[0].module(),
        &module("model")
    );
    assert_eq!(
        before.analysis.conformance().components()[0].implementation(),
        after.analysis.conformance().components()[0].implementation()
    );
    assert_ne!(
        before.analysis.conformance().components()[0].fingerprint(),
        after.analysis.conformance().components()[0].fingerprint()
    );
    assert_ne!(
        unit_key(&before, "unrelated"),
        unit_key(&after, "unrelated")
    );
}

#[test]
fn private_alias_spelling_does_not_change_public_conformance_fingerprints() {
    let sources = |item: &str| {
        [
            file(
                "model.eglur",
                &format!(
                    "pub trait Carries {{ type Item }}\npub struct Holder {{}}\ntype Count = i64\nimpl Carries for Holder {{ type Item = {item} }}\n"
                ),
            ),
            file("unrelated.eglur", "pub fn untouched() -> Unit = ()\n"),
        ]
    };
    let canonical = resolve(sources("i64"), "model.eglur", false);
    let aliased = resolve(sources("Count"), "model.eglur", false);

    assert_eq!(
        canonical.analysis.conformance(),
        aliased.analysis.conformance()
    );
    assert_eq!(
        outputs(&canonical, "model").interface(),
        outputs(&aliased, "model").interface()
    );
    assert_eq!(
        unit_key(&canonical, "unrelated"),
        unit_key(&aliased, "unrelated")
    );
}

#[test]
fn unrelated_interface_changes_do_not_change_global_conformance() {
    let sources = |result: &str| {
        [
            file("api.eglur", &format!("pub fn value() -> {result} = 1\n")),
            file("unrelated.eglur", "pub fn untouched() -> Unit = ()\n"),
        ]
    };
    let before = resolve(sources("i64"), "api.eglur", false);
    let after = resolve(sources("u64"), "api.eglur", false);

    assert_eq!(before.analysis.conformance(), after.analysis.conformance());
    assert_eq!(
        unit_key(&before, "unrelated"),
        unit_key(&after, "unrelated")
    );
}

#[test]
fn impl_body_changes_do_not_change_conformance_contracts() {
    let sources = |value: &str| {
        [
            file(
                "model.eglur",
                &format!(
                    concat!(
                        "pub trait Readable {{ fn read(self: &Self) -> i64 }}\n",
                        "pub struct Count {{}}\n",
                        "impl Readable for Count {{ fn read(self: &Self) -> i64 = {} }}\n",
                    ),
                    value
                ),
            ),
            file("unrelated.eglur", "pub fn untouched() -> Unit = ()\n"),
        ]
    };
    let before = resolve(sources("1"), "model.eglur", false);
    let after = resolve(sources("2"), "model.eglur", false);

    assert_eq!(before.analysis.conformance(), after.analysis.conformance());
    assert_ne!(
        outputs(&before, "model").body(),
        outputs(&after, "model").body()
    );
    assert_eq!(
        unit_key(&before, "unrelated"),
        unit_key(&after, "unrelated")
    );
}

#[test]
fn private_conformance_does_not_enter_the_global_environment() {
    let sources = |item: &str| {
        [
            file(
                "private.eglur",
                &format!(
                    "trait Carries {{ type Item }}\nstruct Holder {{}}\nimpl Carries for Holder {{ type Item = {item} }}\n"
                ),
            ),
            file("unrelated.eglur", "pub fn untouched() -> Unit = ()\n"),
        ]
    };
    let before = resolve(sources("i64"), "private.eglur", false);
    let after = resolve(sources("u64"), "private.eglur", false);

    assert!(before.analysis.conformance().components().is_empty());
    assert_eq!(before.analysis.conformance(), after.analysis.conformance());
    assert_eq!(
        unit_key(&before, "unrelated"),
        unit_key(&after, "unrelated")
    );
}

#[test]
fn default_method_override_changes_the_conformance_route() {
    let before = resolve(
        [file(
            "model.eglur",
            concat!(
                "pub trait Readable { fn read(self: &Self) -> i64 = 7 }\n",
                "pub struct Count {}\n",
                "impl Readable for Count {}\n",
            ),
        )],
        "model.eglur",
        false,
    );
    let after = resolve(
        [file(
            "model.eglur",
            concat!(
                "pub trait Readable { fn read(self: &Self) -> i64 = 7 }\n",
                "pub struct Count {}\n",
                "impl Readable for Count { fn read(self: &Self) -> i64 = 8 }\n",
            ),
        )],
        "model.eglur",
        false,
    );

    assert_eq!(before.analysis.conformance().components().len(), 1);
    assert_eq!(
        before.analysis.conformance().components()[0].implementation(),
        after.analysis.conformance().components()[0].implementation()
    );
    assert_ne!(before.analysis.conformance(), after.analysis.conformance());
}

#[test]
fn conformance_fingerprints_ignore_source_insertion_order() {
    let files = [
        file(
            "a.eglur",
            "pub trait Left {}\npub struct Alpha {}\nimpl Left for Alpha {}\n",
        ),
        file(
            "z.eglur",
            "pub trait Right {}\npub struct Omega {}\nimpl Right for Omega {}\n",
        ),
    ];
    let mut reversed = files.clone();
    reversed.reverse();

    let canonical = resolve(files, "a.eglur", false);
    let shuffled = resolve(reversed, "a.eglur", false);

    assert_eq!(
        canonical.analysis.conformance(),
        shuffled.analysis.conformance()
    );
}

#[test]
fn generic_ordinal_fingerprints_preserve_owner_contexts_under_reordering() {
    fn source(reversed: bool) -> String {
        let mut methods = [
            "pub fn first[U](self: &Self, value: U) -> U = value\n",
            "pub fn second[V](self: &Self, value: V) -> V = value\n",
        ];
        if reversed {
            methods.reverse();
        }
        let mut declarations = vec![
            "pub trait Mark {}\n".to_owned(),
            "pub struct Host {}\n".to_owned(),
            "impl Mark for Host {}\n".to_owned(),
            "pub enum Either[A, B] { Left(A)\n Right(B) }\n".to_owned(),
            "pub type Pair[A, B] = (A, B)\n".to_owned(),
            "pub trait Echo[A] { fn echo[B: Mark](self: &Self, value: B) -> B }\n".to_owned(),
            format!("pub struct Wrap[X, Y] {{\n{}}}\n", methods.concat()),
            "impl[X, Y] Echo[X] for Wrap[X, Y] {\n\
                fn echo[C: Mark](self: &Self, value: C) -> C = value\n}\n"
                .to_owned(),
            "pub fn identity[D](value: D) -> D = value\n".to_owned(),
            "pub fn use(value: Wrap[Bool, Str], item: Host) -> Host = value.echo(identity(item))\n"
                .to_owned(),
        ];
        if reversed {
            declarations.reverse();
        }
        declarations.concat()
    }

    let canonical = resolve([file("main.eglur", &source(false))], "main.eglur", false);
    let reordered = resolve([file("main.eglur", &source(true))], "main.eglur", false);

    assert_eq!(canonical.semantics, reordered.semantics);
}
