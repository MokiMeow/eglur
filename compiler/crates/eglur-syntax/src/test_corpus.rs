use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Corpus {
    pub(crate) schema_version: String,
    pub(crate) input_digest: String,
    pub(crate) cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Case {
    pub(crate) origin: Origin,
    pub(crate) entrypoint: EntryPoint,
    pub(crate) source: Source,
    pub(crate) expectation: Expectation,
    pub(crate) public_examples: Vec<PublicExample>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Origin {
    InlineConformance {
        suite_id: String,
        test_id: String,
    },
    FileConformance {
        suite_id: String,
        test_id: String,
        path: String,
    },
    RegistryConformance {
        suite_id: String,
        test_id: String,
        package: String,
        version: String,
        path: String,
    },
    SyntaxCase {
        case_id: String,
    },
}

impl Origin {
    pub(crate) fn label(&self) -> String {
        match self {
            Self::InlineConformance { suite_id, test_id } => {
                format!("{suite_id}/{test_id}")
            }
            Self::FileConformance {
                suite_id,
                test_id,
                path,
            } => format!("{suite_id}/{test_id}:{path}"),
            Self::RegistryConformance {
                suite_id,
                test_id,
                package,
                version,
                path,
            } => format!("{suite_id}/{test_id}:{package}@{version}/{path}"),
            Self::SyntaxCase { case_id } => format!("syntax-case/{case_id}"),
        }
    }

    pub(crate) fn source_name(&self) -> Option<&str> {
        match self {
            Self::InlineConformance { .. } => Some("code"),
            Self::FileConformance { path, .. } | Self::RegistryConformance { path, .. } => {
                Some(path)
            }
            Self::SyntaxCase { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EntryPoint {
    CompilationUnit,
    Tokens,
    Expression,
    ExpressionSequence,
    ControlHead,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Source {
    Utf8 { text: String },
    Bytes { hex: String },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Expectation {
    pub(crate) kind: OutcomeKind,
    pub(crate) diagnostic_code: Option<String>,
    pub(crate) primary_span: Option<PrimarySpan>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutcomeKind {
    Success,
    LexicalFailure,
    ParseFailure,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PrimarySpan {
    Marker {
        marker: String,
        source: String,
        line: u32,
    },
    Bytes {
        source: String,
        bytes: ExpectedByteRange,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExpectedByteRange {
    pub(crate) start: u32,
    pub(crate) end: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PublicExample {
    pub(crate) document: String,
    pub(crate) opening_line: u32,
}

pub(crate) fn corpus_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../tests/frontend/expectations.json")
}

fn decode_hex(value: &str) -> Vec<u8> {
    assert_eq!(
        value.len() % 2,
        0,
        "hex byte source must have complete pairs"
    );
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("hex source must be ASCII");
            u8::from_str_radix(text, 16).expect("hex source must use byte digits")
        })
        .collect()
}

pub(crate) fn source_bytes(source: Source) -> Vec<u8> {
    match source {
        Source::Utf8 { text } => text.into_bytes(),
        Source::Bytes { hex } => decode_hex(&hex),
    }
}

pub(crate) fn read_corpus() -> Corpus {
    serde_json::from_slice(&fs::read(corpus_path()).expect("front-end corpus should be readable"))
        .expect("front-end corpus should match its closed JSON schema")
}
