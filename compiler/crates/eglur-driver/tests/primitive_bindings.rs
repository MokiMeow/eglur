use eglur_diagnostics::DiagnosticDataValue;
use eglur_driver::{
    CompileAttempt, CompileDisposition, CompilerSession, FileInput, FileSetRequest, SessionOptions,
    SupportProfile, TrustedCatalog,
};
use eglur_sema::PrimitiveType;

fn check(source: &str, profile: SupportProfile) -> CompileAttempt {
    check_files(&[("main.eglur", source)], profile)
}

fn check_files(files: &[(&str, &str)], profile: SupportProfile) -> CompileAttempt {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let request = FileSetRequest::new(
        files
            .iter()
            .map(|(path, source)| FileInput::new(*path, source.as_bytes().to_vec())),
        "main.eglur",
        (files.len() > 1).then_some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::new(false)).unwrap();
    session.resolve(&profile, Some(&catalog)).unwrap();
    CompileAttempt::finalize(session.seal(None).unwrap(), None).unwrap()
}

fn reject(source: &str, profile: SupportProfile, code: &str) -> CompileAttempt {
    let attempt = check(source, profile);
    assert_eq!(
        attempt.disposition(),
        CompileDisposition::Rejected,
        "{source}\n{}",
        attempt.render_human()
    );
    assert_eq!(
        attempt
            .compiler_result()
            .unwrap()
            .primary_diagnostic()
            .unwrap()
            .code()
            .as_str(),
        code,
        "{source}\n{}",
        attempt.render_human()
    );
    attempt
}

fn accept(source: &str) {
    let attempt = check(source, SupportProfile::semantic_core_v1());
    assert_eq!(
        attempt.disposition(),
        CompileDisposition::Succeeded,
        "{source}\n{}",
        attempt.render_human()
    );
}

#[test]
fn source_types_shadow_primitive_names_in_annotations_and_qualified_calls() {
    accept(concat!(
        "struct i32 { field flag: Bool\n",
        "fn read(self: &Self) -> Bool = self.flag }\n",
        "fn read(value: i32) -> Bool = i32.read(&value)\n",
    ));
}

#[test]
fn generic_parameters_shadow_primitive_prelude_bindings() {
    accept(concat!(
        "fn identity[i32](value: i32) -> i32 = value\n",
        "fn use() -> Bool = identity::[Bool](true)\n",
    ));
}

#[test]
fn generic_qualifiers_do_not_fall_back_to_same_named_primitive_owners() {
    for source in [
        "fn use[i32](value: &i32) -> Str = i32.to_str(value)\n",
        "struct Container[i32] { fn use(self: &Self, value: &i32) -> Str = i32.to_str(value) }\n",
    ] {
        reject(
            source,
            SupportProfile::semantic_core_v1(),
            "METHOD_NOT_FOUND",
        );
    }
    accept("fn use[i32: Display](i32: &i32) -> Str = i32.to_str()\n");
    reject(
        "fn use[i32, i32](value: &i32) -> Str = i32.to_str(value)\n",
        SupportProfile::semantic_core_v1(),
        "GENERIC_PARAMETER_REDECLARATION",
    );
}

#[test]
fn explicit_prelude_paths_and_import_aliases_recover_primitive_identity() {
    accept(concat!(
        "import std::prelude::i32 as Int32\n",
        "struct i32 {}\n",
        "fn identity(value: std::prelude::i32) -> Int32 = value\n",
    ));
}

#[test]
fn primitive_bindings_normalize_in_references_applications_callables_and_fields() {
    accept(concat!(
        "import std::prelude::i32 as Scalar\n",
        "fn reference(value: &i32) -> &Scalar = value\n",
        "fn option(value: Opt[i32]) -> Opt[Scalar] = value\n",
        "fn callable(value: Fn(i32) -> i32) -> Fn(Scalar) -> Scalar = value\n",
        "struct Holder { field value: Scalar }\n",
        "fn read_field(value: &Holder) -> std::prelude::i32 = value.value\n",
    ));
    accept(concat!(
        "type i32 = Bool\n",
        "fn reference(value: &i32) -> &Bool = value\n",
        "fn option(value: Opt[i32]) -> Opt[Bool] = value\n",
        "fn callable(value: Fn(i32) -> i32) -> Fn(Bool) -> Bool = value\n",
        "struct Holder { field value: i32 }\n",
        "fn read_field(value: &Holder) -> Bool = value.value\n",
    ));
}

#[test]
fn primitive_trait_positions_are_source_errors_not_missing_inputs() {
    for source in [
        "fn bound[T: i32](value: T) -> T = value\n",
        "fn bound[T: std::prelude::i32](value: T) -> T = value\n",
        "import std::prelude::i32 as Scalar\nfn bound[T: Scalar](value: T) -> T = value\n",
        "type Scalar = i32\nfn bound[T: Scalar](value: T) -> T = value\n",
        "struct S {}\nimpl i32 for S {}\n",
        "struct S {}\nimpl std::prelude::i32 for S {}\n",
    ] {
        reject(
            source,
            SupportProfile::semantic_core_v1(),
            "TRAIT_BOUND_INVALID",
        );
    }
}

