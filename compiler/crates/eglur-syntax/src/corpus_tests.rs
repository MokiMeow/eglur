use eglur_source::{FileId, FileSpan, LineIndex, SourceFile, SourceValidationError};

use crate::frontend::parse_with_entry;
use crate::parser::{MAX_PARSE_DEPTH, MAX_SYNTAX_DIAGNOSTICS, ParseEntryPoint};
use crate::test_corpus::{EntryPoint, Origin, OutcomeKind, PrimarySpan, read_corpus, source_bytes};
use crate::{FrontendOutcome, ParseResource, SyntaxDiagnosticKind, SyntaxKind, SyntaxPhase, lex};

fn assert_primary_span(
    label: &str,
    origin: &Origin,
    source: &SourceFile,
    expected: &PrimarySpan,
    actual: FileSpan,
) {
    let expected_source = match expected {
        PrimarySpan::Marker { source, .. } | PrimarySpan::Bytes { source, .. } => source,
    };
    assert_eq!(
        origin.source_name(),
        Some(expected_source.as_str()),
        "{label}"
    );
    assert_eq!(actual.file(), source.id(), "{label}");

    match expected {
        PrimarySpan::Marker { marker, line, .. } => {
            assert!(!marker.is_empty(), "{label}");
            assert!(*line > 0, "{label}");
            let validated = source
                .validate()
                .expect("marker-based spans require valid UTF-8 source");
            let coordinate = LineIndex::new(validated)
                .coordinate(actual.range().start())
                .expect("diagnostic starts at a Unicode scalar boundary");
            assert_eq!(coordinate.line(), *line, "{label}");
        }
        PrimarySpan::Bytes { bytes, .. } => {
            assert!(bytes.start <= bytes.end, "{label}");
            assert_eq!(actual.range().start().get(), bytes.start, "{label}");
            assert_eq!(actual.range().end().get(), bytes.end, "{label}");
        }
    }
}

#[test]
fn projected_corpus_has_exact_lexical_outcomes() {
    let corpus = read_corpus();
    assert_eq!(corpus.schema_version, "0.1");
    assert!(corpus.input_digest.starts_with("sha256:"));
    assert_eq!(corpus.cases.len(), 2147);

    let mut lexical_failures = 0;
    for (index, case) in corpus.cases.into_iter().enumerate() {
        let label = case.origin.label();
        assert!(
            !case
                .public_examples
                .iter()
                .any(|example| { example.document.is_empty() || example.opening_line == 0 })
        );
        if let Some(span) = &case.expectation.primary_span {
            let source_name = match span {
                PrimarySpan::Marker { source, .. } | PrimarySpan::Bytes { source, .. } => source,
            };
            assert!(!source_name.is_empty(), "{label}");
        }
        if case.entrypoint == EntryPoint::Tokens {
            assert!(matches!(case.origin, Origin::SyntaxCase { .. }), "{label}");
        }

        let source = SourceFile::new(
            FileId::new(u32::try_from(index).expect("fixture count should fit u32")),
            source_bytes(case.source),
        );
        match source.validate() {
            Err(SourceValidationError::InvalidUtf8(span)) => {
                lexical_failures += 1;
                assert_eq!(
                    case.expectation.kind,
                    OutcomeKind::LexicalFailure,
                    "{label}"
                );
                if let Some(expected) = &case.expectation.primary_span {
                    assert_primary_span(&label, &case.origin, &source, expected, span);
                }
                assert_eq!(
                    case.expectation.diagnostic_code.as_deref(),
                    Some("LEX_INVALID_UTF8"),
                    "{label}"
                );
            }
            Err(error @ SourceValidationError::TooLarge { .. }) => {
                panic!("projected fixture exceeded source bounds: {label}: {error}");
            }
            Ok(validated) => {
                let lexed = lex(validated);
                let reconstructed = lexed
                    .tokens()
                    .iter()
                    .map(|token| {
                        lexed
                            .text(*token)
                            .expect("token should belong to its lexical buffer")
                    })
                    .collect::<String>();
                assert_eq!(reconstructed.as_bytes(), source.bytes(), "{label}");

                if case.expectation.kind == OutcomeKind::LexicalFailure {
                    lexical_failures += 1;
                    assert_eq!(lexed.diagnostics().len(), 1, "{label}");
                    assert_eq!(
                        Some(lexed.diagnostics()[0].kind().code()),
                        case.expectation.diagnostic_code.as_deref(),
                        "{label}"
                    );
                    if let Some(expected) = &case.expectation.primary_span {
                        assert_primary_span(
                            &label,
                            &case.origin,
                            &source,
                            expected,
                            lexed.diagnostics()[0].span(),
                        );
                    }
                } else {
                    assert!(lexed.is_success(), "{label}: {:?}", lexed.diagnostics());
                }
            }
        }
    }

    assert_eq!(lexical_failures, 26);
}

