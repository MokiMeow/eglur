use std::collections::BTreeMap;

use eglur_driver::{
    CompileAttempt, CompilerSession, CountUnit, EvaluationLimits, ExecutionAttempt,
    ExecutionOutcome, FileInput, FileSetRequest, ProgramMode, RuntimeDiagnostic, RuntimeEvent,
    SessionOptions, SupportManifest, SupportProfile, TrustedCatalog, UnsupportedInventory, Value,
    execute,
};
use eglur_sema::LanguageItem;
use serde::Serialize;
use serde_json::{Value as JsonValue, json};

use super::model::{
    DiagnosticExpectation, ExpectedScalar, FixtureIdentity, FixtureSource, OutcomeExpectation,
    PrimarySpanExpectation, SemanticFixture, SemanticFixtureCorpus, ValueExpectation,
};

#[derive(Clone, Debug, Serialize)]
pub struct ManifestRecord {
    pub schema: String,
    pub profile: String,
    pub compile: Vec<String>,
    pub execute: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SupportRecord {
    pub manifest: ManifestRecord,
    pub unsupported_inventory: JsonValue,
}

#[derive(Clone, Debug, Serialize)]
pub struct PhaseRecord {
    pub phase: String,
    pub input_count: usize,
    pub input_unit: String,
    pub output_count: usize,
    pub output_unit: String,
    pub elapsed_seconds: u64,
    pub elapsed_nanoseconds: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeDiagnosticRecord {
    pub code: String,
    pub message: String,
    pub data: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExecutionRecord {
    pub kind: String,
    pub process_status: u8,
    pub result_variant: String,
    pub representation: Option<String>,
    pub error_variant: Option<String>,
    pub value_diagnostics: Vec<RuntimeDiagnosticRecord>,
    pub side_effects: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FixtureRecord {
    pub identity: FixtureIdentity,
    pub disposition: String,
    pub compiler_result: JsonValue,
    pub human_output: String,
    pub phases: Vec<PhaseRecord>,
    pub execution: Option<ExecutionRecord>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SliceRun {
    pub schema_version: String,
    pub corpus_input_digest: String,
    pub support: SupportRecord,
    pub fixtures: Vec<FixtureRecord>,
}

impl SliceRun {
    pub fn snapshot_json(&self) -> String {
        let fixtures = self
            .fixtures
            .iter()
            .map(|fixture| {
                json!({
                    "identity": fixture.identity,
                    "disposition": fixture.disposition,
                    "compiler_result": fixture.compiler_result,
                    "human_output": fixture.human_output,
                    "phase_counts": fixture.phases.iter().map(|phase| json!({
                        "phase": phase.phase,
                        "input": {"count": phase.input_count, "unit": phase.input_unit},
                        "output": {"count": phase.output_count, "unit": phase.output_unit},
                    })).collect::<Vec<_>>(),
                    "execution": fixture.execution,
                })
            })
            .collect::<Vec<_>>();
        let value = json!({
            "kind": "eglur_semantic_slice_snapshot",
            "schema_version": self.schema_version,
            "corpus_input_digest": self.corpus_input_digest,
            "support": self.support,
            "fixtures": fixtures,
        });
        format!(
            "{}\n",
            serde_json::to_string(&value).expect("semantic-slice snapshots are JSON serializable")
        )
    }
}

pub fn run_corpus(corpus: &SemanticFixtureCorpus) -> SliceRun {
    let profile = SupportProfile::semantic_core_v1();
    let catalog = TrustedCatalog::first_slice().expect("the first-slice catalog is valid");
    let manifest = SupportManifest::from_profile(&profile);
    let inventory = UnsupportedInventory::from_profile(&profile);
    let support = SupportRecord {
        manifest: ManifestRecord {
            schema: manifest.schema().into(),
            profile: manifest.profile().into(),
            compile: manifest.compile().iter().map(ToString::to_string).collect(),
            execute: manifest.execute().iter().map(ToString::to_string).collect(),
        },
        unsupported_inventory: serde_json::from_str(&inventory.to_json())
            .expect("the unsupported inventory is valid JSON"),
    };
    let fixtures = corpus
        .fixtures
        .iter()
        .map(|fixture| run_fixture(fixture, &profile, &catalog, &manifest))
        .collect();

    SliceRun {
        schema_version: corpus.schema_version.clone(),
        corpus_input_digest: corpus.input_digest.clone(),
        support,
        fixtures,
    }
}

fn run_fixture(
    fixture: &SemanticFixture,
    profile: &SupportProfile,
    catalog: &TrustedCatalog,
    expected_manifest: &SupportManifest,
) -> FixtureRecord {
    let label = fixture.identity.label();
    let mut session = CompilerSession::build(
        request(&fixture.invocation.source),
        SessionOptions::new(true),
    )
    .unwrap_or_else(|error| panic!("{label}: source build failed: {error}"));
    session
        .resolve_for(ProgramMode::Executable, profile, Some(catalog))
        .unwrap_or_else(|error| panic!("{label}: semantic analysis failed internally: {error}"));
    let sealed = session.seal(None).unwrap_or_else(|error| {
        panic!("{label}: checked-program sealing failed internally: {error}")
    });
    let attempt = CompileAttempt::finalize(sealed, None)
        .unwrap_or_else(|error| panic!("{label}: compile-attempt finalization failed: {error}"));
    assert_eq!(
        attempt.support_manifest(),
        expected_manifest,
        "{label}: attempt support manifest drifted"
    );

    let projection = attempt.compiler_result().unwrap_or_else(|error| {
        panic!("{label}: selected corpus produced an inconclusive attempt: {error}")
    });
    let compiler_result = serde_json::from_str::<JsonValue>(
        &projection
            .to_json()
            .unwrap_or_else(|error| panic!("{label}: compiler-result transport failed: {error}")),
    )
    .expect("compiler-result projections are valid JSON");
    let human_output = attempt.render_human();
    let phases = phase_records(attempt.observations(), &label);

    let (disposition, execution) = match (&fixture.expectation.outcome, &attempt) {
        (OutcomeExpectation::CompileError { diagnostic }, CompileAttempt::Rejected(_)) => {
            assert_compile_error(
                fixture,
                diagnostic,
                &projection,
                &compiler_result,
                &human_output,
            );
            assert!(
                fixture.expectation.value_diagnostics.is_empty()
                    && fixture.expectation.side_effects.is_empty(),
                "{label}: compile-error fixtures cannot expect runtime observations"
            );
            ("rejected".into(), None)
        }
        (OutcomeExpectation::Value { value }, CompileAttempt::Succeeded(success)) => {
            assert!(
                projection.succeeded(),
                "{label}: successful attempt projection rejected"
            );
            assert!(
                human_output.is_empty(),
                "{label}: successful compilation rendered diagnostics"
            );
            let execution = match execute(
                success.checked_program(),
                profile,
                catalog,
                EvaluationLimits::default(),
            ) {
                ExecutionAttempt::Executed(ExecutionOutcome::Completed(completed)) => {
                    execution_record(
                        fixture,
                        value,
                        success.checked_program(),
                        completed.value(),
                        completed.process_status(),
                        completed.value_diagnostics(),
                        completed.events(),
                    )
                }
                other => panic!("{label}: expected completed execution, found {other:?}"),
            };
            ("succeeded".into(), Some(execution))
        }
        (expected, actual) => panic!(
            "{label}: expectation {expected:?} produced compile disposition {:?}",
            actual.disposition()
        ),
    };

    if let Some(tokens) = &fixture.expectation.diagnostic_contains {
        for token in tokens {
            assert!(
                human_output.contains(token),
                "{label}: human output is missing diagnostic fragment {token:?}"
            );
        }
    }

    FixtureRecord {
        identity: fixture.identity.clone(),
        disposition,
        compiler_result,
        human_output,
        phases,
        execution,
    }
}

fn request(source: &FixtureSource) -> FileSetRequest {
    match source {
        FixtureSource::Utf8 { text } => FileSetRequest::new(
            [FileInput::with_source_label(
                "code.eglur",
                "code",
                text.as_bytes().to_vec(),
            )],
            "code.eglur",
            None,
        )
        .expect("inline fixture is valid"),
        FixtureSource::Files {
            entry,
            file_set_root,
            files,
            symlinks,
        } => {
            assert!(
                symlinks.is_empty(),
                "semantic slice does not admit symlink fixtures"
            );
            FileSetRequest::new(
                files
                    .iter()
                    .map(|(path, text)| FileInput::new(path.as_str(), text.as_bytes().to_vec())),
                entry.as_str(),
                Some(file_set_root.as_str()),
            )
            .expect("projected file-set fixture is valid")
        }
    }
}

fn assert_compile_error(
    fixture: &SemanticFixture,
    expected: &DiagnosticExpectation,
    projection: &eglur_driver::CompilerResultProjection<'_>,
    machine: &JsonValue,
    human: &str,
) {
    let label = fixture.identity.label();
    let diagnostic = projection
        .primary_diagnostic()
        .expect("rejected compiler result has one primary diagnostic");
    assert_eq!(
        diagnostic.code().as_str(),
        expected.code,
        "{label}: diagnostic code"
    );
    assert_eq!(
        machine["outcome"]["kind"], "rejected",
        "{label}: machine outcome"
    );
    let machine_diagnostic = &machine["outcome"]["primary_diagnostic"];
    assert_eq!(
        machine_diagnostic["code"], expected.code,
        "{label}: machine code"
    );
    assert_eq!(
        machine_diagnostic["message"],
        diagnostic.message(),
        "{label}: message parity"
    );
    assert!(
        human.contains(&format!("[{}] {}", expected.code, diagnostic.message())),
        "{label}: human rendering does not contain the selected primary diagnostic"
    );
    for token in &expected.message_all_of {
        assert!(
            diagnostic.message().contains(token),
            "{label}: message is missing {token:?}"
        );
    }
    for group in &expected.message_any_of {
        assert!(
            group
                .iter()
                .any(|token| diagnostic.message().contains(token)),
            "{label}: message matches none of {group:?}"
        );
    }
    for key in &expected.data_keys {
        assert!(
            machine_diagnostic["data"].get(key).is_some(),
            "{label}: diagnostic data is missing {key:?}"
        );
    }
    for (key, scalar) in &expected.data_equals {
        assert_expected_scalar(
            scalar,
            &machine_diagnostic["data"][key],
            &format!("{label}: diagnostic data.{key}"),
        );
    }
    if let Some(PrimarySpanExpectation::Marker {
        source,
        marker,
        line,
    }) = &expected.primary_span
    {
        let source_text = fixture
            .invocation
            .source
            .source_text(source)
            .unwrap_or_else(|| panic!("{label}: marker source {source:?} is missing"));
        let marker_line = source_text
            .lines()
            .nth((*line as usize).saturating_sub(1))
            .unwrap_or_else(|| panic!("{label}: marker line {line} is missing"));
        assert!(
            marker_line.contains(&format!("@primary {marker}")),
            "{label}: marker metadata does not match source"
        );
        assert_eq!(machine_diagnostic["primary_span"]["source"], *source);
        assert_eq!(
            machine_diagnostic["primary_span"]["text"]["start"]["line"],
            *line
        );
    }
    if let Some(tokens) = &expected.help_contains {
        let help = diagnostic.help().unwrap_or_default();
        for token in tokens {
            assert!(help.contains(token), "{label}: help is missing {token:?}");
        }
    }
    if let Some(tokens) = &expected.related_contains {
        let related = diagnostic
            .related()
            .iter()
            .map(|span| span.label())
            .collect::<Vec<_>>()
            .join("\n");
        for token in tokens {
            assert!(
                related.contains(token),
                "{label}: related spans are missing {token:?}"
            );
        }
    }
    if let Some(help) = diagnostic.help() {
        assert!(
            human.contains(help),
            "{label}: human output dropped diagnostic help"
        );
    }
    for related in diagnostic.related() {
        assert!(
            human.contains(related.label()),
            "{label}: human output dropped related span label {:?}",
            related.label()
        );
    }
    if let Some(exit) = fixture.expectation.process_exit {
        assert_eq!(
            projection.exit_status(),
            exit,
            "{label}: compiler process exit"
        );
    }
}

fn execution_record(
    fixture: &SemanticFixture,
    expected: &ValueExpectation,
    program: &eglur_driver::CheckedProgram,
    value: &Value,
    process_status: u8,
    diagnostics: &[RuntimeDiagnostic],
    events: &[RuntimeEvent],
) -> ExecutionRecord {
    let label = fixture.identity.label();
    let Value::Variant {
        declaration,
        payload,
    } = value
    else {
        panic!("{label}: executable result entry did not return Res")
    };
    let items = program.resolution().signatures().language_items();
    let result_variant = if Some(declaration) == items.declaration(LanguageItem::ResOk) {
        "Ok"
    } else if Some(declaration) == items.declaration(LanguageItem::ResErr) {
        "Err"
    } else {
        panic!("{label}: entry returned an unknown result variant")
    };
    let [payload] = payload.as_ref() else {
        panic!("{label}: result variant must carry one value")
    };
    let (representation, error_variant) = value_projection(payload, &label);
    assert_eq!(result_variant, expected.kind, "{label}: result variant");
    assert_eq!(
        representation, expected.representation,
        "{label}: value representation"
    );
    assert_eq!(
        error_variant, expected.error_variant,
        "{label}: error variant"
    );
    if let Some(exit) = fixture.expectation.process_exit {
        assert_eq!(
            i32::from(process_status),
            exit,
            "{label}: program process exit"
        );
    }
    assert_eq!(
        diagnostics.len(),
        fixture.expectation.value_diagnostics.len(),
        "{label}: value diagnostic count"
    );
    for (actual, expected) in diagnostics
        .iter()
        .zip(&fixture.expectation.value_diagnostics)
    {
        assert_runtime_diagnostic(actual, expected, &label);
    }
    let side_effects = events
        .iter()
        .map(|event| match event {
            RuntimeEvent::Emitted(value) => value.to_string(),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        side_effects, fixture.expectation.side_effects,
        "{label}: side effects"
    );

    ExecutionRecord {
        kind: "completed".into(),
        process_status,
        result_variant: result_variant.into(),
        representation,
        error_variant,
        value_diagnostics: diagnostics.iter().map(runtime_diagnostic_record).collect(),
        side_effects,
    }
}

fn value_projection(value: &Value, label: &str) -> (Option<String>, Option<String>) {
    match value {
        Value::Unit => (Some("()".into()), None),
        Value::Bool(value) => (Some(value.to_string()), None),
        Value::Integer(value) => (Some(value.value().to_string()), None),
        Value::Str(value) => (
            Some(serde_json::to_string(value.as_ref()).expect("strings serialize to JSON")),
            None,
        ),
        Value::NumericError(_) => (Some("NumericError".into()), None),
        Value::Tuple(_) | Value::Struct { .. } | Value::Variant { .. } => {
            panic!("{label}: selected corpus has no aggregate result payload")
        }
    }
}

fn assert_runtime_diagnostic(
    actual: &RuntimeDiagnostic,
    expected: &DiagnosticExpectation,
    label: &str,
) {
    assert_eq!(
        actual.code(),
        expected.code,
        "{label}: value diagnostic code"
    );
    for token in &expected.message_all_of {
        assert!(
            actual.message().contains(token),
            "{label}: value diagnostic message"
        );
    }
    for group in &expected.message_any_of {
        assert!(
            group.iter().any(|token| actual.message().contains(token)),
            "{label}: value diagnostic message matches none of {group:?}"
        );
    }
    for key in &expected.data_keys {
        assert!(
            actual.data().contains_key(key.as_str()),
            "{label}: value diagnostic data.{key}"
        );
    }
    for (key, expected) in &expected.data_equals {
        let actual = actual
            .data()
            .get(key.as_str())
            .unwrap_or_else(|| panic!("{label}: value diagnostic data.{key} is missing"));
        assert_eq!(
            actual.as_ref(),
            expected_scalar_text(expected),
            "{label}: value diagnostic data.{key}"
        );
    }
}

fn expected_scalar_text(value: &ExpectedScalar) -> String {
    match value {
        ExpectedScalar::String(value) => value.clone(),
        ExpectedScalar::Integer(value) => value.to_string(),
        ExpectedScalar::Float(value) => {
            serde_json::to_string(value).expect("finite expected floats serialize to JSON")
        }
        ExpectedScalar::Bool(value) => value.to_string(),
    }
}

fn runtime_diagnostic_record(diagnostic: &RuntimeDiagnostic) -> RuntimeDiagnosticRecord {
    RuntimeDiagnosticRecord {
        code: diagnostic.code().into(),
        message: diagnostic.message().into(),
        data: diagnostic
            .data()
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
    }
}

fn assert_expected_scalar(expected: &ExpectedScalar, actual: &JsonValue, context: &str) {
    let (kind, value) = match expected {
        ExpectedScalar::String(value) => ("str", JsonValue::String(value.clone())),
        ExpectedScalar::Integer(value) => ("int", JsonValue::from(*value)),
        ExpectedScalar::Float(value) => ("float", JsonValue::from(*value)),
        ExpectedScalar::Bool(value) => ("bool", JsonValue::from(*value)),
    };
    assert_eq!(actual["kind"], kind, "{context} kind");
    assert_eq!(actual["value"], value, "{context} value");
}

fn phase_records(observations: &[eglur_driver::PhaseObservation], label: &str) -> Vec<PhaseRecord> {
    assert!(
        !observations.is_empty(),
        "{label}: phase observations are empty"
    );
    assert!(
        observations
            .windows(2)
            .all(|pair| pair[0].phase() < pair[1].phase()),
        "{label}: phase observations are not in canonical order"
    );
    observations
        .iter()
        .map(|observation| {
            let phase = observation.phase();
            let elapsed = observation.elapsed();
            PhaseRecord {
                phase: phase.as_str().into(),
                input_count: observation.input_count(),
                input_unit: count_unit(phase.input_unit()).into(),
                output_count: observation.output_count(),
                output_unit: count_unit(phase.output_unit()).into(),
                elapsed_seconds: elapsed.as_secs(),
                elapsed_nanoseconds: elapsed.subsec_nanos(),
            }
        })
        .collect()
}

fn count_unit(unit: CountUnit) -> &'static str {
    match unit {
        CountUnit::Bytes => "bytes",
        CountUnit::Tokens => "tokens",
        CountUnit::Events => "events",
        CountUnit::SyntaxTrees => "syntax_trees",
        CountUnit::HirFiles => "hir_files",
        CountUnit::SourceFiles => "source_files",
        CountUnit::Declarations => "declarations",
        CountUnit::ImportRequests => "import_requests",
        CountUnit::ImportTargetFacts => "import_target_facts",
        CountUnit::ImportSccs => "import_sccs",
        CountUnit::ImportBindingFacts => "import_binding_facts",
        CountUnit::NameFacts => "name_facts",
        CountUnit::SignatureFacts => "signature_facts",
        CountUnit::TraitFacts => "trait_facts",
        CountUnit::BodyTypeFacts => "body_type_facts",
        CountUnit::OwnershipFacts => "ownership_facts",
        CountUnit::Invocations => "invocations",
        CountUnit::TraitEvidenceFacts => "trait_evidence_facts",
        CountUnit::EffectFacts => "effect_facts",
    }
}

#[cfg(test)]
mod tests {
    use super::expected_scalar_text;
    use crate::semantic_slice_support::model::ExpectedScalar;

    #[test]
    fn runtime_diagnostic_scalars_project_to_canonical_text() {
        assert_eq!(
            expected_scalar_text(&ExpectedScalar::String("x".into())),
            "x"
        );
        assert_eq!(expected_scalar_text(&ExpectedScalar::Integer(-3)), "-3");
        assert_eq!(expected_scalar_text(&ExpectedScalar::Float(1.0)), "1.0");
        assert_eq!(expected_scalar_text(&ExpectedScalar::Bool(true)), "true");
    }
}
