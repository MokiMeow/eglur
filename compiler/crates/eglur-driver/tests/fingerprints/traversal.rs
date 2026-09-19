use super::*;

fn nested_yields(value: &str, depth: usize) -> String {
    format!("{}{value}{}", "{ yield ".repeat(depth), " }".repeat(depth))
}

#[test]
fn deep_blocks_seal_through_the_full_semantic_pipeline() {
    let source = format!("fn f(x: i64) -> i64 = {}\n", nested_yields("x", 255));
    let request = FileSetRequest::single("main.eglur", source.into_bytes()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let profile = SupportProfile::semantic_core_v1();
    let catalog = TrustedCatalog::first_slice().unwrap();
    session.resolve(&profile, Some(&catalog)).unwrap();
    let compiled = session.seal(None).unwrap();
    assert!(compiled.checked_program().is_some());
    assert!(matches!(
        compiled.diagnostics(),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn deep_statement_fingerprints_preserve_leaf_sensitivity() {
    for form in ["yield", "let", "assign", "return"] {
        let source = |leaf: &str| {
            let mut value = leaf.to_owned();
            for _ in 0..255 {
                value = match form {
                    "yield" => format!("{{ yield {value} }}"),
                    "let" => format!("{{ let value: i64 = {value}\nyield value }}"),
                    "assign" => {
                        format!("{{ let mut value: i64 = 0\nvalue = {value}\nyield value }}")
                    }
                    "return" => format!("{{ return {value} }}"),
                    _ => unreachable!(),
                };
            }
            format!("fn f() -> i64 = {value}\n")
        };
        let before = resolve([file("main.eglur", &source("1"))], "main.eglur", false);
        let after = resolve([file("main.eglur", &source("2"))], "main.eglur", true);
        assert_ne!(
            outputs(&before, "main").body(),
            outputs(&after, "main").body(),
            "{form}"
        );
        assert_eq!(
            outputs(&before, "main").interface(),
            outputs(&after, "main").interface(),
            "{form}"
        );
    }
}

#[test]
fn deep_blocks_with_calls_and_places_ignore_observation_and_trivia() {
    let value = nested_yields("choose(right = c.value, left = pair.0)", 240);
    let source = format!(
        concat!(
            "struct Count {{ field value: i64 }}\n",
            "fn choose(left: i64, right: i64) -> i64 = left\n",
            "fn f(c: Count, pair: (i64, i64)) -> i64 = {value}\n",
        ),
        value = value,
    );
    let plain = resolve([file("main.eglur", &source)], "main.eglur", false);
    let observed = resolve([file("main.eglur", &source)], "main.eglur", true);
    let comment = resolve(
        [file("main.eglur", &format!("// extra trivia\n{source}"))],
        "main.eglur",
        false,
    );
    assert_eq!(plain, observed);
    assert_eq!(outputs(&plain, "main"), outputs(&comment, "main"));
    assert_ne!(unit_key(&plain, "main"), unit_key(&comment, "main"));
}

#[test]
fn poison_in_a_deep_block_does_not_produce_fingerprints() {
    let source = format!("fn f() -> i64 = {}\n", nested_yields("true", 255));
    let request = FileSetRequest::single("main.eglur", source.into_bytes()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session.resolve(&profile(), Some(&catalog)).unwrap();
    assert!(analysis.fingerprints().is_none());
    assert!(analysis.resolution().unwrap().fingerprints().is_none());
    let diagnostics = session.into_diagnostics(None);
    let codes = diagnostics
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect::<Vec<_>>();
    assert_eq!(codes, ["RETURN_TYPE_MISMATCH"]);
}