#[test]
fn projected_corpus_has_exact_parser_outcomes_and_lossless_trees() {
    let corpus = read_corpus();
    let mut lexical_failures = 0;
    let mut parse_failures = 0;

    for (index, case) in corpus.cases.into_iter().enumerate() {
        let label = case.origin.label();
        match case.expectation.kind {
            OutcomeKind::LexicalFailure => lexical_failures += 1,
            OutcomeKind::ParseFailure => parse_failures += 1,
            OutcomeKind::Success => {}
        }
        if case.entrypoint == EntryPoint::Tokens {
            assert_ne!(case.expectation.kind, OutcomeKind::ParseFailure, "{label}");
            continue;
        }

        let entrypoint = match case.entrypoint {
            EntryPoint::CompilationUnit => ParseEntryPoint::CompilationUnit,
            EntryPoint::Expression => ParseEntryPoint::Expression,
            EntryPoint::ExpressionSequence => ParseEntryPoint::ExpressionSequence,
            EntryPoint::ControlHead => ParseEntryPoint::ControlHead,
            EntryPoint::Tokens => unreachable!("token entrypoints were handled above"),
        };
        let source = SourceFile::new(
            FileId::new(u32::try_from(index).expect("fixture count should fit u32")),
            source_bytes(case.source),
        );

        match parse_with_entry(&source, entrypoint) {
            FrontendOutcome::Rejected(SourceValidationError::InvalidUtf8(span)) => {
                assert_eq!(
                    case.expectation.kind,
                    OutcomeKind::LexicalFailure,
                    "{label}"
                );
                if let Some(expected) = &case.expectation.primary_span {
                    assert_primary_span(&label, &case.origin, &source, expected, span);
                }
                assert_eq!(
                    case.expectation.diagnostic_code.as_deref(),
                    Some("LEX_INVALID_UTF8"),
                    "{label}"
                );
            }
            FrontendOutcome::Rejected(error @ SourceValidationError::TooLarge { .. }) => {
                panic!("projected fixture exceeded source bounds: {label}: {error}");
            }
            FrontendOutcome::Parsed(parse) => {
                assert_eq!(parse.syntax().text().as_bytes(), source.bytes(), "{label}");
                match case.expectation.kind {
                    OutcomeKind::Success => {
                        assert!(parse.is_success(), "{label}: {:?}", parse.diagnostics());
                    }
                    OutcomeKind::LexicalFailure => {
                        assert_eq!(parse.diagnostics().len(), 1, "{label}");
                        assert_eq!(parse.diagnostics()[0].phase(), SyntaxPhase::Lex, "{label}");
                        assert_eq!(
                            Some(parse.diagnostics()[0].kind().code()),
                            case.expectation.diagnostic_code.as_deref(),
                            "{label}"
                        );
                        if let Some(expected) = &case.expectation.primary_span {
                            assert_primary_span(
                                &label,
                                &case.origin,
                                &source,
                                expected,
                                parse.diagnostics()[0].span(),
                            );
                        }
                    }
                    OutcomeKind::ParseFailure => {
                        assert_eq!(parse.diagnostics().len(), 1, "{label}");
                        let diagnostic = parse.diagnostics()[0];
                        assert_eq!(diagnostic.phase(), SyntaxPhase::Parse, "{label}");
                        assert_eq!(
                            Some(diagnostic.kind().code()),
                            case.expectation.diagnostic_code.as_deref(),
                            "{label}"
                        );
                        if let Some(expected) = &case.expectation.primary_span {
                            assert_primary_span(
                                &label,
                                &case.origin,
                                &source,
                                expected,
                                diagnostic.span(),
                            );
                        }
                    }
                }
            }
        }
    }

    assert_eq!(lexical_failures, 26);
    assert_eq!(parse_failures, 84);
}