#[test]
fn tensor_dtype_arguments_and_tensor_annotations_share_shadowing() {
    let source = concat!(
        "import std::tensor::{ones, Tensor}\n",
        "type f32 = f64\n",
        "fn make() -> Tensor[f32] = ones[f32](2)\n",
        "fn widen() -> Tensor[f64] = make()\n",
    );
    let attempt = check(source, SupportProfile::tensor_constructors_v1());
    assert_eq!(
        attempt.disposition(),
        CompileDisposition::Succeeded,
        "{}",
        attempt.render_human()
    );
}

#[test]
fn every_primitive_binding_supports_explicit_paths_and_import_aliases() {
    for primitive in PrimitiveType::ALL {
        let name = primitive.spelling();
        accept(&format!(
            "import std::prelude::{name} as Builtin\nfn identity(value: std::prelude::{name}) -> Builtin = value\n"
        ));
    }
}

#[test]
fn value_bindings_do_not_shadow_type_bindings() {
    accept("fn use(i32: Bool, value: i32) -> i32 = value\n");
    accept("fn i32() -> Bool = true\nfn use(value: i32) -> i32 = value\n");
    accept("fn use(i32: Bool) -> Str = i32.to_str()\n");
}

#[test]
fn source_type_declarations_outrank_imported_primitive_names() {
    accept(
        "import std::prelude::Bool\nstruct Bool {}\nfn identity(value: Bool) -> Bool = value\nfn make() -> Bool = Bool{}\n",
    );
}

#[test]
fn primitive_import_reexports_and_aliases_preserve_identity_across_modules() {
    let attempt = check_files(
        &[
            (
                "main.eglur",
                "import facade::Count\nstruct i32 {}\nfn identity(value: Count) -> std::prelude::i32 = value\n",
            ),
            ("facade.eglur", "pub import std::prelude::i32 as Count\n"),
        ],
        SupportProfile::semantic_core_v1(),
    );
    assert_eq!(
        attempt.disposition(),
        CompileDisposition::Succeeded,
        "{}",
        attempt.render_human()
    );
}

#[test]
fn imported_user_type_and_alias_scope_outrank_the_primitive_prelude() {
    let attempt = check_files(
        &[
            (
                "main.eglur",
                "import model::Token as i32\nimport model::Builtin\nfn identity(value: i32) -> model::Token = value\nfn canonical(value: Builtin) -> std::prelude::i32 = value\n",
            ),
            (
                "model.eglur",
                "pub struct Token {}\npub type Builtin = i32\n",
            ),
        ],
        SupportProfile::semantic_core_v1(),
    );
    assert_eq!(
        attempt.disposition(),
        CompileDisposition::Succeeded,
        "{}",
        attempt.render_human()
    );
}

#[test]
fn higher_tier_import_errors_do_not_fall_through_to_a_primitive_binding() {
    reject(
        "import std::prelude::i32\nimport std::prelude::Bool as i32\nfn identity(value: i32) -> i32 = value\n",
        SupportProfile::semantic_core_v1(),
        "NAME_AMBIGUOUS",
    );
    let attempt = check_files(
        &[
            (
                "main.eglur",
                "import model::Missing as i32\nfn identity(value: i32) -> i32 = value\n",
            ),
            ("model.eglur", "pub struct Present {}\n"),
        ],
        SupportProfile::semantic_core_v1(),
    );
    assert_eq!(
        attempt.disposition(),
        CompileDisposition::Rejected,
        "{}",
        attempt.render_human()
    );
    assert_eq!(
        attempt
            .compiler_result()
            .unwrap()
            .primary_diagnostic()
            .unwrap()
            .code()
            .as_str(),
        "IMPORT_NOT_FOUND"
    );
}

#[test]
fn shadowed_scalar_diagnostics_distinguish_source_types_from_literals() {
    let attempt = reject(
        "struct Bool {}\nfn bad() -> Bool = true\n",
        SupportProfile::semantic_core_v1(),
        "RETURN_TYPE_MISMATCH",
    );
    let result = attempt.compiler_result().unwrap();
    let diagnostic = result.primary_diagnostic().unwrap();
    assert_eq!(
        diagnostic.data().get("expected_type"),
        Some(&DiagnosticDataValue::from("Bool"))
    );
    assert_eq!(
        diagnostic.data().get("actual_type"),
        Some(&DiagnosticDataValue::from("std::prelude::Bool"))
    );
}

