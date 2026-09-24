use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use eglur_diagnostics::DiagnosticSnapshot;
use eglur_driver::{
    CompileAttempt, CompileDisposition, CompilerSession, FileInput, FileSetRequest, SessionOptions,
    SupportProfile, TrustedCatalog,
};
use eglur_sema::{CanonicalTarget, DeclarationKind, FactState};
use serde::Deserialize;

#[derive(Deserialize)]
struct ModuleFixture {
    id: String,
    file_set_root: String,
    files: BTreeMap<String, String>,
    entry: String,
    expect: FixtureExpectation,
}

#[derive(Deserialize)]
struct FixtureExpectation {
    compile_error: FixtureCompileError,
}

#[derive(Deserialize)]
struct FixtureCompileError {
    code: String,
    message_contains: Vec<String>,
}

fn compile(source: &str) -> (Vec<String>, CompileDisposition) {
    let request = FileSetRequest::new(
        [FileInput::new("main.eglur", source.as_bytes().to_vec())],
        "main.eglur",
        None,
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::new(false)).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let sealed = session.seal(None).unwrap();
    let codes = match sealed.diagnostics() {
        DiagnosticSnapshot::Accepted(_) => Vec::new(),
        DiagnosticSnapshot::Rejected(diagnostics) => diagnostics
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code().as_str().to_owned())
            .collect(),
    };
    let disposition = CompileAttempt::finalize(sealed, None)
        .unwrap()
        .disposition();
    (codes, disposition)
}

#[test]
fn unused_public_declarations_reject_private_type_contracts() {
    for declaration in [
        "pub fn expose() -> Hidden = Hidden{}",
        "pub fn expose[T: Internal](value: T) -> T = value",
        "pub struct Expose[T: Internal] {}",
        "pub enum Expose[T: Internal] { Item(T) }",
        "pub trait Expose[T: Internal] {}",
        "pub type Expose[T: Internal] = T",
        "pub type Expose = Hidden",
        "pub const EXPOSE: Hidden = Hidden{}",
        "pub struct Expose { pub field value: Opt[Hidden] }",
        "pub struct Expose { pub prop hidden: Hidden { fn get_hidden(self: &Self) -> Hidden = Hidden{} } }",
        "pub struct Expose { pub fn value(self: &Self) -> Hidden = Hidden{} }",
        "pub trait Expose { fn value(self: &Self) -> Hidden }",
        "pub enum Expose { Item(Hidden) }",
        "pub enum Expose { Item { value: Hidden } }",
        "pub fn expose(value: Fn(&Hidden) -> Unit) -> Unit = ()",
        "pub fn expose(value: (i64, Opt[Hidden])) -> Unit = ()",
        "pub trait Public[T] {}\npub fn expose[T: Public[Hidden]](value: T) -> T = value",
    ] {
        let source = format!("struct Hidden {{}}\ntrait Internal {{}}\n{declaration}\n");
        let (codes, disposition) = compile(&source);
        assert_eq!(
            codes.first().map(String::as_str),
            Some("VISIBILITY_VIOLATION"),
            "{source}: {codes:?}"
        );
        assert_eq!(disposition, CompileDisposition::Rejected, "{source}");
    }
}

#[test]
fn private_representation_and_alias_spellings_do_not_make_public_contracts_private() {
    for source in [
        "struct Hidden {}\npub struct Expose { field value: Hidden }\n",
        "struct Hidden {}\nstruct Private { pub field value: Hidden }\n",
        "struct Hidden {}\npub struct Expose { fn hidden(self: &Self) -> Hidden = Hidden{} }\n",
        "type Internal = i64\npub fn expose() -> Internal = 1\n",
        "pub fn expose[T](value: T) -> T = value\n",
        "trait Internal {}\nimpl Internal for i64 {}\ntype Checked[T: Internal] = T\npub fn expose() -> Checked[i64] = 1\n",
        "pub trait Carries { type Item }\nstruct Private {}\nimpl Carries for Private { type Item = i64 }\ntype Item[T: Carries] = T::Item\npub fn expose() -> Item[Private] = 1\n",
        concat!(
            "type InternalCount = i64\n",
            "pub type Count = InternalCount\n",
            "pub struct Counter {\n",
            "  pub field value: InternalCount\n",
            "  pub prop doubled: InternalCount {\n",
            "    fn get_doubled(self: &Self) -> InternalCount = self.value\n",
            "  }\n",
            "  pub fn add(self: &Self, delta: InternalCount) -> InternalCount = delta\n",
            "}\n",
            "pub enum Event { Counted(InternalCount) }\n",
            "pub trait Scale { fn scale(self: &Self, factor: InternalCount) -> InternalCount }\n",
            "pub const ONE: InternalCount = 1\n",
        ),
    ] {
        let (codes, disposition) = compile(source);
        assert!(codes.is_empty(), "{source}: {codes:?}");
        assert_eq!(disposition, CompileDisposition::Succeeded, "{source}");
    }
}