#[test]
fn parser_depth_limit_covers_all_recursive_prefix_families() {
    let unary = format!(
        "fn main() -> Bool = {}true\n",
        "!".repeat(usize::from(MAX_PARSE_DEPTH))
    );
    let reference = format!(
        "fn main(value: {}i64) -> Unit {{}}\n",
        "& ".repeat(usize::from(MAX_PARSE_DEPTH) + 1)
    );
    let conditional = format!(
        "fn main() -> Unit = {}true {{}}{}\n",
        "if ",
        " else if true {}".repeat(usize::from(MAX_PARSE_DEPTH) + 1)
    );
    let attribute = format!(
        "#[{}true{}]\nfn main() -> Unit {{}}\n",
        "meta(".repeat(usize::from(MAX_PARSE_DEPTH) + 1),
        ")".repeat(usize::from(MAX_PARSE_DEPTH) + 1),
    );
    let alias_pattern = format!(
        "fn main(value: i64) -> Unit {{\n  let {}_ = value\n}}\n",
        "binding @ ".repeat(usize::from(MAX_PARSE_DEPTH) + 1),
    );

    for (index, (label, source_text, expected_resource)) in [
        ("unary", unary, ParseResource::RecursiveDepth),
        ("reference", reference, ParseResource::RecursiveDepth),
        ("conditional", conditional, ParseResource::RecursiveDepth),
        ("attribute", attribute, ParseResource::DelimiterDepth),
        (
            "alias pattern",
            alias_pattern,
            ParseResource::RecursiveDepth,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let source = SourceFile::new(
            FileId::new(u32::try_from(index).expect("test index should fit u32")),
            source_text.into_bytes(),
        );
        let FrontendOutcome::Parsed(parse) =
            parse_with_entry(&source, ParseEntryPoint::CompilationUnit)
        else {
            panic!("depth fixture should be valid UTF-8");
        };

        assert_eq!(parse.syntax().text().as_bytes(), source.bytes(), "{label}");
        assert_eq!(parse.diagnostics().len(), 1, "{label}");
        assert_eq!(
            parse.diagnostics()[0].phase(),
            SyntaxPhase::Parse,
            "{label}"
        );
        assert_eq!(
            parse.diagnostics()[0].kind(),
            SyntaxDiagnosticKind::ParseResourceLimitExceeded,
            "{label}"
        );
        let resource = parse.diagnostics()[0]
            .resource_limit()
            .expect("resource diagnostic should carry structured data");
        assert_eq!(resource.resource(), expected_resource, "{label}");
        assert_eq!(resource.limit(), i64::from(MAX_PARSE_DEPTH), "{label}");
        assert_eq!(
            resource.observed_at_least(),
            i64::from(MAX_PARSE_DEPTH) + 1,
            "{label}"
        );
    }
}

#[test]
fn recursive_prefix_boundary_accepts_the_last_in_budget_expression() {
    let source_text = format!(
        "fn main() -> Bool = {}true\n",
        "!".repeat(usize::from(MAX_PARSE_DEPTH) - 1)
    );
    let source = SourceFile::new(FileId::new(0), source_text.into_bytes());
    let FrontendOutcome::Parsed(parse) =
        parse_with_entry(&source, ParseEntryPoint::CompilationUnit)
    else {
        panic!("recursive boundary fixture should be valid UTF-8");
    };

    assert!(parse.is_success(), "{:?}", parse.diagnostics());
    assert_eq!(parse.syntax().text().as_bytes(), source.bytes());
}

#[test]
fn compilation_unit_recovers_only_at_proven_top_level_boundaries() {
    let source = SourceFile::new(
        FileId::new(0),
        b"fn first() -> i64 = ,\nfn middle() -> i64 = 1\nconst broken: i64 = ,\nfn last() -> i64 = 2\n"
            .to_vec(),
    );
    let FrontendOutcome::Parsed(parse) =
        parse_with_entry(&source, ParseEntryPoint::CompilationUnit)
    else {
        panic!("recovery fixture should be valid UTF-8");
    };

    assert_eq!(parse.syntax().text().as_bytes(), source.bytes());
    assert_eq!(parse.diagnostics().len(), 2);
    assert!(
        parse.diagnostics().iter().all(|diagnostic| {
            diagnostic.kind() == SyntaxDiagnosticKind::ParseExpectedExpression
        })
    );
    assert!(
        parse
            .diagnostics()
            .windows(2)
            .all(|pair| { pair[0].span().range().start() < pair[1].span().range().start() })
    );
    assert_eq!(
        parse
            .syntax()
            .children()
            .map(|child| child.kind())
            .collect::<Vec<_>>(),
        vec![
            SyntaxKind::ErrorNode,
            SyntaxKind::ErrorNode,
            SyntaxKind::FnDecl,
            SyntaxKind::ErrorNode,
            SyntaxKind::ErrorNode,
            SyntaxKind::FnDecl,
        ]
    );
}

#[test]
fn recovery_skips_a_balanced_failed_body_but_not_an_unclosed_body() {
    let balanced = SourceFile::new(
        FileId::new(0),
        b"fn bad() -> Unit {\n  let value = ,\n}\nfn good() -> Unit {}\n".to_vec(),
    );
    let FrontendOutcome::Parsed(balanced_parse) =
        parse_with_entry(&balanced, ParseEntryPoint::CompilationUnit)
    else {
        panic!("balanced recovery fixture should be valid UTF-8");
    };
    assert_eq!(balanced_parse.syntax().text().as_bytes(), balanced.bytes());
    assert_eq!(balanced_parse.diagnostics().len(), 1);
    assert_eq!(
        balanced_parse
            .syntax()
            .children()
            .map(|child| child.kind())
            .collect::<Vec<_>>(),
        vec![
            SyntaxKind::ErrorNode,
            SyntaxKind::ErrorNode,
            SyntaxKind::FnDecl
        ]
    );

    let unclosed = SourceFile::new(
        FileId::new(1),
        b"fn bad() -> Unit {\n  let value = ,\nfn not_proven_top_level() -> Unit {}\n".to_vec(),
    );
    let FrontendOutcome::Parsed(unclosed_parse) =
        parse_with_entry(&unclosed, ParseEntryPoint::CompilationUnit)
    else {
        panic!("unclosed recovery fixture should be valid UTF-8");
    };
    assert_eq!(unclosed_parse.syntax().text().as_bytes(), unclosed.bytes());
    assert_eq!(unclosed_parse.diagnostics().len(), 1);
    assert!(
        !unclosed_parse
            .syntax()
            .children()
            .any(|child| child.kind() == SyntaxKind::FnDecl)
    );
}

#[test]
fn recovery_boundary_matrix_rejects_keyword_shaped_false_boundaries() {
    let starters = [
        "fn recovered() -> Unit {}",
        "pub fn recovered() -> Unit {}",
        "#[test]\nfn recovered() -> Unit {}",
        "import std::io",
        "closed trait Recovered {}",
        "struct Recovered {}",
        "enum Recovered { A }",
        "type Recovered = i64",
        "const recovered: i64 = 1",
        "impl Trait for Value {}",
    ];

    for (line_ending_index, line_ending) in ["\n", "\r\n"].into_iter().enumerate() {
        for (starter_index, starter) in starters.into_iter().enumerate() {
            let source_text = format!("fn broken() -> i64 = ,{line_ending}{starter}{line_ending}");
            let source = SourceFile::new(
                FileId::new(
                    u32::try_from(line_ending_index * starters.len() + starter_index)
                        .expect("matrix index should fit u32"),
                ),
                source_text.into_bytes(),
            );
            let FrontendOutcome::Parsed(parse) =
                parse_with_entry(&source, ParseEntryPoint::CompilationUnit)
            else {
                panic!("recovery matrix fixture should be valid UTF-8");
            };

            assert_eq!(
                parse.syntax().text().as_bytes(),
                source.bytes(),
                "{starter}"
            );
            assert_eq!(parse.diagnostics().len(), 1, "{starter}");
            assert!(
                parse
                    .syntax()
                    .children()
                    .any(|child| child.kind() != SyntaxKind::ErrorNode),
                "{starter}"
            );
        }
    }

    let same_line = SourceFile::new(
        FileId::new(100),
        b"fn broken() -> i64 = , fn same_line() -> Unit {}\nfn recovered() -> Unit {}\n".to_vec(),
    );
    let FrontendOutcome::Parsed(same_line_parse) =
        parse_with_entry(&same_line, ParseEntryPoint::CompilationUnit)
    else {
        panic!("same-line fixture should be valid UTF-8");
    };
    assert_eq!(
        same_line_parse
            .syntax()
            .children()
            .filter(|child| child.kind() == SyntaxKind::FnDecl)
            .count(),
        1
    );

    let misplaced_module = SourceFile::new(
        FileId::new(101),
        b"fn broken() -> i64 = ,\nmodule not_a_boundary\nfn recovered() -> Unit {}\n".to_vec(),
    );
    let FrontendOutcome::Parsed(module_parse) =
        parse_with_entry(&misplaced_module, ParseEntryPoint::CompilationUnit)
    else {
        panic!("module-boundary fixture should be valid UTF-8");
    };
    assert_eq!(module_parse.diagnostics().len(), 1);
    assert!(
        !module_parse
            .syntax()
            .children()
            .any(|child| child.kind() == SyntaxKind::ModuleDecl)
    );

    let unmatched_close = SourceFile::new(
        FileId::new(102),
        b"fn broken() -> i64 = ,\n}\nfn not_proven_top_level() -> Unit {}\n".to_vec(),
    );
    let FrontendOutcome::Parsed(unmatched_close_parse) =
        parse_with_entry(&unmatched_close, ParseEntryPoint::CompilationUnit)
    else {
        panic!("unmatched-close fixture should be valid UTF-8");
    };
    assert!(
        !unmatched_close_parse
            .syntax()
            .children()
            .any(|child| child.kind() == SyntaxKind::FnDecl)
    );
}

#[test]
fn diagnostic_limit_stops_recovery_deterministically() {
    let mut source_text = String::new();
    for index in 0..MAX_SYNTAX_DIAGNOSTICS + 8 {
        source_text.push_str(&format!("fn broken_{index}() -> i64 = ,\n"));
    }
    let source = SourceFile::new(FileId::new(0), source_text.into_bytes());
    let FrontendOutcome::Parsed(parse) =
        parse_with_entry(&source, ParseEntryPoint::CompilationUnit)
    else {
        panic!("diagnostic-limit fixture should be valid UTF-8");
    };

    assert_eq!(parse.syntax().text().as_bytes(), source.bytes());
    assert_eq!(parse.diagnostics().len(), MAX_SYNTAX_DIAGNOSTICS);
    assert!(parse.diagnostics().iter().all(|diagnostic| {
        diagnostic.kind() == SyntaxDiagnosticKind::ParseExpectedExpression
            && diagnostic.resource_limit().is_none()
    }));
}

#[test]
fn ordinary_control_body_is_not_reclassified_as_a_restricted_suffix() {
    let source = SourceFile::new(FileId::new(0), b"if ready { value } {}".to_vec());
    let FrontendOutcome::Parsed(parse) = parse_with_entry(&source, ParseEntryPoint::ControlHead)
    else {
        panic!("control-head fixture should be valid UTF-8");
    };

    assert_eq!(parse.diagnostics().len(), 1);
    assert_eq!(
        parse.diagnostics()[0].kind(),
        SyntaxDiagnosticKind::ParseUnexpectedToken
    );
}

#[test]
fn contextual_reduce_by_preserves_every_syntactically_valid_candidate() {
    let sources = [
        "counter.reduce_by((h, f), g)",
        "schema.reduce_by((h, f), (w, g),)",
        "schema.reduce_by(((h w), f),)",
    ];

    for (index, source_text) in sources.into_iter().enumerate() {
        let source = SourceFile::new(
            FileId::new(u32::try_from(index).expect("test index should fit u32")),
            source_text.as_bytes().to_vec(),
        );
        let FrontendOutcome::Parsed(parse) = parse_with_entry(&source, ParseEntryPoint::Expression)
        else {
            panic!("reduce_by fixture should be valid UTF-8");
        };

        assert!(
            parse.is_success(),
            "{source_text}: {:?}",
            parse.diagnostics()
        );
    }
}

#[test]
fn short_ascii_inputs_terminate_and_remain_lossless() {
    const ALPHABET: &[u8] = b"a0 \n(){}[],:@|.\"\\#";

    for length in 0_u32..=3 {
        let case_count = ALPHABET.len().pow(length);
        for ordinal in 0..case_count {
            let mut value = ordinal;
            let mut bytes = Vec::with_capacity(length as usize);
            for _ in 0..length {
                bytes.push(ALPHABET[value % ALPHABET.len()]);
                value /= ALPHABET.len();
            }

            let source = SourceFile::new(FileId::new(0), bytes);
            let FrontendOutcome::Parsed(parse) =
                parse_with_entry(&source, ParseEntryPoint::CompilationUnit)
            else {
                panic!("ASCII fixture should be valid UTF-8");
            };

            assert_eq!(parse.syntax().text().as_bytes(), source.bytes());
        }
    }
}

#[test]
fn every_single_byte_input_has_one_controlled_frontend_outcome() {
    for byte in u8::MIN..=u8::MAX {
        let source = SourceFile::new(FileId::new(u32::from(byte)), vec![byte]);
        match parse_with_entry(&source, ParseEntryPoint::CompilationUnit) {
            FrontendOutcome::Parsed(parse) => {
                assert_eq!(parse.syntax().text().as_bytes(), source.bytes());
            }
            FrontendOutcome::Rejected(SourceValidationError::InvalidUtf8(_)) => {}
            FrontendOutcome::Rejected(SourceValidationError::TooLarge { .. }) => {
                panic!("one-byte input cannot exceed the source limit");
            }
        }
    }
}

#[test]
fn generated_arbitrary_bytes_terminate_without_losing_valid_utf8() {
    let mut state = 0x9e37_79b9_u32;

    for case_index in 0_u32..4_096 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let length = usize::try_from(state % 48).expect("generated length should fit usize");
        let mut bytes = Vec::with_capacity(length);
        for _ in 0..length {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            bytes.push(state.to_le_bytes()[0]);
        }

        let source = SourceFile::new(FileId::new(case_index), bytes);
        match parse_with_entry(&source, ParseEntryPoint::CompilationUnit) {
            FrontendOutcome::Parsed(parse) => {
                assert_eq!(parse.syntax().text().as_bytes(), source.bytes());
            }
            FrontendOutcome::Rejected(SourceValidationError::InvalidUtf8(_)) => {}
            FrontendOutcome::Rejected(SourceValidationError::TooLarge { .. }) => {
                panic!("bounded generated input cannot exceed the source limit");
            }
        }
    }
}