#[test]
fn primitive_diagnostic_names_do_not_use_transparent_import_aliases() {
    let attempt = reject(
        "import std::prelude::i32 as Count\nfn bad(value: Count) -> Bool = value\n",
        SupportProfile::semantic_core_v1(),
        "RETURN_TYPE_MISMATCH",
    );
    let result = attempt.compiler_result().unwrap();
    assert_eq!(
        result
            .primary_diagnostic()
            .unwrap()
            .data()
            .get("actual_type"),
        Some(&DiagnosticDataValue::from("i32"))
    );
}

#[test]
fn primitive_arity_and_reserved_intrinsic_policies_remain_separate() {
    for ty in ["i32[Bool]", "std::prelude::i32[Bool]"] {
        reject(
            &format!("fn bad(value: {ty}) -> Unit = ()\n"),
            SupportProfile::semantic_core_v1(),
            "TYPE_ARG_ARITY_MISMATCH",
        );
    }
    reject(
        "struct Opt {}\n",
        SupportProfile::semantic_core_v1(),
        "RESERVED_BUILTIN_NAME",
    );
    reject(
        "type i32 = i32\nfn use(value: i32) -> Unit = ()\n",
        SupportProfile::semantic_core_v1(),
        "TYPE_ALIAS_CYCLE",
    );
}

#[test]
fn primitive_type_qualifiers_do_not_import_display_trait_methods() {
    for owner in ["i32", "std::prelude::i32", "Int32", "Number"] {
        let source = format!(
            "import std::prelude::i32 as Int32\ntype Number = i32\nfn use(value: i32) -> Str = {owner}.to_str(&value)\n"
        );
        reject(
            &source,
            SupportProfile::semantic_core_v1(),
            "METHOD_NOT_FOUND",
        );
    }
}

#[test]
fn complete_empty_primitive_inventory_rejects_unknown_methods() {
    for owner in ["i32", "Number", "std::prelude::i32"] {
        let source = format!("type Number = i32\nfn use() -> Unit = {owner}.no_such_method()\n");
        reject(
            &source,
            SupportProfile::semantic_core_v1(),
            "METHOD_NOT_FOUND",
        );
    }
}

#[test]
fn tensor_closed_dtype_families_check_identity_not_shadowed_spelling() {
    for source in [
        "import std::tensor::{ones, Tensor}\nstruct f32 {}\nfn bad() -> Tensor[f32] = ones[f32](2)\n",
        "import std::tensor::{nan, Tensor}\ntype f32 = f64\nfn bad() -> Tensor[f32] = nan[f32](2)\n",
        "import std::tensor::{ones, Tensor}\nfn bad[f32]() -> Tensor[f32] = ones[f32](2)\n",
    ] {
        reject(
            source,
            SupportProfile::tensor_constructors_v1(),
            "CALL_ARG_TYPE_MISMATCH",
        );
    }
    let source = "import std::tensor::{ones, Tensor}\nstruct f32 {}\nfn make() -> Tensor[std::prelude::f32] = ones[std::prelude::f32](2)\n";
    assert_eq!(
        check(source, SupportProfile::tensor_constructors_v1()).disposition(),
        CompileDisposition::Succeeded
    );
}

#[test]
fn bool_dtype_shorthand_keeps_canonical_identity_under_shadowing() {
    let source = concat!(
        "import std::tensor::{zeros, Tensor}\n",
        "struct Bool {}\n",
        "fn make() -> Tensor[std::prelude::Bool] = zeros[bool](2)\n",
    );
    assert_eq!(
        check(source, SupportProfile::tensor_constructors_v1()).disposition(),
        CompileDisposition::Succeeded
    );
}

#[test]
fn primitive_type_bindings_are_not_struct_constructor_declarations() {
    for head in ["i32", "std::prelude::i32", "Count"] {
        let source = format!(
            "import std::prelude::i32 as Count\nfn bad() -> Unit {{ _ = {head}{{}}\n yield () }}\n"
        );
        let attempt = reject(
            &source,
            SupportProfile::semantic_core_v1(),
            "CONSTRUCTOR_TARGET_KIND_MISMATCH",
        );
        let result = attempt.compiler_result().unwrap();
        let data = result.primary_diagnostic().unwrap().data();
        assert_eq!(
            data.get("actual_kind"),
            Some(&DiagnosticDataValue::from("primitive_type"))
        );
        assert_eq!(
            data.get("expected_kind"),
            Some(&DiagnosticDataValue::from("struct"))
        );
    }
}

#[test]
fn primitive_bindings_do_not_require_trusted_nominal_metadata() {
    let request = FileSetRequest::single("main.eglur", b"import std::prelude::i32 as Count\nfn identity(value: Count) -> std::prelude::i32 = value\n".to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let profile = SupportProfile::semantic_core_v1();
    session.resolve(&profile, None).unwrap();
    let snapshot = session.into_diagnostics(None);
    assert!(snapshot.diagnostics().is_empty(), "{snapshot:?}");
}
