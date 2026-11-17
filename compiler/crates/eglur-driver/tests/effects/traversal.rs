use super::{effect_facts, effect_profile, known_summary, single, source_callable};
use eglur_driver::TrustedCatalog;
use eglur_sema::{EffectCategory, StateScope};

const BUMP: &str = "fn bump!(value: &mut i64) -> Unit { *value = 2\nyield () }\n";

#[test]
fn parser_budgeted_groups_keep_known_effects_on_the_default_stack() {
    let source = format!(
        "fn f(x: i64) -> i64 = {}x{}\n",
        "(".repeat(255),
        ")".repeat(255),
    );
    let mut session = single(&source);
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    assert!(known_summary(effect_facts(resolution), source_callable(resolution, "f")).is_pure());
    assert!(session.into_diagnostics(None).diagnostics().is_empty());
}

#[test]
fn deep_grouped_mutable_paths_preserve_localization() {
    let source = format!(
        "{BUMP}fn f() -> i64 {{ let mut value = 1\nbump!(&mut {}value{})\nyield value }}\n",
        "(".repeat(250),
        ")".repeat(250),
    );
    let mut session = single(&source);
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    let facts = effect_facts(resolution);
    assert_eq!(
        known_summary(facts, source_callable(resolution, "bump!")).categories(),
        [EffectCategory::State]
    );
    assert!(known_summary(facts, source_callable(resolution, "f")).is_pure());
    assert!(session.into_diagnostics(None).diagnostics().is_empty());
}

#[test]
fn nested_branch_joins_keep_aliases_from_every_path() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (then_value, else_value, pure) in [
        ("target", "&mut local", false),
        ("&mut local", "target", false),
        ("&mut local", "&mut local", true),
    ] {
        let source = format!(
            "{BUMP}fn f!(flag: Bool, target: &mut i64) -> Unit {{\nlet mut local = 0\nlet mut alias = &mut local\nif flag {{ if flag {{ alias = {then_value}\nyield () }} else {{ yield () }}\nyield () }} else {{ alias = {else_value}\nyield () }}\nbump!(alias)\nyield () }}\n",
        );
        let mut session = single(&source);
        let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
        let resolution = analysis.resolution().unwrap();
        let summary = known_summary(effect_facts(resolution), source_callable(resolution, "f!"));
        assert_eq!(summary.is_pure(), pure);
        if !pure {
            assert_eq!(summary.categories(), [EffectCategory::State]);
            assert_eq!(summary.state_scope(), StateScope::Localizable);
        }
        assert!(session.into_diagnostics(None).diagnostics().is_empty());
    }
}

#[test]
fn match_aliases_precede_guard_and_arm_effects() {
    let source = format!(
        "{BUMP}fn f!(flag: Bool, target: &mut i64) -> Unit = match target {{ alias if flag => {{ bump!(alias)\nyield () }}, alias => {{ if flag {{ bump!(alias)\nyield () }} else {{ yield () }}\nyield () }} }}\n",
    );
    let mut session = single(&source);
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    let summary = known_summary(effect_facts(resolution), source_callable(resolution, "f!"));
    assert_eq!(summary.categories(), [EffectCategory::State]);
    assert_eq!(summary.state_scope(), StateScope::Localizable);
    assert!(session.into_diagnostics(None).diagnostics().is_empty());
}
