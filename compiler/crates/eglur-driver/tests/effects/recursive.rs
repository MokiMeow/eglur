use super::{effect_facts, effect_profile, known_summary, single, source_callable};
use eglur_driver::TrustedCatalog;
use eglur_sema::{EffectCategory, EffectSummary, FactState, StateScope};

#[test]
fn repeated_recursive_calls_keep_site_localization_in_both_declaration_orders() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for count in [1, 2, 8, 32] {
        for hub_first in [false, true] {
            let calls = (0..count)
                .map(|index| format!("leaf{index}!(&mut cell)\n").repeat(3))
                .collect::<String>();
            let hub = format!(
                "fn hub!(go: Bool) -> Unit {{ let mut cell: i64 = 0\n\
                 if go {{ {calls}yield () }} else {{ yield () }}\nyield () }}\n"
            );
            let leaves = (0..count)
                .map(|index| {
                    format!(
                        "fn leaf{index}!(target: &mut i64) -> Unit {{\n\
                         *target = 1\nhub!(false)\nyield () }}\n"
                    )
                })
                .collect::<String>();
            let source = if hub_first {
                format!("{hub}{leaves}")
            } else {
                format!("{leaves}{hub}")
            };
            let mut session = single(&source);
            let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
            let resolution = analysis.resolution().unwrap();
            let facts = effect_facts(resolution);
            let hub = source_callable(resolution, "hub!");
            assert!(known_summary(facts, hub).is_pure());
            let mut members = vec![hub];
            for index in 0..count {
                let leaf = source_callable(resolution, &format!("leaf{index}!"));
                let summary = known_summary(facts, leaf);
                assert_eq!(summary.categories(), [EffectCategory::State]);
                assert_eq!(summary.state_scope(), StateScope::Localizable);
                members.push(leaf);
            }
            members.sort_unstable();
            assert!(facts.sccs().iter().any(|scc| scc.members() == members));
            assert!(session.into_diagnostics(None).diagnostics().is_empty());
        }
    }
}

#[test]
fn unresolved_bang_requirements_propagate_through_repeated_recursive_calls() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let alpha = concat!(
        "fn alpha![T: Runnable](value: &mut T) -> Unit {\n",
        "  beta!::[T](value)\n",
        "  beta!::[T](value)\n",
        "  yield ()\n",
        "}\n",
    );
    let beta = concat!(
        "fn beta![T: Runnable](value: &mut T) -> Unit {\n",
        "  alpha!::[T](value)\n",
        "  value.run!()\n",
        "  yield ()\n",
        "}\n",
    );
    for declarations in [format!("{alpha}{beta}"), format!("{beta}{alpha}")] {
        let source =
            format!("trait Runnable {{ fn run!(self: &mut Self) -> Unit }}\n{declarations}");
        let mut session = single(&source);
        let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
        let resolution = analysis.resolution().unwrap();
        for name in ["alpha!", "beta!"] {
            assert!(matches!(
                effect_facts(resolution).source_summary(source_callable(resolution, name)),
                FactState::Known(EffectSummary::Unknown)
            ));
        }
        assert!(session.into_diagnostics(None).diagnostics().is_empty());
    }
}

#[test]
fn self_recursive_state_mutation_remains_localizable() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = single(concat!(
        "fn spin!(go: Bool, value: &mut i64) -> Unit {\n",
        "  if go { spin!(false, value) } else { *value = 1 }\n",
        "  yield ()\n",
        "}\n",
    ));
    let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    let spin = source_callable(resolution, "spin!");
    let summary = known_summary(effect_facts(resolution), spin);
    assert_eq!(summary.categories(), [EffectCategory::State]);
    assert_eq!(summary.state_scope(), StateScope::Localizable);
    assert!(session.into_diagnostics(None).diagnostics().is_empty());
}

#[test]
fn recursive_components_keep_the_first_blocking_dependency() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for poison_first in [false, true] {
        let calls = if poison_first {
            "poison!()\n_ = unavailable!(0.0, 1.0)\n"
        } else {
            "_ = unavailable!(0.0, 1.0)\npoison!()\n"
        };
        let source = format!(
            "fn alpha!() -> Unit {{ beta!()\n{calls}yield () }}\n\
             fn beta!() -> Unit {{ alpha!()\nyield () }}\n\
             fn poison!() -> Unit = missing()\n\
             fn unavailable!(left: f64, right: f64) -> f64 = left + right\n"
        );
        let mut session = single(&source);
        let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
        let resolution = analysis.resolution().unwrap();
        let facts = effect_facts(resolution);
        for name in ["alpha!", "beta!"] {
            let summary = facts.source_summary(source_callable(resolution, name));
            if poison_first {
                assert!(matches!(summary, FactState::Poisoned(_)), "{summary:?}");
            } else {
                assert!(matches!(summary, FactState::Unperformed), "{summary:?}");
            }
        }
        let snapshot = session.into_diagnostics(None);
        assert_eq!(snapshot.diagnostics().len(), 1);
        assert_eq!(snapshot.diagnostics()[0].code().as_str(), "NAME_NOT_FOUND");
    }
}

#[test]
fn later_call_updates_retain_direct_state_and_preserve_transitive_io() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mutator = concat!(
        "pub fn mutator!(value: &mut i64) -> Unit {\n",
        "  *value = 1\n",
        "  driver!()\n",
        "  yield ()\n",
        "}\n",
    );
    let driver = concat!(
        "pub fn driver!() -> Unit {\n",
        "  let mut cell: i64 = 0\n",
        "  mutator!(&mut cell)\n",
        "  _ = print!(\"hello\")\n",
        "  yield ()\n",
        "}\n",
    );
    for declarations in [format!("{mutator}{driver}"), format!("{driver}{mutator}")] {
        let mut session = single(&format!("import std::io::print!\n{declarations}"));
        let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
        let resolution = analysis.resolution().unwrap();
        let facts = effect_facts(resolution);
        let summary = known_summary(facts, source_callable(resolution, "mutator!"));
        assert_eq!(
            summary.categories(),
            [EffectCategory::Io, EffectCategory::State]
        );
        assert_eq!(summary.state_scope(), StateScope::Localizable);
        let summary = known_summary(facts, source_callable(resolution, "driver!"));
        assert_eq!(summary.categories(), [EffectCategory::Io]);
        assert_eq!(summary.state_scope(), StateScope::None);
        assert!(session.into_diagnostics(None).diagnostics().is_empty());
    }
}
