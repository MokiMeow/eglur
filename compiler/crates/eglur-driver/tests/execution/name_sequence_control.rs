use super::*;

#[test]
fn zero_argument_calls_and_tuple_children_preserve_source_order() {
    let (program, catalog, profile) = compile_single(concat!(
        "import std::test::emit!\n",
        "fn left!() -> i64 { emit!(\"left\")\nyield 3 }\n",
        "fn right!() -> i64 { emit!(\"right\")\nyield 4 }\n",
        "fn main!() -> Res[Str, Str] { let pair = (left!(), right!())\n",
        "yield Ok(to_str(wrapping_add(pair.0, pair.1))) }\n",
    ));
    let result = completed(&program, &catalog, &profile);
    assert_eq!(ok_str(result.value()), "7");
    assert_eq!(
        result.events(),
        [
            eglur_driver::RuntimeEvent::Emitted("left".into()),
            eglur_driver::RuntimeEvent::Emitted("right".into())
        ]
    );
}

#[test]
fn nested_block_bindings_keep_outer_identity_and_activation() {
    let (program, catalog, profile) = compile_single(concat!(
        "import std::test::emit!\n",
        "fn main!() -> Res[Str, Str] { let x = 3\n",
        "let pair = (x, 4)\n{ let x = pair.1\nemit!(to_str(x)) }\n",
        "emit!(to_str(x))\nyield Ok(to_str(wrapping_add(pair.0, pair.1))) }\n",
    ));
    let result = completed(&program, &catalog, &profile);
    assert_eq!(ok_str(result.value()), "7");
    assert_eq!(
        result.events(),
        [
            eglur_driver::RuntimeEvent::Emitted("4".into()),
            eglur_driver::RuntimeEvent::Emitted("3".into())
        ]
    );
}
