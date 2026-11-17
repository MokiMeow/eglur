use super::*;

const SINGLE: &str = concat!(
    "trait Loop[A] {}\n",
    "impl[A, T: Loop[Opt[A]]] Loop[A] for T {}\n",
);

const MUTUAL: &str = concat!(
    "trait Loop[A] {}\ntrait Next[A] {}\n",
    "impl[A, T: Next[Opt[A]]] Loop[A] for T {}\n",
    "impl[A, T: Loop[A]] Next[A] for T {}\n",
);

fn profiles() -> [SupportProfile; 2] {
    [trait_profile(), SupportProfile::semantic_core_v1()]
}

fn resolve(source: &str, profile: &SupportProfile) -> DiagnosticSnapshot {
    let mut session = session(source);
    let catalog = TrustedCatalog::first_slice().unwrap();
    session.resolve(profile, Some(&catalog)).unwrap();
    session.into_diagnostics(None)
}

#[test]
fn growing_cycles_keep_original_obligation_data_across_semantic_profiles() {
    let templates = [
        SINGLE,
        MUTUAL,
        concat!(
            "type Maybe[A] = Opt[A]\ntrait Loop[A] {}\n",
            "impl[A, T: Loop[Maybe[Maybe[A]]]] Loop[A] for T {}\n",
        ),
    ];
    for template in templates {
        let source = format!(
            "{template}fn require[X: Loop[i64]](value: X) -> X = value\nfn invalid(value: i64) -> i64 = require(value)\n"
        );
        for profile in profiles() {
            let snapshot = resolve(&source, &profile);
            let diagnostics = snapshot.diagnostics();
            assert_eq!(diagnostics.len(), 1, "{source}");
            assert_eq!(diagnostics[0].code().as_str(), "TRAIT_BOUND_NOT_SATISFIED");
            assert_eq!(
                diagnostics[0].data().get("trait"),
                Some(&DiagnosticDataValue::from("Loop[i64]"))
            );
            assert_eq!(
                diagnostics[0].data().get("type"),
                Some(&DiagnosticDataValue::from("i64"))
            );
        }
    }
}

#[test]
fn exact_and_future_assumptions_still_produce_checked_evidence() {
    for (template, bound) in [
        (SINGLE, "Loop[i64]"),
        (SINGLE, "Loop[Opt[Opt[i64]]]"),
        (MUTUAL, "Next[Opt[i64]]"),
        (MUTUAL, "Loop[Opt[Opt[i64]]]"),
    ] {
        let source = format!(
            "{template}fn require[X: Loop[i64]](value: X) -> X = value\nfn valid[X: {bound}](value: X) -> X = require(value)\n"
        );
        for profile in profiles() {
            assert!(
                matches!(resolve(&source, &profile), DiagnosticSnapshot::Accepted(_)),
                "{source}"
            );
        }
    }
}

#[test]
fn assumptions_for_other_subjects_or_owners_do_not_discharge_the_cycle() {
    let source = format!(
        "{SINGLE}fn require[X: Loop[i64]](value: X) -> X = value\nfn valid[X: Loop[Opt[i64]]](value: X) -> X = require(value)\nfn invalid[X: Loop[Opt[i64]]](value: i64, unused: X) -> i64 = require(value)\n"
    );
    for profile in profiles() {
        let snapshot = resolve(&source, &profile);
        let diagnostics = snapshot.diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code().as_str(), "TRAIT_BOUND_NOT_SATISFIED");
        assert_eq!(
            diagnostics[0].data().get("type"),
            Some(&DiagnosticDataValue::from("i64"))
        );
    }
}

#[test]
fn finite_prefix_into_a_growing_cycle_reports_the_cycle_entry() {
    let source = format!(
        "{MUTUAL}trait Start[A] {{}}\nimpl[A, T: Loop[A]] Start[A] for T {{}}\nfn require[X: Start[i64]](value: X) -> X = value\nfn invalid(value: i64) -> i64 = require(value)\n"
    );
    for profile in profiles() {
        let snapshot = resolve(&source, &profile);
        let diagnostics = snapshot.diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].data().get("trait"),
            Some(&DiagnosticDataValue::from("Loop[i64]"))
        );
    }
}

#[test]
fn recursive_impl_declarations_and_finite_shrinking_proofs_remain_legal() {
    let controls = [
        SINGLE.to_owned(),
        MUTUAL.to_owned(),
        concat!(
            "trait Loop[A] {}\nimpl[A] Loop[A] for i64 {}\n",
            "impl[A, T: Loop[A]] Loop[A] for Opt[T] {}\n",
            "fn require[X: Loop[i64]](value: X) -> X = value\n",
            "fn valid(value: Opt[Opt[i64]]) -> Opt[Opt[i64]] = require(value)\n",
        )
        .to_owned(),
        concat!(
            "trait Loop[A] {}\nimpl[A, T: Loop[Opt[A]]] Loop[Opt[Opt[A]]] for T {}\n",
            "fn require[X: Loop[Opt[Opt[Opt[i64]]]]](value: X) -> X = value\n",
            "fn valid[X: Loop[Opt[i64]]](value: X) -> X = require(value)\n",
        )
        .to_owned(),
    ];
    for source in controls {
        for profile in profiles() {
            let snapshot = resolve(&source, &profile);
            assert!(
                matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
                "{source}: {:?}",
                snapshot.diagnostics()
            );
        }
    }
}

#[test]
fn type_applications_and_associated_results_use_the_same_cycle_certificate() {
    let applications = [
        "struct Required[T: Loop[i64]] {}\nfn invalid(value: Required[i64]) -> Unit = ()\n",
        "trait Gives { type Item }\nstruct Holder[T] {}\nimpl[T: Loop[i64]] Gives for Holder[T] { type Item = i64 }\ntype Read[T: Gives] = T::Item\nfn invalid() -> Read[Holder[i64]] = 1\n",
    ];
    for application in applications {
        let source = format!("{SINGLE}{application}");
        for profile in [
            trait_profile_with_evidence(false),
            trait_profile(),
            SupportProfile::semantic_core_v1(),
        ] {
            let snapshot = resolve(&source, &profile);
            let diagnostics = snapshot.diagnostics();
            assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
            assert_eq!(diagnostics[0].code().as_str(), "TRAIT_BOUND_NOT_SATISFIED");
            assert_eq!(
                diagnostics[0].data().get("trait"),
                Some(&DiagnosticDataValue::from("Loop[i64]"))
            );
        }
    }
}