#[test]
fn visibility_is_blamed_on_the_unused_public_contract() {
    for (declaration, expected_span) in [
        (
            "pub fn expose(first: Opt[Hidden], second: Hidden) -> Hidden = Hidden{}",
            "Opt[Hidden]",
        ),
        (
            "pub fn expose[T: Internal](value: T) -> T = value",
            "Internal",
        ),
        ("pub type Expose = Opt[Hidden]", "Opt[Hidden]"),
        ("pub enum Expose { Item { value: Hidden } }", "Hidden"),
    ] {
        let source = format!("struct Hidden {{}}\ntrait Internal {{}}\n{declaration}\n");
        let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
        let mut session = CompilerSession::build(request, SessionOptions::new(false)).unwrap();
        let catalog = TrustedCatalog::first_slice().unwrap();
        session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        let DiagnosticSnapshot::Rejected(snapshot) = session.into_diagnostics(None) else {
            panic!("missing public-contract visibility diagnostic: {source}")
        };
        assert_eq!(snapshot.diagnostics().len(), 1, "{source}");
        let diagnostic = &snapshot.diagnostics()[0];
        assert_eq!(diagnostic.code().as_str(), "VISIBILITY_VIOLATION");
        let span = diagnostic
            .primary_span()
            .as_source()
            .unwrap()
            .file_span()
            .range();
        assert_eq!(
            source[span.start().as_usize()..span.end().as_usize()].trim(),
            expected_span,
            "{source}"
        );
        assert!(diagnostic.message().contains("private"), "{source}");
    }
}

#[test]
fn structural_and_bound_failures_precede_public_visibility() {
    for (declaration, expected) in [
        (
            "pub type Expose = Opt[Hidden, Hidden]",
            "TYPE_ARG_ARITY_MISMATCH",
        ),
        (
            "type Checked[T: Internal] = Hidden\npub fn expose(value: Checked[i64]) -> Unit = ()",
            "TRAIT_BOUND_NOT_SATISFIED",
        ),
    ] {
        let source = format!("struct Hidden {{}}\ntrait Internal {{}}\n{declaration}\n");
        let (codes, disposition) = compile(&source);
        assert_eq!(codes, [expected], "{source}");
        assert_ne!(disposition, CompileDisposition::Succeeded, "{source}");
    }
}

#[test]
fn public_conformance_headers_are_checked_before_missing_bindings() {
    for header in [
        "impl Marker[i64] for Carrier[Hidden] {}",
        "impl Marker[Hidden] for Carrier[i64] {}",
        "impl[T: Internal] Marker[i64] for GenericCarrier[T] {}",
    ] {
        let source = format!(
            "trait Internal {{}}\nstruct Hidden {{}}\npub trait Marker[T] {{ type Item }}\npub struct Carrier[T] {{}}\npub struct GenericCarrier[T] {{}}\n{header}\n",
        );
        let (codes, disposition) = compile(&source);
        assert_eq!(codes, ["VISIBILITY_VIOLATION"], "{source}");
        assert_eq!(disposition, CompileDisposition::Rejected, "{source}");
    }
}

