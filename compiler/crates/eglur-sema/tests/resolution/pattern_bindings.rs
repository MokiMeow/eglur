use super::{CapabilitySet, analyze, codes, empty_external};
use eglur_sema::{CompileCapability, ResolveCapability};

#[test]
fn aliases_coalesce_binding_identity_without_discarding_origins() {
    let external = empty_external();
    for (pattern, occurrences) in [
        ("x @ x", 2),
        ("x @ (x)", 2),
        ("x @ (x, _)", 2),
        ("(x @ x, _)", 2),
        ("x @ x @ x", 3),
        ("x @ (x | x)", 3),
        ("(x @ x) | (x @ x)", 4),
        ("x @ Point{x}", 2),
        ("Point{x: x @ x}", 2),
        ("x @ Maybe::Some(x)", 2),
        ("Maybe::Some(x @ x)", 2),
    ] {
        let source = format!(
            "struct Point {{ field x: i64 }}\nenum Maybe {{ Some(i64) }}\nfn f(v: i64) -> i64 = match v {{ {pattern} => x }}\n",
        );
        for patterns in [true, false] {
            let mut support = CapabilitySet::resolver(true);
            if !patterns {
                support.0.remove(&CompileCapability::Resolve(
                    ResolveCapability::ResolvePatternScopes,
                ));
            }
            let analysis = analyze(
                vec![("main.eglur", &source)],
                &[&[0]],
                &support,
                Some(&external),
            );
            assert!(codes(&analysis.diagnostics).is_empty(), "{pattern}");
            let bindings = analysis
                .names
                .locals()
                .iter()
                .filter(|local| local.name().spelling() == "x")
                .collect::<Vec<_>>();
            let [binding] = bindings.as_slice() else {
                panic!("one local identity for {pattern}");
            };
            assert_eq!(binding.origins().len(), occurrences, "{pattern}");
            assert!(
                binding
                    .origins()
                    .windows(2)
                    .all(|pair| pair[0].get() < pair[1].get()),
                "{pattern}"
            );
            for &origin in binding.origins() {
                assert_eq!(
                    analysis.names.local_binding(
                        binding.id().module(),
                        binding.id().body(),
                        origin
                    ),
                    Some(binding.id()),
                    "{pattern}"
                );
            }
        }
    }
}

#[test]
fn nested_aliases_preserve_one_identity_at_the_parser_budget() {
    let source = format!(
        "fn f(v: i64) -> i64 = match v {{ {}x => x }}\n",
        "x @ ".repeat(254),
    );
    let external = empty_external();
    let analysis = analyze(
        vec![("main.eglur", &source)],
        &[&[0]],
        &CapabilitySet::resolver(true),
        Some(&external),
    );
    assert!(codes(&analysis.diagnostics).is_empty());
    assert_eq!(analysis.names.locals().len(), 2);
    assert_eq!(analysis.names.locals()[1].origins().len(), 255);
}
