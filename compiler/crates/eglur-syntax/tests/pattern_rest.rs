use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, Parse, SyntaxDiagnosticKind, SyntaxKind, parse};

fn parse_pattern(pattern: &str) -> Parse {
    let text = format!(
        "fn inspect(input: Input) -> Unit = match input {{ {pattern} => () }}\n\
         fn following() -> Unit = ()\n"
    );
    let source = SourceFile::new(FileId::new(0), text.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parsed) = parse(&source) else {
        panic!("test source should reach the parser");
    };
    assert_eq!(parsed.syntax().text(), text);

    let following = parsed.syntax().children().last().unwrap();
    assert_eq!(following.kind(), SyntaxKind::FnDecl, "{pattern}");
    assert_eq!(following.text().trim(), "fn following() -> Unit = ()");
    parsed
}

#[test]
fn named_rest_accepts_every_position_for_each_braced_head() {
    for head in ["", "Record ", "Owner::Case "] {
        for fields in [
            "..",
            ".., first, last: renamed",
            "first, .., last: renamed",
            "first, last: renamed, ..",
            "\n.., // remaining fields\nfirst,\nlast: renamed,\n",
        ] {
            let pattern = format!("{head}{{ {fields} }}");
            let parsed = parse_pattern(&pattern);
            assert!(parsed.is_success(), "{pattern}: {:?}", parsed.diagnostics());
        }
    }
}

#[test]
fn nested_lists_keep_independent_rest_rules() {
    for pattern in [
        "Outer { .., child: Inner { .., leaf }, sibling: Other { value, .. }, tail }",
        "Owner::Case { child: { .., leaf }, .., pair: (first, ..) }",
        "(Record { .., value }, ..)",
        "Owner::Case(Record { .., value }, ..)",
        "(value, ..)",
        "Owner::Case(value, ..)",
        "(..)",
        "Owner::Case(..)",
    ] {
        let parsed = parse_pattern(pattern);
        assert!(parsed.is_success(), "{pattern}: {:?}", parsed.diagnostics());
    }
}

#[test]
fn duplicate_named_rest_and_non_tail_positional_rest_still_fail() {
    let mut patterns = Vec::new();
    for head in ["", "Record ", "Owner::Case "] {
        for fields in [".., ..", ".., first, ..", "first, .., last: value, .."] {
            patterns.push(format!("{head}{{ {fields} }}"));
        }
    }
    patterns.extend(
        [
            "(.., value)",
            "(first, .., last)",
            "(.., ..)",
            "Owner::Case(.., value)",
            "Owner::Case(first, .., last)",
            "Owner::Case(.., ..)",
            "Outer { .., child: Inner { .., leaf, .. } }",
            "Outer { .., child: Inner { leaf, .. }, .. }",
            "Outer { .., pair: (.., tail) }",
            "(Record { .., value }, .., tail)",
        ]
        .map(str::to_owned),
    );

    for pattern in patterns {
        let parsed = parse_pattern(&pattern);
        assert_eq!(parsed.diagnostics().len(), 1, "{pattern}");
        assert_eq!(
            parsed.diagnostics()[0].kind(),
            SyntaxDiagnosticKind::ParseInvalidRestPosition,
            "{pattern}"
        );
    }
}