#[test]
fn long_flat_lists_and_postfix_chains_remain_linear_and_lossless() {
    let list = format!(
        "fn main() -> Vec[i64] = [{}]\n",
        std::iter::repeat_n("0", 4_096)
            .collect::<Vec<_>>()
            .join(",")
    );
    let list_source = SourceFile::new(FileId::new(0), list.into_bytes());
    let FrontendOutcome::Parsed(list_parse) =
        parse_with_entry(&list_source, ParseEntryPoint::CompilationUnit)
    else {
        panic!("flat-list fixture should be valid UTF-8");
    };
    assert!(list_parse.is_success(), "{:?}", list_parse.diagnostics());
    assert_eq!(list_parse.syntax().text().as_bytes(), list_source.bytes());

    let chain = format!(
        "fn main(value: Value) -> Value = value{}\n",
        ".member".repeat(4_096)
    );
    let chain_source = SourceFile::new(FileId::new(1), chain.into_bytes());
    let FrontendOutcome::Parsed(chain_parse) =
        parse_with_entry(&chain_source, ParseEntryPoint::CompilationUnit)
    else {
        panic!("postfix-chain fixture should be valid UTF-8");
    };
    assert!(chain_parse.is_success(), "{:?}", chain_parse.diagnostics());
    assert_eq!(chain_parse.syntax().text().as_bytes(), chain_source.bytes());
}

#[test]
fn mixed_and_truncated_delimiters_terminate_losslessly() {
    let sources = [
        "fn main() -> Unit {\n  let value = [({1})]\n}\n",
        "fn main() -> Unit {\n  let value = [({1])\n}\n",
        "fn main() -> Unit {\n  let value = (((((((1\n}\n",
        "fn main() -> Unit {\n  let value = ]})\n}\n",
    ];

    for (index, source_text) in sources.into_iter().enumerate() {
        let source = SourceFile::new(
            FileId::new(u32::try_from(index).expect("test index should fit u32")),
            source_text.as_bytes().to_vec(),
        );
        let FrontendOutcome::Parsed(parse) =
            parse_with_entry(&source, ParseEntryPoint::CompilationUnit)
        else {
            panic!("delimiter fixture should be valid UTF-8");
        };
        assert_eq!(
            parse.syntax().text().as_bytes(),
            source.bytes(),
            "{source_text}"
        );
    }
}
