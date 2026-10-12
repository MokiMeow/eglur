use super::*;

#[test]
fn imported_scalar_type_alias_preserves_execution() {
    let (program, catalog, profile) = compile_single(concat!(
        "import std::prelude::i64 as Count\n",
        "fn main!() -> Res[Str, Str] { let value: Count = 7\nyield Ok(to_str(value)) }\n",
    ));
    let result = completed(&program, &catalog, &profile);
    assert_eq!(ok_str(result.value()), "7");
    assert!(result.events().is_empty());
}

#[test]
fn reexported_type_and_value_aliases_preserve_call_order() {
    let (program, catalog, profile) = compile_files(
        [
            (
                "provider.eglur",
                "pub type Count = i64\npub fn compute(x: Count) -> Count = x\n",
            ),
            (
                "forward.eglur",
                "pub import provider::Count\npub import provider::compute as calculate\n",
            ),
            (
                "main.eglur",
                "import forward::{Count, calculate}\nimport std::test::emit!\nfn main!() -> Res[Str, Str] { emit!(\"before\")\nlet value: Count = calculate(7)\nemit!(\"after\")\nyield Ok(to_str(value)) }\n",
            ),
        ],
        "main.eglur",
    );
    let result = completed(&program, &catalog, &profile);
    assert_eq!(ok_str(result.value()), "7");
    assert_eq!(
        result.events(),
        [
            eglur_driver::RuntimeEvent::Emitted("before".into()),
            eglur_driver::RuntimeEvent::Emitted("after".into())
        ],
    );
}
