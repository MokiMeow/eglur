use super::*;

#[test]
fn nested_item_bodies_keep_trait_and_inherent_owner_context() {
    let (program, catalog, profile) = compile_single(concat!(
        "import std::test::emit!\n",
        "struct Counter {\n",
        "  field value: i64\n",
        "  prop count: i64 { fn get_count(self: &Self) -> i64 = self.value }\n",
        "  fn offset(self: &Self) -> i64 = 3\n",
        "}\n",
        "impl Display for Counter {\n",
        "  fn to_str(self: &Self) -> Str = to_str(wrapping_add(self.value, self.offset()))\n",
        "}\n",
        "fn main!() -> Res[Str, Str] { let c = Counter { value: 4 }\n",
        "let text = c.to_str()\nemit!(text)\nyield Ok(text) }\n",
    ));
    let result = completed(&program, &catalog, &profile);
    assert_eq!(ok_str(result.value()), "7");
    assert_eq!(
        result.events(),
        [eglur_driver::RuntimeEvent::Emitted("7".into())]
    );
}
