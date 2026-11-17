use eglur_diagnostics::DiagnosticDataValue;
use eglur_driver::{
    CompileAttempt, CompileDisposition, CompilerSession, FileSetRequest, SessionOptions,
    SupportProfile, SupportProfileId, SupportSchemaVersion, TrustedCatalog,
};
use eglur_sema::{CompileCapability, TypecheckCapability};
use serde_json::Value;

fn check(source: &str, profile: &SupportProfile) -> CompileAttempt {
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    session.resolve(profile, Some(&catalog)).unwrap();
    let sealed = session.seal(None).unwrap();
    if sealed.trace().has_unsupported() {
        assert!(sealed.checked_program().is_none());
    }
    CompileAttempt::finalize(sealed, None).unwrap()
}

fn assert_missing_member(source: &str, receiver_type: &str) {
    let attempt = check(source, &SupportProfile::semantic_core_v1());
    assert_eq!(
        attempt.disposition(),
        CompileDisposition::Rejected,
        "{source}"
    );
    let result = attempt.compiler_result().unwrap();
    let diagnostic = result.primary_diagnostic().unwrap();
    assert_eq!(diagnostic.code().as_str(), "MEMBER_NOT_FOUND", "{source}");
    assert_eq!(
        diagnostic.data().get("member"),
        Some(&DiagnosticDataValue::from("missing")),
        "{source}"
    );
    assert_eq!(
        diagnostic.data().get("receiver_type"),
        Some(&DiagnosticDataValue::from(receiver_type)),
        "{source}"
    );
    let range = diagnostic
        .primary_span()
        .as_source()
        .unwrap()
        .file_span()
        .range();
    assert_eq!(
        &source[range.start().as_usize()..range.end().as_usize()],
        "missing"
    );
}

#[test]
fn missing_struct_members_are_source_errors_with_member_token_blame() {
    for source in [
        "struct Point { field x: i32 }\nfn read(value: Point) -> i32 = value.missing\n",
        "struct Point { field x: i32 }\nfn read(value: &Point) -> i32 = value.missing\n",
        "struct Point { field x: i32 }\nfn read!(value: &mut Point) -> i32 = value.missing\n",
        "struct Point { field x: i32 }\ntype Alias = Point\nfn read(value: &Alias) -> i32 = value.missing\n",
        "struct Point { prop x: i32 { fn get_x(self: &Self) -> i32 = 0 } }\nfn read(value: Point) -> i32 = value.missing\n",
        "struct Point { field x: i32 }\nfn read(value: Point) -> Unit { value.missing = true }\n",
    ] {
        assert_missing_member(source, "Point");
    }
}

#[test]
fn non_field_receivers_do_not_refuse_implemented_method_resolution() {
    for (source, receiver) in [
        ("fn read(value: i32) -> Unit = value.missing\n", "i32"),
        ("fn read(value: &Bool) -> Unit = value.missing\n", "Bool"),
        ("fn read(value: Str) -> Unit = value.missing\n", "Str"),
        ("fn read(value: Unit) -> Unit = value.missing\n", "Unit"),
        (
            "fn read(value: (i32, Bool)) -> Unit = value.missing\n",
            "(i32, Bool)",
        ),
        (
            "fn read(value: Fn() -> Unit) -> Unit = value.missing\n",
            "Fn() -> Unit",
        ),
        ("fn read[T](value: T) -> Unit = value.missing\n", "T"),
        (
            "fn read[T: Display](value: &T) -> Unit = value.missing\n",
            "T",
        ),
        (
            "fn read(value: Opt[i32]) -> Unit = value.missing\n",
            "Opt[i32]",
        ),
        (
            "fn read(value: Res[i32, Str]) -> Unit = value.missing\n",
            "Res[i32, Str]",
        ),
        (
            "enum Choice { Item { value: i32 } }\nfn read(value: Choice) -> Unit = value.missing\n",
            "Choice",
        ),
        (
            "type i32 = Bool\nfn read(value: std::prelude::i32) -> Unit = value.missing\n",
            "std::prelude::i32",
        ),
    ] {
        assert_missing_member(source, receiver);
    }
}

#[test]
fn method_name_is_not_a_stored_field_or_property() {
    assert_missing_member(
        "struct Cell { fn missing(self: &Self) -> Unit = () }\nfn read(cell: Cell) -> Unit = cell.missing\n",
        "Cell",
    );
    assert_missing_member(
        "enum Choice { Item { missing: i32 } }\nfn read(value: Choice) -> Unit = value.missing\n",
        "Choice",
    );
}

