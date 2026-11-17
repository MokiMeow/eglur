use super::{accept, reject};
use eglur_driver::{
    CompilerSession, FileSetRequest, SessionOptions, SupportProfile, TrustedCatalog,
};
use eglur_sema::{CompileCapability, TypecheckCapability};

const IMPORTS: &str = "import std::tensor::ones\nimport std::tensor::Tensor\n";

#[test]
fn nested_constructor_call_operands_seal_on_the_default_stack() {
    let depth = 100;
    let source = format!(
        "{IMPORTS}fn dim(t: Tensor[f32]) -> Nat = 2\nfn make() -> Tensor[f32] = {}ones[f32](2){}\n",
        "ones[f32](dim(".repeat(depth),
        "))".repeat(depth),
    );
    accept(&source);
}

#[test]
fn nested_aliased_constructor_operands_keep_canonical_identity() {
    let depth = 100;
    let source = format!(
        "import std::tensor::zeros as filled\nimport std::tensor::Tensor\ntype Real = f32\nfn dim(t: Tensor[Real]) -> Nat = 2\nfn make() -> Tensor[Real] = {}filled[Real](2){}\n",
        "filled[Real](dim(".repeat(depth),
        "))".repeat(depth),
    );
    accept(&source);
}

#[test]
fn nested_constructor_block_operands_share_the_work_stack() {
    let depth = 100;
    let source = format!(
        "{IMPORTS}fn make() -> Tensor[f32] = {}ones[f32](2){}\n",
        "ones[f32]({ let t = ".repeat(depth),
        "\nyield 2 })".repeat(depth),
    );
    accept(&source);
}

#[test]
fn nested_constructor_branch_operands_share_the_work_stack() {
    let depth = 60;
    let source = format!(
        "{IMPORTS}fn make(flag: Bool) -> Tensor[f32] = {}ones[f32](2){}\n",
        "ones[f32](if flag { let t = ".repeat(depth),
        "\nyield 2 } else { yield 0 })".repeat(depth),
    );
    accept(&source);
}

#[test]
fn nested_constructor_checked_operands_restore_numeric_contexts() {
    let depth = 60;
    let source = format!(
        "{IMPORTS}fn dim(r: Res[Nat, NumericError]) -> Nat = match r {{ Ok(n) => n, Err(e) => 0 }}\nfn make() -> Tensor[f32] = {}ones[f32](2){}\n",
        "ones[f32](dim(checked { let t = ".repeat(depth),
        "\nyield 2 }))".repeat(depth),
    );
    accept(&source);
}

#[test]
fn nested_constructor_result_operands_restore_failure_contexts() {
    let depth = 60;
    let source = format!(
        "{IMPORTS}fn dim(r: Res[Nat, Str]) -> Nat = match r {{ Ok(n) => n, Err(e) => 0 }}\nfn make() -> Tensor[f32] = {}ones[f32](2){}\n",
        "ones[f32](dim(result { let t = ".repeat(depth),
        "\nyield Ok(2) }))".repeat(depth),
    );
    accept(&source);
}

#[test]
fn nested_diverging_constructor_operands_preserve_path_barriers() {
    let depth = 100;
    let source = format!(
        "{IMPORTS}fn stop() -> Never = stop()\nfn make() -> Tensor[f32] = {}ones[f32](2){}\n",
        "ones[f32](stop(), { return ".repeat(depth),
        " })".repeat(depth),
    );
    accept(&source);
}

#[test]
fn deep_invalid_constructor_operand_keeps_its_single_diagnostic() {
    let depth = 100;
    let source = format!(
        "{IMPORTS}fn dim(t: Tensor[f32]) -> Nat = 2\nfn make() -> Tensor[f32] = {}ones[f32](-1){}\n",
        "ones[f32](dim(".repeat(depth),
        "))".repeat(depth),
    );
    reject(&source, "DOMAIN_INVALID_VALUE");
}

#[test]
fn deep_unsupported_constructor_operand_does_not_publish_success() {
    let depth = 100;
    let source = format!(
        "{IMPORTS}fn dim(t: Tensor[f32]) -> Nat = 2\nfn make() -> Tensor[f32] = {}ones[f32](dims = 2){}\n",
        "ones[f32](dim(".repeat(depth),
        "))".repeat(depth),
    );
    let request = FileSetRequest::single("main.eglur", source.into_bytes()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve(&SupportProfile::tensor_constructors_v1(), Some(&catalog))
        .unwrap();
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::AnalyzeTensorSource)
    }));
    assert!(analysis.fingerprints().is_none());

    let sealed = session.seal(None).unwrap();
    assert!(sealed.checked_program().is_none());
    assert!(sealed.diagnostics().diagnostics().is_empty());
}
