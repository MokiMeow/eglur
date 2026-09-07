use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const CORPUS_SCHEMA_VERSION: &str = "0.1";
const CORPUS_FIXTURE_COUNT: usize = 23;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureIdentity {
    pub suite_id: String,
    pub test_id: String,
}

impl FixtureIdentity {
    pub fn label(&self) -> String {
        format!("{}/{}", self.suite_id, self.test_id)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    pub mode: String,
    pub runner: String,
    pub source: FixtureSource,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FixtureSource {
    Utf8 {
        text: String,
    },
    Files {
        entry: String,
        file_set_root: String,
        files: BTreeMap<String, String>,
        symlinks: BTreeMap<String, String>,
    },
}

impl FixtureSource {
    pub fn source_text(&self, source: &str) -> Option<&str> {
        match self {
            Self::Utf8 { text } => (source == "code").then_some(text),
            Self::Files { files, .. } => files.get(source).map(String::as_str),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ExpectedScalar {
    String(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticExpectation {
    pub code: String,
    pub message_all_of: Vec<String>,
    pub message_any_of: Vec<Vec<String>>,
    pub data_keys: Vec<String>,
    pub data_equals: BTreeMap<String, ExpectedScalar>,
    pub help_contains: Option<Vec<String>>,
    pub related_contains: Option<Vec<String>>,
    pub primary_span: Option<PrimarySpanExpectation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrimarySpanExpectation {
    Marker {
        source: String,
        marker: String,
        line: u32,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ValueExpectation {
    pub kind: String,
    pub representation: Option<String>,
    pub error_variant: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OutcomeExpectation {
    Value { value: ValueExpectation },
    CompileError { diagnostic: DiagnosticExpectation },
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Expectation {
    pub outcome: OutcomeExpectation,
    pub value_diagnostics: Vec<DiagnosticExpectation>,
    pub diagnostic_contains: Option<Vec<String>>,
    pub side_effects: Vec<String>,
    pub process_exit: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SemanticFixture {
    pub identity: FixtureIdentity,
    pub invocation: Invocation,
    pub expectation: Expectation,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SemanticFixtureCorpus {
    pub schema_version: String,
    pub input_digest: String,
    pub fixtures: Vec<SemanticFixture>,
    #[serde(skip)]
    pub path: PathBuf,
}

impl SemanticFixtureCorpus {
    pub fn load(repository_root: &Path) -> Result<Self, String> {
        let path = repository_root.join("tests/semantic/core-v1.expectations.json");
        let content = fs::read(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let mut corpus = serde_json::from_slice::<Self>(&content)
            .map_err(|error| format!("failed to decode {}: {error}", path.display()))?;
        corpus.path = path;
        corpus.validate()?;
        Ok(corpus)
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != CORPUS_SCHEMA_VERSION {
            return Err(format!(
                "semantic corpus schema is {:?}, expected {CORPUS_SCHEMA_VERSION:?}",
                self.schema_version
            ));
        }
        if !valid_digest(&self.input_digest) {
            return Err("semantic corpus input_digest is not a SHA-256 address".into());
        }
        if self.fixtures.len() != CORPUS_FIXTURE_COUNT {
            return Err(format!(
                "semantic corpus contains {} fixtures, expected {CORPUS_FIXTURE_COUNT}",
                self.fixtures.len()
            ));
        }
        let identities = self
            .fixtures
            .iter()
            .map(|fixture| &fixture.identity)
            .collect::<Vec<_>>();
        if !identities.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err("semantic corpus identities must be sorted and unique".into());
        }
        for fixture in &self.fixtures {
            if fixture.invocation.mode != "auto" || fixture.invocation.runner != "main" {
                return Err(format!(
                    "{} is outside the auto/main semantic-slice invocation",
                    fixture.identity.label()
                ));
            }
            if let FixtureSource::Files {
                files,
                entry,
                file_set_root,
                symlinks,
            } = &fixture.invocation.source
            {
                if files.is_empty() || !files.contains_key(entry) {
                    return Err(format!(
                        "{} has an empty file set or missing entry",
                        fixture.identity.label()
                    ));
                }
                if file_set_root != "." || !symlinks.is_empty() {
                    return Err(format!(
                        "{} uses unsupported file-set roots or symlinks",
                        fixture.identity.label()
                    ));
                }
            }
        }
        Ok(())
    }
}

fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

#[cfg(test)]
mod tests {
    use super::{FixtureSource, OutcomeExpectation, PrimarySpanExpectation, SemanticFixtureCorpus};

    #[test]
    fn strict_decoder_rejects_unknown_fields() {
        let error = serde_json::from_str::<SemanticFixtureCorpus>(
            r#"{"schema_version":"0.1","input_digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","fixtures":[],"extra":true}"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("unknown field `extra`"));
    }

    #[test]
    fn strict_decoder_rejects_unknown_tagged_variant_fields() {
        for error in [
            serde_json::from_str::<FixtureSource>(
                r#"{"kind":"utf8","text":"","extra":true}"#,
            )
            .unwrap_err(),
            serde_json::from_str::<PrimarySpanExpectation>(
                r#"{"kind":"marker","source":"code","marker":"x","line":1,"extra":true}"#,
            )
            .unwrap_err(),
            serde_json::from_str::<OutcomeExpectation>(
                r#"{"kind":"value","value":{"kind":"Ok","representation":"\"ok\"","error_variant":null},"extra":true}"#,
            )
            .unwrap_err(),
        ] {
            assert!(error.to_string().contains("unknown field `extra`"));
        }
    }
}
