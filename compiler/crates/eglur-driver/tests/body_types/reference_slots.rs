use super::{diagnostic_codes, session};
use eglur_diagnostics::DiagnosticSnapshot;
use eglur_driver::{SupportProfile, TrustedCatalog};

#[test]
fn associated_cycle_is_primary_when_a_body_uses_the_poisoned_projection() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for rhs in [
        "(&i64, Self::Item)",
        "Opt[&Self::Item]",
        "(Own[i64], Self::Item)",
    ] {
        let source = format!(
            "trait HasItem {{ type Item }}\n\
             struct Broken {{}}\n\
             impl HasItem for Broken {{ type Item = {rhs} }}\n\
             fn consume[T: HasItem](carrier: T, value: T::Item) -> T::Item = value\n\
             fn main() -> i64 = consume(Broken{{}}, 1)\n"
        );
        let mut session = session(&source);
        session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        assert_eq!(
            diagnostic_codes(&session.into_diagnostics(None)),
            ["ASSOCIATED_TYPE_CYCLE"],
            "{source}"
        );
    }
}

fn assert_reference_rejection(snapshot: &DiagnosticSnapshot, source: &str) {
    assert_eq!(
        diagnostic_codes(snapshot),
        ["REFERENCE_TYPE_POSITION_FORBIDDEN"],
        "{source}"
    );
    for required in ["reference", "type position", "forbidden"] {
        assert!(
            snapshot.diagnostics()[0].message().contains(required),
            "missing {required:?}: {}\n{source}",
            snapshot.diagnostics()[0].message()
        );
    }
}

#[test]
fn reference_bearing_alias_arguments_are_rejected_even_when_erased() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for argument in [
        "&i64",
        "&mut i64",
        "(&i64, Bool)",
        "Fn(&i64) -> i64",
        "Fn(i64) -> &i64",
    ] {
        for wrapper in ["Erased", "Cell", "Opt"] {
            let source = format!(
                "type Borrow = {argument}\n\
                 type Alias = Borrow\n\
                 type Erased[T] = Unit\n\
                 struct Cell[T] {{}}\n\
                 fn invalid(value: {wrapper}[Alias]) -> Unit = ()\n"
            );
            let mut session = session(&source);

            session
                .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
                .unwrap();

            assert_reference_rejection(&session.into_diagnostics(None), &source);
        }
    }
}

#[test]
fn callable_and_constructor_inference_reject_reference_bearing_arguments() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for argument in ["&i64", "&mut i64", "(&i64, Bool)", "Fn(&i64) -> i64"] {
        for expression in [
            "consume(value)",
            "consume::[Alias](value)",
            "Cell { value: value }",
            "Choice::Value(value)",
            "Choice::Named { value: value }",
        ] {
            let source = format!(
                "type Alias = {argument}\n\
                 fn consume[T](value: T) -> Unit = ()\n\
                 struct Cell[T] {{ field value: T }}\n\
                 enum Choice[T] {{\n Value(T)\n Named {{ value: T }}\n }}\n\
                 fn invalid!(value: Alias) -> Unit {{\n\
                   let rejected = {expression}\n\
                   yield ()\n\
                 }}\n"
            );
            let mut session = session(&source);

            session
                .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
                .unwrap();

            assert_reference_rejection(&session.into_diagnostics(None), &source);
        }
    }
}

#[test]
fn expected_result_inference_cannot_hide_a_reference_in_a_generic_slot() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for expected in ["&i64", "(&i64, Bool)", "Fn(&i64) -> i64"] {
        let source = format!(
            "type Alias = {expected}\n\
             fn stop() -> Never = stop()\n\
             fn fabricate[T]() -> T = stop()\n\
             fn invalid() -> Unit {{\n\
               let rejected: Alias = fabricate()\n\
               yield ()\n\
             }}\n"
        );
        let mut session = session(&source);

        session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();

        assert_reference_rejection(&session.into_diagnostics(None), &source);
    }
}

#[test]
fn explicit_borrows_outside_generic_slots_remain_valid_through_aliases() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let source = concat!(
        "type Borrow[T] = &T\n",
        "type Alias[T] = Borrow[T]\n",
        "type Reader[T] = Fn(&T) -> i64\n",
        "fn inspect[T](value: Alias[T]) -> Unit = ()\n",
        "fn use_reader[T](value: &T, reader: Reader[T]) -> Unit = ()\n",
        "fn valid(value: &i64, reader: Fn(&i64) -> i64) -> Unit {\n",
        "  inspect(value)\n",
        "  inspect::[i64](value)\n",
        "  use_reader(value, reader)\n",
        "  yield ()\n",
        "}\n",
    );
    let mut session = session(source);

    let analysis = session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    assert_eq!(
        analysis.trace().missing().count(),
        0,
        "{:?}",
        analysis.trace().missing().collect::<Vec<_>>()
    );
    let snapshot = session.into_diagnostics(None);

    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "{:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn reference_slot_rejection_precedes_trait_bounds() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for expression in ["consume(value)", "consume::[Alias](value)"] {
        let source = format!(
            "trait Mark {{}}\n\
             type Alias = &i64\n\
             fn consume[T: Mark](value: T) -> Unit = ()\n\
             fn invalid(value: Alias) -> Unit = {expression}\n"
        );
        let mut session = session(&source);

        session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();

        assert_reference_rejection(&session.into_diagnostics(None), &source);
    }
}

#[test]
fn trait_and_associated_slots_report_the_reference_position_contract() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for source in [
        "type Alias = &i64\ntrait Use[T] {}\nfn invalid[T: Use[Alias]]() -> Unit = ()\n",
        "type Alias = &i64\ntrait Use[T] {}\nstruct Cell {}\nimpl Use[Alias] for Cell {}\n",
        "type Alias = &i64\ntrait Linked { type Item }\nstruct Cell {}\nimpl Linked for Cell { type Item = Alias }\n",
        "trait Linked { type Item }\nstruct Cell {}\nimpl Linked for Cell { type Item = Fn(&i64) -> i64 }\n",
    ] {
        let mut session = session(source);

        session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();

        assert_reference_rejection(&session.into_diagnostics(None), source);
    }
}
