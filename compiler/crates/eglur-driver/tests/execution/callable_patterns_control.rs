use super::*;

#[test]
fn parameter_and_nested_constructor_bindings_keep_runtime_values() {
    for (input, expected) in [("Some((1, 2))", "7"), ("None", "4")] {
        let source = format!(
            "import std::test::emit!\n\
             fn choose(seed: i64, value: Opt[(i64, i64)]) -> i64 = match value {{\n\
               whole @ Some((left, right)) => wrapping_add(seed, wrapping_add(left, right)),\n\
               None => seed\n\
             }}\n\
             fn main!() -> Res[Str, Str] {{ let text = to_str(choose(4, {input}))\n\
               emit!(text)\nyield Ok(text) }}\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        let result = completed(&program, &catalog, &profile);
        assert_eq!(ok_str(result.value()), expected);
        assert_eq!(
            result.events(),
            [eglur_driver::RuntimeEvent::Emitted(expected.into())]
        );
    }
}