#[test]
fn public_conformance_bindings_use_alias_expansion_and_nested_closure() {
    for item in ["Hidden", "Opt[Hidden]", "InternalHidden"] {
        let source = format!(
            "pub trait Expose {{ type Item }}\nstruct Hidden {{}}\ntype InternalHidden = Hidden\npub struct Carrier {{}}\nimpl Expose for Carrier {{ type Item = {item} }}\n",
        );
        let (codes, disposition) = compile(&source);
        assert_eq!(codes, ["VISIBILITY_VIOLATION"], "{source}");
        assert_eq!(disposition, CompileDisposition::Rejected, "{source}");
    }
    let normalized = concat!(
        "pub trait Source { type Item }\n",
        "pub trait Expose { type Item }\n",
        "struct Hidden {}\n",
        "struct PrivateCarrier {}\n",
        "impl Source for PrivateCarrier { type Item = Hidden }\n",
        "type Pick[T: Source] = T::Item\n",
        "pub struct PublicCarrier {}\n",
        "impl Expose for PublicCarrier { type Item = Pick[PrivateCarrier] }\n",
    );
    let (codes, disposition) = compile(normalized);
    assert_eq!(codes, ["VISIBILITY_VIOLATION"], "{normalized}");
    assert_eq!(disposition, CompileDisposition::Rejected);
    for source in [
        "pub trait Expose { type Item }\nstruct Hidden {}\nimpl Expose for i64 { type Item = Hidden }\n",
        "pub trait Expose { type Item }\nstruct Hidden {}\nimpl Expose for &Hidden { type Item = i64 }\n",
    ] {
        let (codes, disposition) = compile(source);
        assert_eq!(codes, ["VISIBILITY_VIOLATION"], "{source}");
        assert_eq!(disposition, CompileDisposition::Rejected, "{source}");
    }
    for source in [
        "pub trait Expose { type Item }\npub struct Carrier {}\ntype Count = i64\nimpl Expose for Carrier { type Item = Count }\n",
        "pub trait Expose { type Item }\nstruct Hidden {}\nstruct Carrier {}\nimpl Expose for Carrier { type Item = Hidden }\n",
        "trait Expose { type Item }\nstruct Hidden {}\npub struct Carrier {}\nimpl Expose for Carrier { type Item = Hidden }\n",
        "trait Expose[T] { type Item }\nstruct Hidden {}\npub struct Carrier {}\nimpl Expose[Hidden] for Carrier { type Item = Hidden }\n",
        "pub trait Source { type Item }\npub trait Expose { type Item }\npub struct Carrier[T: Source] {}\nimpl[T: Source] Expose for Carrier[T] { type Item = T::Item }\n",
    ] {
        let (codes, disposition) = compile(source);
        assert!(codes.is_empty(), "{source}: {codes:?}");
        assert_eq!(disposition, CompileDisposition::Succeeded, "{source}");
    }
}

#[test]
fn associated_binding_validation_precedes_visibility() {
    let source = concat!(
        "pub trait Expose { type Item }\n",
        "struct Hidden {}\n",
        "pub struct Carrier {}\n",
        "impl Expose for Carrier { type Item = &Hidden }\n",
    );
    let (codes, disposition) = compile(source);
    assert_eq!(codes, ["REFERENCE_TYPE_POSITION_FORBIDDEN"]);
    assert_eq!(disposition, CompileDisposition::Rejected);
}

#[test]
fn rejected_public_conformance_cannot_supply_checked_evidence() {
    for source in [
        "pub trait Expose { type Item }\nstruct Hidden {}\npub struct Carrier {}\nimpl Expose for Carrier { type Item = Hidden }\n",
        "pub trait Expose { type Item }\nstruct Hidden {}\npub struct Carrier[T] {}\nimpl Expose for Carrier[Hidden] { type Item = i64 }\n",
    ] {
        let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
        let mut session = CompilerSession::build(request, SessionOptions::new(false)).unwrap();
        let catalog = TrustedCatalog::first_slice().unwrap();
        let analysis = session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        let resolution = analysis.resolution().unwrap();
        let implementation = resolution
            .declarations()
            .modules()
            .flat_map(|module| module.declarations())
            .find(|declaration| declaration.kind() == DeclarationKind::Impl)
            .unwrap();
        let target = CanonicalTarget::Source(implementation.id());
        assert!(matches!(
            resolution.signatures().impl_header(&target),
            FactState::Poisoned(_)
        ));
        assert!(
            resolution
                .trait_environment()
                .checked_impl_target(&target)
                .is_none()
        );
        assert!(resolution.fingerprints().is_none());
    }
}