#[test]
fn trusted_nominal_member_layout_refuses_input_without_a_source_error() {
    for (source, owner) in [
        (
            "fn read(value: &NumericError) -> Unit = value.missing\n",
            "std::prelude::NumericError",
        ),
        (
            "type Alias = NumericError\nfn read(value: &Alias) -> Unit = value.missing\n",
            "std::prelude::NumericError",
        ),
        (
            "import std::io::IoError\nfn read(value: IoError) -> Unit = value.missing\n",
            "std::io::IoError",
        ),
        (
            "import std::tensor::Tensor\nfn read(value: &Tensor[f32]) -> Unit = value.missing\n",
            "std::tensor::Tensor",
        ),
    ] {
        let attempt = check(source, &SupportProfile::semantic_core_v1());
        assert_eq!(
            attempt.disposition(),
            CompileDisposition::Unsupported,
            "{source}"
        );
        let payload: Value =
            serde_json::from_str(&attempt.development_attempt().unwrap().to_json()).unwrap();
        assert!(
            payload["diagnostics"].as_array().unwrap().is_empty(),
            "{payload}"
        );
        let entry = payload["capability_trace"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["capability"] == "typecheck.members")
            .unwrap();
        assert_eq!(entry["disposition"], "satisfied", "{payload}");
        let inputs = entry["unavailable_inputs"].as_array().unwrap();
        assert_eq!(inputs.len(), 1, "{payload}");
        assert_eq!(inputs[0]["kind"], "trusted_value_member_inventory");
        assert_eq!(inputs[0]["owner"], owner);
        assert!(!inputs[0]["locations"].as_array().unwrap().is_empty());
        assert_eq!(
            attempt.render_human(),
            format!(
                "tool unsupported: typecheck.members (missing field/property inventory: {owner})"
            )
        );
    }
}

#[test]
fn incomplete_member_input_retains_source_errors_without_a_conclusive_result() {
    let attempt = check(
        "struct Cell {}\nfn invalid(cell: Cell) -> Unit = cell.missing\nfn external(value: NumericError) -> Unit = value.missing\n",
        &SupportProfile::semantic_core_v1(),
    );
    assert_eq!(attempt.disposition(), CompileDisposition::Unsupported);
    assert!(attempt.compiler_result().is_err());
    let CompileAttempt::Unsupported(unsupported) = attempt else {
        unreachable!()
    };
    assert_eq!(
        unsupported.diagnostics().diagnostics()[0].code().as_str(),
        "MEMBER_NOT_FOUND"
    );
}

#[test]
fn closure_support_remains_a_distinct_refusal() {
    let attempt = check(
        "fn value() -> Fn() -> Unit = { || () }\n",
        &SupportProfile::semantic_core_v1(),
    );
    assert_eq!(attempt.disposition(), CompileDisposition::Unsupported);
    assert_eq!(
        attempt.render_human(),
        "tool unsupported: typecheck.closures"
    );
}

#[test]
fn stored_property_and_qualified_callable_reads_remain_supported() {
    for source in [
        "struct Cell[T] { field value: T }\nfn read[T](cell: &Cell[T]) -> T = cell.value\n",
        "struct Cell { field mut value: i32 }\nfn write!(cell: &mut Cell) -> Unit { cell.value = 1 }\n",
        "struct Cell { field value: i32\nprop count: i32 { fn get_count(self: &Self) -> i32 = self.value } }\nfn read(cell: &Cell) -> i32 = cell.count\n",
        "struct Cell { fn read(self: &Self) -> i32 = 0 }\nfn value() -> Fn(&Cell) -> i32 = Cell.read\nfn call(cell: &Cell) -> i32 = cell.read()\n",
        "fn read(value: i32) -> Str = value.to_str()\n",
    ] {
        let attempt = check(source, &SupportProfile::semantic_core_v1());
        assert_eq!(
            attempt.disposition(),
            CompileDisposition::Succeeded,
            "{source}\n{}",
            attempt.render_human()
        );
    }
}

#[test]
fn poisoned_receiver_and_method_call_keep_their_own_diagnostics() {
    for (source, expected) in [
        ("fn read() -> Unit = absent.missing\n", "NAME_NOT_FOUND"),
        (
            "struct Cell {}\nfn read(cell: Cell) -> Unit = cell.missing()\n",
            "METHOD_NOT_FOUND",
        ),
        (
            "struct Cell {}\nfn read() -> Fn(&Cell) -> Unit = Cell.missing\n",
            "METHOD_NOT_FOUND",
        ),
    ] {
        let attempt = check(source, &SupportProfile::semantic_core_v1());
        assert_eq!(
            attempt.disposition(),
            CompileDisposition::Rejected,
            "{source}"
        );
        assert_eq!(
            attempt
                .compiler_result()
                .unwrap()
                .primary_diagnostic()
                .unwrap()
                .code()
                .as_str(),
            expected,
            "{source}"
        );
    }
}

#[test]
fn missing_member_capability_is_not_negative_lookup_evidence() {
    let full = SupportProfile::semantic_core_v1();
    let profile = SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("no-member-values").unwrap(),
        full.compile_capabilities().filter(|capability| {
            *capability != CompileCapability::Typecheck(TypecheckCapability::ResolveMembers)
        }),
    )
    .unwrap();
    let attempt = check(
        "struct Cell {}\nfn read(cell: Cell) -> Unit = cell.missing\n",
        &profile,
    );
    assert_eq!(attempt.disposition(), CompileDisposition::Unsupported);
    assert!(attempt.render_human().contains("typecheck.members"));
    let CompileAttempt::Unsupported(unsupported) = attempt else {
        unreachable!()
    };
    assert!(unsupported.diagnostics().diagnostics().is_empty());
}
