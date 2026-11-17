use super::*;

#[test]
fn guarded_match_evaluates_the_scrutinee_once_and_only_the_selected_arm() {
    for (guard, expected, event) in [("true", "7", "one"), ("false", "9", "fallback")] {
        let source = format!(
            "import std::test::emit!\nfn mark!(label: Str, value: i64) -> i64 {{ emit!(label)\nyield value }}\nfn main!() -> Res[Str, Str] = Ok(to_str(match mark!(\"scrutinee\", 1) {{ 0 => mark!(\"zero\", 0), 1 if {guard} => mark!(\"one\", 7), _ => mark!(\"fallback\", 9) }}))\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        let result = completed(&program, &catalog, &profile);
        assert_eq!(ok_str(result.value()), expected);
        assert_eq!(
            result.events(),
            [
                eglur_driver::RuntimeEvent::Emitted("scrutinee".into()),
                eglur_driver::RuntimeEvent::Emitted(event.into())
            ]
        );
    }
}
