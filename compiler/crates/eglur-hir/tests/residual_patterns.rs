use eglur_hir::{FieldPattern, PatternKind, PatternPayload, lower};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, parse};

#[test]
fn match_residual_heads_preserve_written_paths_and_payloads() {
    for (pattern, expected) in [
        ("None", vec![("None", "absent")]),
        ("(None)", vec![("None", "absent")]),
        ("None @ None", vec![("None", "absent")]),
        (
            "Some(None)",
            vec![("Some", "positional"), ("None", "absent")],
        ),
        ("(None, Ok)", vec![("None", "absent"), ("Ok", "absent")]),
        ("None | Err", vec![("None", "absent"), ("Err", "absent")]),
        (
            "Some { value: None }",
            vec![("Some", "named"), ("None", "absent")],
        ),
        ("Record { value: None }", vec![("None", "absent")]),
        ("None {}", vec![("None", "named")]),
        ("None()", vec![("None", "positional")]),
        ("std::opt::None", vec![("std::opt::None", "absent")]),
    ] {
        let text = format!("fn f(input: Opt[i64]) -> i64 = match input {{ {pattern} => 0 }}\n");
        let source = SourceFile::new(FileId::new(0), text.as_bytes().to_vec());
        let FrontendOutcome::Parsed(parse) = parse(&source) else {
            panic!("valid UTF-8")
        };
        assert!(parse.is_success(), "{pattern}: {:?}", parse.diagnostics());
        let hir = lower(&parse);
        assert!(hir.invalid_occurrences().is_empty());
        let mut observed = Vec::new();
        for (_, body) in hir.bodies() {
            for (_, pattern) in body.patterns() {
                if let PatternKind::Variant { path, payload } = pattern.kind() {
                    let span = hir.origin(path.origin()).source().range();
                    let spelling = &text[span.start().as_usize()..span.end().as_usize()];
                    assert_eq!(Some(spelling), path.text().as_deref());
                    let payload = match payload {
                        PatternPayload::Absent => "absent",
                        PatternPayload::Positional(_) => "positional",
                        PatternPayload::Named(_) => "named",
                    };
                    observed.push((spelling, payload));
                }
            }
        }
        observed.sort_unstable();
        let mut expected = expected;
        expected.sort_unstable();
        assert_eq!(observed, expected, "{pattern}");
    }
}

#[test]
fn residual_spelling_does_not_reclassify_explicit_binding_roles() {
    for name in ["Some", "None", "Ok", "Err"] {
        let text = format!(
            "fn parameter({name}: i64) -> i64 = {name}\n\
             fn local() -> i64 {{ let {name} = 1\nyield {name} }}\n\
             fn alias(input: i64) -> i64 = match input {{ {name} @ _ => {name} }}\n\
             fn record(input: Record) -> i64 = match input {{ Record {{ {name} }} => {name} }}\n"
        );
        let source = SourceFile::new(FileId::new(0), text.as_bytes().to_vec());
        let FrontendOutcome::Parsed(parse) = parse(&source) else {
            panic!("valid UTF-8")
        };
        assert!(parse.is_success(), "{text}: {:?}", parse.diagnostics());
        let hir = lower(&parse);
        assert!(hir.invalid_occurrences().is_empty());
        let mut bindings = 0;
        let mut aliases = 0;
        let mut shorthands = 0;
        for (_, body) in hir.bodies() {
            for (_, pattern) in body.patterns() {
                match pattern.kind() {
                    PatternKind::Binding(found) if found.text() == Some(name) => bindings += 1,
                    PatternKind::Alias { name: found, .. } if found.text() == Some(name) => {
                        aliases += 1
                    }
                    PatternKind::Struct { fields, .. } => {
                        for field in fields {
                            if matches!(field, FieldPattern::Field { name: found, pattern: None, .. } if found.text() == Some(name))
                            {
                                shorthands += 1;
                            }
                        }
                    }
                    PatternKind::Variant { path, .. } => {
                        panic!("binding became constructor: {path:?}")
                    }
                    _ => {}
                }
            }
        }
        assert_eq!((bindings, aliases, shorthands), (2, 1, 1), "{name}");
    }
}
