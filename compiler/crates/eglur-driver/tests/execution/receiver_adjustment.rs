use super::*;

#[test]
fn implicit_mutable_receiver_updates_the_original_place() {
    for call in [
        "counter.set!(7)",
        "((counter.set!))(7)",
        "7.then(counter.set!)",
        "(7,).apply((counter.set!))",
    ] {
        let source = format!(
            "struct Counter {{\n\
               field mut value: i64\n\
               fn set!(self: &mut Self, value: i64) -> Unit {{\n\
                 self.value = value\n\
                 yield ()\n\
               }}\n\
             }}\n\
             fn main!() -> Res[Str, Str] {{\n\
               let mut counter = Counter {{ value: 0 }}\n\
               {call}\n\
               yield Ok(to_str(counter.value))\n\
             }}\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        assert_eq!(
            ok_str(completed(&program, &catalog, &profile).value()),
            "7",
            "{call}"
        );
    }
}

#[test]
fn mutable_receiver_borrows_projected_places_and_preserves_existing_references() {
    for (setup, receiver, observed) in [
        (
            "let mut holder = Holder { counter: Counter { value: 0 } }",
            "holder.counter",
            "holder.counter.value",
        ),
        (
            "let mut pair = (Counter { value: 0 }, 9)",
            "pair.0",
            "pair.0.value",
        ),
        (
            "let mut counter = Counter { value: 0 }\nlet reference = &mut counter",
            "reference",
            "counter.value",
        ),
        (
            "let mut counter = Counter { value: 0 }\nlet reference = &mut counter",
            "(*reference)",
            "counter.value",
        ),
    ] {
        let source = format!(
            "struct Counter {{\n\
               field mut value: i64\n\
               fn set!(self: &mut Self, value: i64) -> Unit {{ self.value = value\nyield () }}\n\
             }}\n\
             struct Holder {{ field mut counter: Counter }}\n\
             fn main!() -> Res[Str, Str] {{\n\
               {setup}\n\
               ({receiver}.set!)(7)\n\
               yield Ok(to_str({observed}))\n\
             }}\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        assert_eq!(
            ok_str(completed(&program, &catalog, &profile).value()),
            "7",
            "{receiver}"
        );
    }
}

#[test]
fn shared_receivers_dereference_places_temporaries_and_existing_references() {
    for receiver in ["counter", "Counter { value: 7 }", "shared", "writable"] {
        let source = format!(
            "struct Counter {{\n\
               field value: i64\n\
               fn read(self: &Self) -> i64 = (*self).value\n\
             }}\n\
             fn main!() -> Res[Str, Str] {{\n\
               let mut counter = Counter {{ value: 7 }}\n\
               let shared = &counter\n\
               let writable = &mut counter\n\
               yield Ok(to_str(({receiver}).read()))\n\
             }}\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        assert_eq!(
            ok_str(completed(&program, &catalog, &profile).value()),
            "7",
            "{receiver}"
        );
    }
}

#[test]
fn receiver_borrow_observes_later_argument_writes_but_value_receiver_keeps_snapshot() {
    for (receiver_type, expected) in [("&Self", "9"), ("Self", "1")] {
        let source = format!(
            "struct Counter {{\n\
               field value: i64\n\
               fn read(self: {receiver_type}, ignored: Unit) -> i64 = self.value\n\
             }}\n\
             fn replace!(target: &mut Counter) -> Unit {{ *target = Counter {{ value: 9 }}\nyield () }}\n\
             fn main!() -> Res[Str, Str] {{\n\
               let mut counter = Counter {{ value: 1 }}\n\
               yield Ok(to_str(counter.read(replace!(&mut counter))))\n\
             }}\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        assert_eq!(
            ok_str(completed(&program, &catalog, &profile).value()),
            expected,
            "{receiver_type}"
        );
    }
}