#[test]
fn a_private_alias_user_is_poisoned_by_an_invalid_public_contract() {
    let source = concat!(
        "struct Hidden {}\n",
        "pub type Exposed = Hidden\n",
        "type Follow = Exposed\n",
        "pub trait Marker {}\n",
        "impl Marker for Exposed {}\n",
        "fn consume(value: Follow) -> Unit = ()\n",
    );
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::new(false)).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let resolution = analysis.resolution().unwrap();
    for name in ["Exposed", "Follow", "consume"] {
        let declaration = resolution
            .declarations()
            .modules()
            .flat_map(|module| module.declarations())
            .find(|declaration| declaration.name().is_some_and(|found| found.text() == name))
            .unwrap();
        assert!(
            matches!(
                resolution.signatures().source_declaration(declaration.id()),
                FactState::Poisoned(_)
            ),
            "{name} kept a checked signature"
        );
    }
    let implementation = resolution
        .declarations()
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| declaration.kind() == DeclarationKind::Impl)
        .unwrap();
    let target = CanonicalTarget::Source(implementation.id());
    assert!(matches!(
        resolution.signatures().impl_header(&target),
        FactState::Poisoned(_)
    ));
    assert!(
        resolution
            .trait_environment()
            .checked_impl_target(&target)
            .is_none()
    );
    assert!(resolution.fingerprints().is_none());
    let DiagnosticSnapshot::Rejected(snapshot) = session.into_diagnostics(None) else {
        panic!("missing public-contract visibility diagnostic")
    };
    assert_eq!(snapshot.diagnostics().len(), 1);
    assert_eq!(
        snapshot.diagnostics()[0].code().as_str(),
        "VISIBILITY_VIOLATION"
    );
}

#[test]
fn an_unused_public_contract_in_another_file_is_still_rejected() {
    let request = FileSetRequest::new(
        [
            FileInput::new(
                "api.eglur",
                b"struct Hidden {}\npub fn expose() -> Hidden = Hidden{}\n".to_vec(),
            ),
            FileInput::new("main.eglur", b"pub fn marker() -> Unit = ()\n".to_vec()),
        ],
        "main.eglur",
        Some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::new(false)).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let sealed = session.seal(None).unwrap();
    let DiagnosticSnapshot::Rejected(snapshot) = sealed.diagnostics() else {
        panic!("unused public declaration in another file was accepted")
    };
    assert_eq!(snapshot.diagnostics().len(), 1);
    assert_eq!(
        snapshot.diagnostics()[0].code().as_str(),
        "VISIBILITY_VIOLATION"
    );
    assert_eq!(
        CompileAttempt::finalize(sealed, None)
            .unwrap()
            .disposition(),
        CompileDisposition::Rejected
    );
}

