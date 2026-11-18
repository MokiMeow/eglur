use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, parse};

#[test]
fn tuple_entries_require_their_own_group_for_leading_binary_continuation() {
    for operator in ["+", "-", "*", "/", "%", "==", "!=", "<", "&&", "||"] {
        for ending in ["\n", "\r\n", "\r", "/*\n*/"] {
            for expression in [
                format!("(left{ending}{operator} right, tail)"),
                format!("(tail, left{ending}{operator} right)"),
                format!("(left{ending}{operator} right,)"),
            ] {
                let text = format!("fn demo() -> Unit = {expression}\n");
                let source = SourceFile::new(FileId::new(0), text.as_bytes());
                let FrontendOutcome::Parsed(parsed) = parse(&source) else {
                    panic!("UTF-8 source must reach parsing");
                };
                assert_eq!(parsed.syntax().text(), text);
                assert!(!parsed.is_success(), "accepted tuple continuation: {text}");
            }
        }
    }
}

#[test]
fn groups_nested_commas_and_trailing_operators_preserve_continuation() {
    for expression in [
        "(left\n - right)",
        "((left\n - right), tail)",
        "(tail, (left\n - right))",
        "(left -\n right, tail)",
        "(left\n .value, tail)",
        "(call(left, right)\n - tail)",
        "([left, right]\n - tail)",
        "(Record { first: left, second: right }\n - tail)",
        "(\"comma, inside a string\"\n - tail)",
        "(left /* comma, in a comment */\n - right)",
    ] {
        let text = format!("fn demo() -> Unit = {expression}\n");
        let source = SourceFile::new(FileId::new(0), text.as_bytes());
        let FrontendOutcome::Parsed(parsed) = parse(&source) else {
            panic!("UTF-8 source must reach parsing");
        };
        assert_eq!(parsed.syntax().text(), text);
        assert!(parsed.is_success(), "{text}: {:?}", parsed.diagnostics());
    }
}
