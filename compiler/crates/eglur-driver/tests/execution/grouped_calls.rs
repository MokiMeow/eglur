use super::*;

#[test]
fn grouped_named_callees_preserve_generic_and_overload_selection() {
    for expression in [
        "id(7)",
        "(id)(7)",
        "((id))(7)",
        "(id)::[i64](7)",
        "(pick)(7)",
        "(wrapping_add)(3, 4)",
        "7.then((id))",
        "(3, 4).apply((wrapping_add))",
    ] {
        let source = format!(
            "fn id[T](value: T) -> T = value\n\
             overload fn pick(value: i64) -> i64 = value\n\
             overload fn pick(value: Str) -> Str = value\n\
             fn main!() -> Res[Str, Str] = Ok(to_str({expression}))\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        assert_eq!(
            ok_str(completed(&program, &catalog, &profile).value()),
            "7",
            "{expression}"
        );
    }
}

#[test]
fn grouped_method_callees_preserve_receiver_and_trait_dispatch() {
    for expression in [
        "counter.read()",
        "(counter.read)()",
        "((counter.relay))::[i64](7)",
        "(counter.pick)(7)",
        "(counter.to_str)()",
        "7.then((counter.relay))",
        "((7,)).apply((counter.relay))",
    ] {
        let source = format!(
            "struct Counter {{\n\
               field value: i64\n\
               fn read(self: &Self) -> i64 = self.value\n\
               fn relay[T](self: &Self, value: T) -> T = value\n\
               overload fn pick(self: &Self, value: i64) -> i64 = value\n\
               overload fn pick(self: &Self, value: Str) -> Str = value\n\
             }}\n\
             impl Display for Counter {{ fn to_str(self: &Self) -> Str = to_str(self.value) }}\n\
             fn main!() -> Res[Str, Str] {{\n\
               let counter = Counter {{ value: 7 }}\n\
               yield Ok(to_str({expression}))\n\
             }}\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        assert_eq!(
            ok_str(completed(&program, &catalog, &profile).value()),
            "7",
            "{expression}"
        );
    }
}

#[test]
fn grouped_residual_constructors_keep_contextual_types() {
    for expression in [
        "Ok(\"7\")",
        "(Ok)(\"7\")",
        "(\"7\").then((Ok))",
        "match ((Some))(7) { Some(value) => (Ok)(to_str(value)), None => (Err)(\"absent\") }",
    ] {
        let source = format!("fn main!() -> Res[Str, Str] = {expression}\n");
        let (program, catalog, profile) = compile_single(&source);
        let result = completed(&program, &catalog, &profile);
        assert_eq!(result.process_status(), 0, "{expression}");
        assert_eq!(ok_str(result.value()), "7", "{expression}");
    }
}

#[test]
fn grouped_calls_evaluate_receiver_and_named_arguments_once_in_source_order() {
    let (program, catalog, profile) = compile_single(concat!(
        "import std::test::emit!\n",
        "struct Counter {\n",
        "  field value: i64\n",
        "  fn join!(self: Self, first: i64, second: i64) -> i64 {\n",
        "    emit!(\"call\")\nyield wrapping_add(first, second)\n",
        "  }\n",
        "}\n",
        "fn make!() -> Counter { emit!(\"receiver\")\nyield Counter { value: 0 } }\n",
        "fn mark!(value: i64) -> i64 { emit!(to_str(value))\nyield value }\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str(((make!()).join!)(second=(mark!)(4), first=(mark!)(3))))\n",
    ));
    let result = completed(&program, &catalog, &profile);
    assert_eq!(ok_str(result.value()), "7");
    assert_eq!(
        result.events(),
        ["receiver", "4", "3", "call"].map(|mark| eglur_driver::RuntimeEvent::Emitted(mark.into()))
    );
}

#[test]
fn grouped_method_with_an_explicit_reference_updates_the_original_place() {
    let (program, catalog, profile) = compile_single(concat!(
        "struct Counter {\n",
        "  field mut value: i64\n",
        "  fn set!(self: &mut Self, value: i64) -> Unit { self.value = value\nyield () }\n",
        "}\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let mut counter = Counter { value: 0 }\n",
        "  let reference = &mut counter\n",
        "  ((reference.set!))(7)\n",
        "  yield Ok(to_str(counter.value))\n",
        "}\n",
    ));
    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn grouped_import_aliases_and_reexports_keep_source_identity() {
    let (program, catalog, profile) = compile_files(
        [
            (
                "main.eglur",
                concat!(
                    "import bridge::id as alias\n",
                    "fn main!() -> Res[Str, Str] = (Ok)((to_str)((alias)(7)))\n",
                ),
            ),
            ("bridge.eglur", "pub import helpers::id\n"),
            ("helpers.eglur", "pub fn id[T](value: T) -> T = value\n"),
        ],
        "main.eglur",
    );
    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}
