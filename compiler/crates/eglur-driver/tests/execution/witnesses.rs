use super::*;

#[test]
fn implementation_and_trait_prerequisites_keep_distinct_dispatch() {
    let (program, catalog, profile) = compile_single(concat!(
        "trait Left { fn left(self: &Self) -> i64 }\n",
        "trait Right { fn right(self: &Self) -> i64 }\n",
        "impl Left for Bool { fn left(self: &Self) -> i64 = 11 }\n",
        "impl Right for i64 { fn right(self: &Self) -> i64 = 22 }\n",
        "trait Root[A: Right] { fn combine(self: &Self, value: A) -> i64 }\n",
        "impl[T: Left, A: Right] Root[A] for T {\n",
        "  fn combine(self: &Self, value: A) -> i64 =\n",
        "    wrapping_add(wrapping_mul(self.left(), 100), value.right())\n",
        "}\n",
        "fn call[T: Root[i64]](value: T) -> i64 = value.combine(0)\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str(call(true)))\n",
    ));
    assert_eq!(
        ok_str(completed(&program, &catalog, &profile).value()),
        "1122"
    );
}

#[test]
fn imported_default_method_keeps_root_trait_and_method_witnesses() {
    let (program, catalog, profile) = compile_files(
        [
            (
                "dep.eglur",
                concat!(
                    "pub trait Left { fn left(self: &Self) -> i64 }\n",
                    "pub trait Right { fn right(self: &Self) -> i64 }\n",
                    "pub struct First {}\npub struct Second {}\n",
                    "impl Left for First { fn left(self: &Self) -> i64 = 11 }\n",
                    "impl Right for Second { fn right(self: &Self) -> i64 = 22 }\n",
                    "pub trait Root[A: Right] {\n",
                    "  fn base(self: &Self) -> i64\n",
                    "  fn combine[B: Left](self: &Self, value: A, other: B) -> i64 =\n",
                    "    wrapping_add(wrapping_add(wrapping_mul(self.base(), 1000), wrapping_mul(value.right(), 10)), other.left())\n",
                    "}\n",
                    "impl[A: Right] Root[A] for First { fn base(self: &Self) -> i64 = 7 }\n",
                ),
            ),
            (
                "main.eglur",
                concat!(
                    "import dep::First\nimport dep::Second\nimport dep::Root\n",
                    "fn forward[T: Root[Second]](value: T) -> i64 = value.combine(Second {}, First {})\n",
                    "fn main!() -> Res[Str, Str] = Ok(to_str(forward(First {})))\n",
                ),
            ),
        ],
        "main.eglur",
    );
    assert_eq!(
        ok_str(completed(&program, &catalog, &profile).value()),
        "7231"
    );
}
