use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, Parse, SyntaxDiagnosticKind, SyntaxKind, SyntaxNode, parse};

const FAMILIES: [&str; 6] = [
    "view",
    "rearrange",
    "inflate",
    "reduce",
    "contract",
    "einop",
];

fn parse_schema(family: &str, input: &str, output: &str) -> Parse {
    let text = format!(
        "fn schema() -> Unit = {family}{{ {input} -> {output} }}\n\
         fn following() -> Unit = ()\n"
    );
    let source = SourceFile::new(FileId::new(5), text.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parsed) = parse(&source) else {
        panic!("test source must reach the parser");
    };
    assert_eq!(parsed.syntax().text(), text);

    let following = parsed.syntax().children().last().unwrap();
    assert_eq!(following.kind(), SyntaxKind::FnDecl, "{text}");
    assert_eq!(following.text().trim(), "fn following() -> Unit = ()");
    parsed
}

fn axis_lists(parsed: &Parse) -> Vec<SyntaxNode> {
    let mut pending = vec![parsed.syntax()];
    let mut lists = Vec::new();
    while let Some(node) = pending.pop() {
        if node.kind() == SyntaxKind::TransformAxisList {
            lists.push(node);
        } else {
            let children: Vec<_> = node.children().collect();
            pending.extend(children.into_iter().rev());
        }
    }
    lists
}

#[test]
fn standalone_packs_keep_list_entry_boundaries_in_every_schema_family() {
    for family in FAMILIES {
        for (axes, spellings) in [
            ("...", vec!["..."]),
            ("...tail", vec!["...tail"]),
            (
                "head ...tail (a + 2 * b) ...",
                vec!["head", "...tail", "(a + 2 * b)", "..."],
            ),
            ("...tail(a * b)", vec!["...tail", "(a * b)"]),
            ("... tail", vec!["...", "tail"]),
            (".../* gap */tail", vec!["...", "/* gap */tail"]),
            ("...first ...second", vec!["...first", "...second"]),
        ] {
            for (input, output) in [(axes, "end"), ("start", axes)] {
                let parsed = parse_schema(family, input, output);
                assert!(
                    parsed.is_success(),
                    "{family}: {input} -> {output}: {:?}",
                    parsed.diagnostics()
                );
                let lists = axis_lists(&parsed);
                assert_eq!(lists.len(), 2);
                let list = &lists[usize::from(output == axes)];
                let entries: Vec<_> = list.children().collect();
                assert_eq!(entries.len(), spellings.len());
                for (entry, spelling) in entries.iter().zip(&spellings) {
                    assert_eq!(entry.kind(), SyntaxKind::AxisAtom);
                    assert_eq!(entry.text().trim(), *spelling);
                    if spelling.starts_with("...") {
                        assert_eq!(entry.children().count(), 0);
                    }
                }
            }
        }
    }
}

#[test]
fn packs_are_rejected_as_grouped_or_arithmetic_factors_on_either_side() {
    for family in FAMILIES {
        for pack in ["...", "...tail"] {
            for (axes, offending) in [
                (format!("({pack})"), pack),
                (format!("(({pack}))"), pack),
                (format!("a + {pack}"), pack),
                (format!("2 * {pack}"), pack),
                (format!("a + (b * {pack})"), pack),
                (format!("a * (b + {pack})"), pack),
                (format!("{pack} + a"), "+"),
                (format!("{pack} * b"), "*"),
            ] {
                for (input, output) in [(&*axes, "end"), ("start", &*axes)] {
                    let parsed = parse_schema(family, input, output);
                    assert!(
                        !parsed.is_success(),
                        "accepted {family}: {input} -> {output}"
                    );
                    let [diagnostic] = parsed.diagnostics() else {
                        panic!("expected one diagnostic: {:?}", parsed.diagnostics());
                    };
                    assert_eq!(
                        diagnostic.kind(),
                        SyntaxDiagnosticKind::ParseUnexpectedToken
                    );
                    let text = parsed.syntax().text();
                    let start = text.find(&axes).unwrap() + axes.find(offending).unwrap();
                    let span = diagnostic.span();
                    assert_eq!(span.file(), FileId::new(5));
                    assert_eq!(span.range().start().as_usize(), start, "{text}");
                    assert_eq!(
                        span.range().end().as_usize(),
                        start + offending.len(),
                        "{text}"
                    );
                }
            }
        }
    }
}

#[test]
fn pack_entries_preserve_empty_neighbors_and_list_order() {
    for family in ["view", "rearrange", "inflate", "einop", "contract"] {
        let output = if family == "contract" {
            "...first"
        } else {
            "...second, , ...first"
        };
        let parsed = parse_schema(family, ", ...first, , ...second, ", output);
        assert!(parsed.is_success(), "{family}: {:?}", parsed.diagnostics());
        let lists = axis_lists(&parsed);
        let mut expected = vec!["", "...first", "", "...second", ""];
        if family == "contract" {
            expected.push("...first");
        } else {
            expected.extend(["...second", "", "...first"]);
        }
        assert_eq!(lists.len(), expected.len());
        for (list, expected) in lists.iter().zip(expected) {
            assert_eq!(list.text().trim(), expected);
            let entries: Vec<_> = list.children().collect();
            assert_eq!(entries.len(), usize::from(!expected.is_empty()));
            if let Some(entry) = entries.first() {
                assert_eq!(entry.kind(), SyntaxKind::AxisAtom);
                assert_eq!(entry.text().trim(), expected);
                assert_eq!(entry.children().count(), 0);
            }
        }
    }
}
