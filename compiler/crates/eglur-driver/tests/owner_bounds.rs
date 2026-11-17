use eglur_diagnostics::{DiagnosticDataValue, DiagnosticSnapshot};
use eglur_driver::{
    CompileAttempt, CompileDisposition, CompilerSession, FileSetRequest, SessionOptions,
    SupportProfile, TrustedCatalog,
};

const CARRIER: &str = concat!(
    "trait Carries { type Item }\n",
    "struct Carrier {}\n",
    "impl Carries for Carrier { type Item = i64 }\n",
    "type Item[T: Carries] = T::Item\n",
);

fn compile(source: &str) -> (DiagnosticSnapshot, CompileDisposition) {
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    session
        .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
        .unwrap();
    let sealed = session.seal(None).unwrap();
    assert!(!sealed.trace().has_missing(), "{source}");
    let diagnostics = sealed.diagnostics().clone();
    let disposition = CompileAttempt::finalize(sealed, None)
        .unwrap()
        .disposition();
    (diagnostics, disposition)
}

fn assert_accepted(source: &str) {
    let (diagnostics, disposition) = compile(source);
    assert!(
        diagnostics.diagnostics().is_empty(),
        "{source}: {diagnostics:?}"
    );
    assert_eq!(disposition, CompileDisposition::Succeeded, "{source}");
}

#[test]
fn invocation_normalizes_both_owner_and_required_bounds() {
    for caller_argument in ["Item[Carrier]", "i64"] {
        for callee_argument in ["Item[Carrier]", "i64"] {
            let source = format!(
                "{CARRIER}trait Link[A] {{}}\n\
                 fn accept[T: Link[{callee_argument}]](x: T) -> Unit = ()\n\
                 fn take[T: Link[{caller_argument}]](x: T) -> Unit = accept(x)\n"
            );
            assert_accepted(&source);
        }
    }
}

#[test]
fn dependent_owner_bounds_normalize_before_invocation_in_either_parameter_order() {
    for parameters in [
        "U: Link[DerivedItem[Wrap[T]]], T: Uses[Item[Carrier]]",
        "T: Uses[Item[Carrier]], U: Link[DerivedItem[Wrap[T]]]",
    ] {
        let source = format!(
            "{CARRIER}trait Uses[A] {{}}\ntrait Derived {{ type Item }}\ntrait Link[A] {{}}\n\
             struct Wrap[T] {{}}\nimpl[T: Uses[i64]] Derived for Wrap[T] {{ type Item = Bool }}\n\
             type DerivedItem[T: Derived] = T::Item\n\
             fn accept[T: Link[Bool]](x: T) -> Unit = ()\n\
             fn take[{parameters}](x: U) -> Unit = accept(x)\n"
        );
        assert_accepted(&source);
    }
}

#[test]
fn receiver_candidates_use_normalized_owner_trait_arguments() {
    let source = format!(
        "{CARRIER}trait Link[A] {{ fn linked(self: &Self, value: A) -> A }}\n\
         fn take[T: Link[Item[Carrier]]](x: T) -> i64 = x.linked(7)\n"
    );
    assert_accepted(&source);
}

#[test]
fn normalized_current_trait_and_current_impl_evidence_remain_available() {
    let source = format!(
        "{CARRIER}trait Link[A] {{\n\
             fn linked(self: &Self, value: A) -> A\n\
             fn forward(self: &Self, value: A) -> A = self.linked(value)\n}}\n\
             struct Value {{}}\nimpl Link[i64] for Value {{\n\
             fn linked(self: &Self, value: i64) -> i64 = value\n\
             fn another(self: &Self, value: i64) -> i64 = self.linked(value)\n}}\n\
             fn take[T: Link[Item[Carrier]]](x: T) -> i64 = x.forward(7)\n"
    );
    assert_accepted(&source);
}

#[test]
fn normalized_evidence_does_not_supply_wrong_or_other_owner_bounds() {
    for invalid_bound in ["", ": Link[Bool]"] {
        let source = format!(
            "{CARRIER}trait Link[A] {{}}\n\
             fn accept[T: Link[i64]](x: T) -> Unit = ()\n\
             fn valid[T: Link[Item[Carrier]]](x: T) -> Unit = accept(x)\n\
             fn invalid[T{invalid_bound}](x: T) -> Unit = accept(x)\n"
        );
        let (snapshot, disposition) = compile(&source);
        assert_eq!(disposition, CompileDisposition::Rejected);
        let diagnostics = snapshot.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code().as_str(), "TRAIT_BOUND_NOT_SATISFIED");
        assert_eq!(
            diagnostics[0].data().get("trait"),
            Some(&DiagnosticDataValue::from("Link[i64]"))
        );
    }
}

#[test]
fn trait_default_bodies_check_with_normalized_inherited_bounds() {
    assert_accepted(&format!(
        "{CARRIER}trait Uses[A] {{ fn fetch(self: &Self) -> i64 }}\n\
         impl Uses[i64] for Bool {{ fn fetch(self: &Self) -> i64 = 7 }}\n\
         fn accept[T: Uses[i64]](x: T) -> i64 = x.fetch()\n\
         trait Runs[T: Uses[Item[Carrier]]] {{ fn run(self: &Self, x: T) -> i64 = accept(x) }}\n\
         struct Runner {{}}\nimpl Runs[Bool] for Runner {{}}\n\
         fn take(x: Runner) -> i64 = x.run(true)\n"
    ));
}

#[test]
fn exact_symbolic_assumptions_survive_without_proving_a_recursive_impl_cycle() {
    assert_accepted(concat!(
        "trait Link[A] {}\n",
        "fn accept[A: Link[B], B: Link[A]](x: A, y: B) -> Unit = ()\n",
        "fn take[A: Link[B], B: Link[A]](x: A, y: B) -> Unit = accept(x, y)\n",
    ));
    let source = format!(
        "{CARRIER}trait Mark {{}}\ntrait Link[A] {{}}\nimpl[T: Mark] Mark for T {{}}\n\
         fn accept[T: Mark](x: T) -> Unit = ()\n\
         fn take[T: Link[Item[Carrier]]](x: T) -> Unit = accept(1)\n"
    );
    let (snapshot, disposition) = compile(&source);
    assert_eq!(disposition, CompileDisposition::Rejected);
    assert_eq!(snapshot.diagnostics().len(), 1);
    assert_eq!(
        snapshot.diagnostics()[0].code().as_str(),
        "TRAIT_BOUND_NOT_SATISFIED"
    );
    assert_eq!(
        snapshot.diagnostics()[0].data().get("trait"),
        Some(&DiagnosticDataValue::from("Mark"))
    );
}

#[test]
fn late_impl_failure_invalidates_owner_bounds_without_derivative_call_errors() {
    let source = concat!(
        "trait Link[A] {}\n",
        "trait Carries { type Item\nfn get(self: &Self) -> Self::Item }\n",
        "struct Carrier {}\nimpl Carries for Carrier { type Item = i64 }\n",
        "type Item[T: Carries] = T::Item\n",
        "fn accept[T: Link[i64]](x: T) -> Unit = ()\n",
        "fn take[T: Link[Item[Carrier]]](x: T) -> Unit = accept(x)\n",
        "fn independent() -> Bool = 1\n",
    );
    let (snapshot, disposition) = compile(source);
    assert_eq!(disposition, CompileDisposition::Rejected);
    let codes = snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect::<Vec<_>>();
    assert_eq!(codes, ["TRAIT_IMPL_MISSING_METHOD", "RETURN_TYPE_MISMATCH"]);
}
