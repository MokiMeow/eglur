use super::{effect_facts, effect_profile, single, source_callable};
use eglur_diagnostics::{DiagnosticDataValue, DiagnosticSnapshot};
use eglur_driver::TrustedCatalog;
use eglur_sema::{EffectSummary, FactState};

#[test]
fn failed_trait_calls_preserve_body_types_and_complete_effect_provenance() {
    for count in [8, 32, 128] {
        for explicit in [false, true] {
            let call = if explicit {
                "bounded::[Plain](value)"
            } else {
                "bounded(value)"
            };
            let calls = (0..count).map(|_| format!("{call}\n")).collect::<String>();
            let source = format!(
                "import std::test::emit!\ntrait Mark {{}}\nstruct Plain {{}}\n\
                 fn bounded[T: Mark](value: T) -> Unit = ()\n\
                 fn drive!(value: Plain) -> Unit {{ emit!(\"before\")\n\
                 {calls}emit!(\"after\")\nyield () }}\n"
            );
            let mut session = single(&source);
            let catalog = TrustedCatalog::first_slice().unwrap();
            let analysis = session.resolve(&effect_profile(), Some(&catalog)).unwrap();
            let resolution = analysis.resolution().unwrap();
            let drive = source_callable(resolution, "drive!");
            assert!(matches!(
                resolution.body_types().body(drive),
                FactState::Known(_)
            ));
            assert!(matches!(
                effect_facts(resolution).source_summary(drive),
                FactState::Poisoned(_)
            ));
            let bounded = source_callable(resolution, "bounded");
            assert!(matches!(
                effect_facts(resolution).source_summary(bounded),
                FactState::Known(EffectSummary::Known(summary)) if summary.is_pure()
            ));

            let DiagnosticSnapshot::Rejected(snapshot) = session.into_diagnostics(None) else {
                panic!("failed trait bounds reject compilation");
            };
            assert_eq!(snapshot.diagnostics().len(), count);
            assert_eq!(snapshot.omitted_error_count(), 0);
            for diagnostic in snapshot.diagnostics() {
                assert_eq!(diagnostic.code().as_str(), "TRAIT_BOUND_NOT_SATISFIED");
                assert_eq!(
                    diagnostic.data().get("trait"),
                    Some(&DiagnosticDataValue::from("Mark"))
                );
                assert_eq!(
                    diagnostic.data().get("type"),
                    Some(&DiagnosticDataValue::from("Plain"))
                );
            }
        }
    }
}