#[test]
fn mutable_receiver_and_shared_argument_can_alias_the_same_place() {
    let (program, catalog, profile) = compile_single(concat!(
        "struct Counter {\n",
        "  field mut value: i64\n",
        "  fn update!(self: &mut Self, other: &Self) -> i64 {\n",
        "    self.value = 7\nyield (*other).value\n",
        "  }\n",
        "}\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let mut counter = Counter { value: 0 }\n",
        "  yield Ok(to_str(counter.update!(&counter)))\n",
        "}\n",
    ));
    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn temporary_receiver_and_named_arguments_evaluate_once_in_written_order() {
    let (program, catalog, profile) = compile_single(concat!(
        "import std::test::emit!\n",
        "struct Counter {\n",
        "  field value: i64\n",
        "  fn read(self: &Self, first: Unit, second: Unit) -> i64 = (*self).value\n",
        "}\n",
        "fn make!() -> Counter { emit!(\"receiver\")\nyield Counter { value: 7 } }\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str(((make!()).read)(second=emit!(\"second\"), first=emit!(\"first\"))))\n",
    ));
    let result = completed(&program, &catalog, &profile);
    assert_eq!(ok_str(result.value()), "7");
    assert_eq!(
        result.events(),
        ["receiver", "second", "first"]
            .map(|value| eglur_driver::RuntimeEvent::Emitted(value.into()))
    );
}

#[test]
fn receiver_place_acquisition_precedes_argument_failure_without_entering_the_method() {
    let (program, catalog, profile) = compile_single(concat!(
        "import std::test::emit!\n",
        "struct Counter {\n",
        "  field mut value: i64\n",
        "  fn set!(self: &mut Self, value: i64) -> Unit {\n",
        "    emit!(\"method\")\nself.value = value\nyield ()\n",
        "  }\n",
        "}\n",
        "fn fail!() -> Res[i64, Str] { emit!(\"argument\")\nyield Err(\"stop\") }\n",
        "fn main!() -> Res[Str, Str] = result {\n",
        "  let mut counter = Counter { value: 0 }\n",
        "  let reference = &mut counter\n",
        "  (*({ emit!(\"receiver\")\nyield reference })).set!(fail!()?)\n",
        "  yield Ok(\"unreachable\")\n",
        "}\n",
    ));
    let result = completed(&program, &catalog, &profile);
    assert_eq!(result.process_status(), 1);
    assert_eq!(ok_str(result.value()), "stop");
    assert_eq!(
        result.events(),
        ["receiver", "argument"].map(|value| eglur_driver::RuntimeEvent::Emitted(value.into()))
    );
}

#[test]
fn generic_trait_receivers_preserve_writes_and_reference_shape() {
    for receiver in ["input", "(*input)"] {
        let source = format!(
            "closed trait Update {{ fn set!(self: &mut Self, value: i64) -> Unit }}\n\
             struct Counter {{ field mut value: i64 }}\n\
             impl Update for Counter {{\n\
               fn set!(self: &mut Self, value: i64) -> Unit {{ (*self).value = value\nyield () }}\n\
             }}\n\
             fn update![T: Update](input: &mut T) -> Unit {{ {receiver}.set!(7)\nyield () }}\n\
             fn main!() -> Res[Str, Str] {{\n\
               let mut counter = Counter {{ value: 0 }}\n\
               update!(&mut counter)\n\
               yield Ok(to_str(counter.value))\n\
             }}\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        assert_eq!(
            ok_str(completed(&program, &catalog, &profile).value()),
            "7",
            "{receiver}"
        );
    }
}

#[test]
fn generic_owner_and_impl_template_receivers_borrow_the_caller_place() {
    for method in ["set_inherent!", "set_trait!"] {
        let source = format!(
            "trait Update[A] {{ fn set_trait!(self: &mut Self, value: A) -> Unit }}\n\
             struct Counter[T] {{\n\
               field mut value: T\n\
               fn set_inherent!(self: &mut Self, value: T) -> Unit {{ self.value = value\nyield () }}\n\
             }}\n\
             impl[T] Update[T] for Counter[T] {{\n\
               fn set_trait!(self: &mut Self, value: T) -> Unit {{ (*self).value = value\nyield () }}\n\
             }}\n\
             fn main!() -> Res[Str, Str] {{\n\
               let mut counter = Counter {{ value: 0 }}\n\
               counter.{method}(7)\n\
               yield Ok(to_str(counter.value))\n\
             }}\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        assert_eq!(
            ok_str(completed(&program, &catalog, &profile).value()),
            "7",
            "{method}"
        );
    }
}

#[test]
fn display_adapter_and_method_dispatch_supply_real_shared_references() {
    for expression in [
        "to_str(label)",
        "label.to_str()",
        "reference.to_str()",
        "stringify(label)",
        "show(label)",
        "(Label { value: 7 }).to_str()",
    ] {
        let source = format!(
            "struct Label {{ field value: i64 }}\n\
             impl Display for Label {{\n\
               fn to_str(self: &Self) -> Str {{ let copied = *self\nyield to_str(copied.value) }}\n\
             }}\n\
             fn stringify[T: Display](value: T) -> Str = to_str(value)\n\
             fn show[T: Display](value: T) -> Str = value.to_str()\n\
             fn main!() -> Res[Str, Str] {{\n\
               let label = Label {{ value: 7 }}\n\
               let reference = &label\n\
               yield Ok({expression})\n\
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
fn scalar_display_hooks_project_only_their_borrowed_input() {
    for expression in [
        "to_str(value)",
        "value.to_str()",
        "shared.to_str()",
        "writable.to_str()",
        "show(value)",
        "show_ref(shared)",
        "(7).to_str()",
    ] {
        let source = format!(
            "fn show[T: Display](value: T) -> Str = value.to_str()\n\
             fn show_ref[T: Display](value: &T) -> Str = value.to_str()\n\
             fn main!() -> Res[Str, Str] {{\n\
               let mut value = 7\n\
               let shared = &value\n\
               let writable = &mut value\n\
               yield Ok({expression})\n\
             }}\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        let attempt = execute(&program, &profile, &catalog, EvaluationLimits::default());
        let ExecutionAttempt::Executed(ExecutionOutcome::Completed(result)) = attempt else {
            panic!("{expression}: {attempt:?}");
        };
        assert_eq!(ok_str(result.value()), "7", "{expression}");
    }
}

#[test]
fn implicit_receiver_borrows_require_reference_execution_before_any_events() {
    for expression in ["counter.read()", "(Counter {}).read()", "to_str(counter)"] {
        let source = format!(
            "import std::test::emit!\n\
             struct Counter {{ fn read(self: &Self) -> Str = \"7\" }}\n\
             impl Display for Counter {{ fn to_str(self: &Self) -> Str = \"7\" }}\n\
             fn main!() -> Res[Str, Str] {{\n\
               let counter = Counter {{}}\n\
               emit!(\"before\")\n\
               yield Ok({expression})\n\
             }}\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        let reduced =
            without_execution_capability(&profile, ExecutionCapability::ReferencesAndMutation);
        assert_eq!(
            execute(&program, &reduced, &catalog, EvaluationLimits::default()),
            ExecutionAttempt::Unsupported(SupportReason::MissingCapability(
                ExecutionCapability::ReferencesAndMutation
            )),
            "{expression}"
        );
        assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
    }
}
