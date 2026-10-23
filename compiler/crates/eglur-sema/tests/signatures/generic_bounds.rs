use super::*;

fn assert_functions_known(text: &str, names: &[&str]) {
    let external = empty_external();
    let analysis = analyze(text, &CapabilitySet::signatures(), Some(&external), None);
    assert!(
        matches!(analysis.diagnostics, DiagnosticSnapshot::Accepted(_)),
        "{:?}",
        analysis.diagnostics
    );
    let signatures = analysis.signatures.unwrap();
    for name in names {
        let id = source_def(&analysis.declarations, name, DeclarationKind::Function);
        assert!(matches!(
            signatures.source_declaration(id),
            FactState::Known(DeclarationSignature::Callable(_))
        ));
    }
}

#[test]
fn known_declaring_bounds_survive_incremental_publication() {
    let head = "trait HasItem { type Item }\ntrait Uses[T] {}\ntrait More[T] {}\ntrait Marker {}\n";
    let bounds = ["Uses[T::Item]", "More[T::Item]", "Marker"];
    for a in 0..bounds.len() {
        for b in 0..bounds.len() {
            for c in 0..bounds.len() {
                let indices = [a, b, c];
                if indices.into_iter().collect::<BTreeSet<_>>().len() != bounds.len() {
                    continue;
                }
                let written = indices
                    .into_iter()
                    .map(|i| bounds[i])
                    .collect::<Vec<_>>()
                    .join("+");
                assert_functions_known(
                    &format!("{head}fn drive[T:HasItem+{written}](x:T::Item)->T::Item=x\n"),
                    &["drive"],
                );
            }
        }
    }
}

#[test]
fn primed_bounds_support_forward_mutual_and_alias_backed_projections() {
    assert_functions_known(
        r#"
trait HasItem { type Item }
trait Uses[T] {}
trait More[T] {}
type Hidden[T:HasItem]=T::Item
fn forward[U:Uses[T::Item]+More[T::Item],T:HasItem](x:U)->U=x
fn mutual[U:HasItem+Uses[T::Item]+More[U::Item],T:HasItem+Uses[U::Item]+More[T::Item]](x:U)->U=x
fn aliases[T:HasItem+Uses[Hidden[T]]+More[T::Item]](x:T)->T=x
"#,
        &["forward", "mutual", "aliases"],
    );
}

#[test]
fn primed_bound_membership_does_not_replace_written_duplicate_checks() {
    for bounds in [
        "HasItem+HasItem",
        "HasItem+Uses[T::Item]+HasItem",
        "Uses[T::Item]+HasItem+Uses[T::Item]",
    ] {
        let text = format!(
            "trait HasItem {{ type Item }}\ntrait Uses[T] {{}}\nfn drive[T:{bounds}]()->Unit=()\n"
        );
        let external = empty_external();
        let analysis = analyze(&text, &CapabilitySet::signatures(), Some(&external), None);
        assert_eq!(
            diagnostic_codes(&analysis.diagnostics),
            ["TRAIT_BOUND_INVALID"]
        );
        assert_eq!(
            analysis.diagnostics.diagnostics().first().unwrap().data()["reason"].to_string(),
            "duplicate"
        );
    }
}

#[test]
fn every_primed_declaring_bound_participates_in_ambiguity() {
    for bounds in [
        "HasItem+Other+Uses[T::Item]+Marker",
        "Other+HasItem+Marker+Uses[T::Item]",
        "HasItem+Marker+Other+Uses[T::Item]",
    ] {
        let text = format!(
            "trait HasItem {{ type Item }}\ntrait Other {{ type Item }}\ntrait Uses[T] {{}}\ntrait Marker {{}}\nfn drive[T:{bounds}]()->Unit=()\n"
        );
        let external = empty_external();
        let analysis = analyze(&text, &CapabilitySet::signatures(), Some(&external), None);
        assert_eq!(
            diagnostic_codes(&analysis.diagnostics),
            ["ASSOCIATED_TYPE_PROJECTION_INVALID"]
        );
        assert_eq!(
            analysis.diagnostics.diagnostics().first().unwrap().data()["reason"].to_string(),
            "ambiguous declaring bounds"
        );
    }
}

#[test]
fn a_late_declaring_bound_cannot_be_accepted_after_an_earlier_use() {
    let external = empty_external();
    let text = "trait HasItem { type Item }\n\
         trait Other[A] { type Item }\n\
         trait Uses[A] {}\n\
         fn drive[T:Uses[T::Item]+Other[T::Item]+HasItem]()->Unit=()\n";
    let analysis = analyze(text, &CapabilitySet::signatures(), Some(&external), None);
    let diagnostics = analysis.diagnostics.diagnostics();
    for diagnostic in diagnostics {
        assert_eq!(
            diagnostic.code().as_str(),
            "ASSOCIATED_TYPE_PROJECTION_INVALID"
        );
        assert_eq!(
            diagnostic.data()["reason"].to_string(),
            "ambiguous declaring bounds"
        );
    }
    let actual = diagnostics
        .iter()
        .map(|diagnostic| {
            diagnostic
                .primary_span()
                .as_source()
                .unwrap()
                .file_span()
                .range()
                .start()
                .as_usize()
        })
        .collect::<BTreeSet<_>>();
    let expected = text
        .match_indices("T::Item")
        .map(|(offset, _)| offset)
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
    let function = source_def(&analysis.declarations, "drive", DeclarationKind::Function);
    assert!(matches!(
        analysis.signatures.unwrap().source_declaration(function),
        FactState::Poisoned(_)
    ));
}