#[test]
fn existing_module_fixtures_reject_public_type_leaks() {
    let expected = [
        "MOD-public-function-cannot-return-private-nominal",
        "MOD-public-generic-bound-cannot-name-private-trait",
        "MOD-public-struct-bound-cannot-name-private-trait",
        "MOD-public-enum-bound-cannot-name-private-trait",
        "MOD-public-trait-bound-cannot-name-private-trait",
        "MOD-public-type-alias-bound-cannot-name-private-trait",
        "MOD-public-property-cannot-expose-private-nominal",
        "MOD-public-inherent-method-cannot-expose-private-nominal",
        "MOD-public-trait-method-cannot-expose-private-nominal",
        "MOD-public-enum-payload-cannot-expose-private-nominal",
        "MOD-public-const-cannot-expose-private-nominal",
        "MOD-public-type-alias-cannot-bridge-private-nominal",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<BTreeSet<_>>();
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/conformance/core/modules.yaml");
    let suite: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    let cases = suite
        .get("tests")
        .and_then(serde_yaml::Value::as_sequence)
        .unwrap();
    let mut covered = BTreeSet::new();

    for case in cases {
        let Some(id) = case.get("id").and_then(serde_yaml::Value::as_str) else {
            continue;
        };
        if !expected.contains(id) {
            continue;
        }
        let case: ModuleFixture = serde_yaml::from_value(case.clone()).unwrap();
        assert!(
            covered.insert(case.id.clone()),
            "duplicate fixture {}",
            case.id
        );
        let request = FileSetRequest::new(
            case.files
                .into_iter()
                .map(|(path, source)| FileInput::new(path, source.into_bytes())),
            case.entry,
            Some(case.file_set_root.as_str()),
        )
        .unwrap();
        let mut session = CompilerSession::build(request, SessionOptions::new(false)).unwrap();
        let catalog = TrustedCatalog::first_slice().unwrap();
        session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        let sealed = session.seal(None).unwrap();
        let DiagnosticSnapshot::Rejected(snapshot) = sealed.diagnostics() else {
            panic!("{} was not rejected", case.id)
        };
        let diagnostic = &snapshot.diagnostics()[0];
        assert_eq!(
            diagnostic.code().as_str(),
            case.expect.compile_error.code,
            "{}",
            case.id
        );
        for phrase in case.expect.compile_error.message_contains {
            assert!(
                diagnostic.message().contains(&phrase),
                "{}: {phrase}",
                case.id
            );
        }
        assert_eq!(
            CompileAttempt::finalize(sealed, None)
                .unwrap()
                .disposition(),
            CompileDisposition::Rejected,
            "{}",
            case.id
        );
    }
    assert_eq!(covered, expected);
}

#[test]
fn existing_trait_fixtures_reject_public_conformance_leaks() {
    let expected = [
        "TRT-public-conformance-cannot-expose-private-associated-binding",
        "TRT-public-conformance-on-intrinsic-target-checks-associated-visibility",
        "TRT-associated-reference-error-precedes-public-visibility-error",
        "TRT-public-conformance-header-cannot-contain-private-type-argument",
        "TRT-public-conformance-trait-reference-cannot-contain-private-type-argument",
        "TRT-public-conformance-bound-cannot-name-private-trait",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/conformance/core/traits.yaml");
    let suite: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    let cases = suite
        .get("tests")
        .and_then(serde_yaml::Value::as_sequence)
        .unwrap();
    let mut covered = BTreeSet::new();

    for case in cases {
        let Some(id) = case.get("id").and_then(serde_yaml::Value::as_str) else {
            continue;
        };
        if !expected.contains(id) {
            continue;
        }
        covered.insert(id);
        let request = if let Some(code) = case.get("code").and_then(serde_yaml::Value::as_str) {
            FileSetRequest::single("main.eglur", code.as_bytes().to_vec()).unwrap()
        } else {
            let fixture: ModuleFixture = serde_yaml::from_value(case.clone()).unwrap();
            FileSetRequest::new(
                fixture
                    .files
                    .into_iter()
                    .map(|(path, source)| FileInput::new(path, source.into_bytes())),
                fixture.entry,
                Some(fixture.file_set_root.as_str()),
            )
            .unwrap()
        };
        let mut session = CompilerSession::build(request, SessionOptions::new(false)).unwrap();
        let catalog = TrustedCatalog::first_slice().unwrap();
        session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        let sealed = session.seal(None).unwrap();
        let DiagnosticSnapshot::Rejected(snapshot) = sealed.diagnostics() else {
            panic!("{id} was not rejected")
        };
        let expected_code = case["expect"]["compile_error"]["code"].as_str().unwrap();
        assert_eq!(
            snapshot.diagnostics()[0].code().as_str(),
            expected_code,
            "{id}"
        );
        assert_eq!(
            CompileAttempt::finalize(sealed, None)
                .unwrap()
                .disposition(),
            CompileDisposition::Rejected,
            "{id}"
        );
    }
    assert_eq!(covered, expected);
}
