use eglur_driver::{
    CompileAttempt, CompileDisposition, CompilerSession, FileInput, FileSetRequest, ProgramMode,
    SessionOptions, SupportProfile, TrustedCatalog,
};

fn check(source: &str) -> CompileAttempt {
    check_files(&[("main.eglur", source)])
}

fn check_files(files: &[(&str, &str)]) -> CompileAttempt {
    let request = FileSetRequest::new(
        files
            .iter()
            .map(|(path, source)| FileInput::new(*path, source.as_bytes().to_vec())),
        "main.eglur",
        (files.len() > 1).then_some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    session
        .resolve_for(
            ProgramMode::Library,
            &SupportProfile::semantic_core_v1(),
            Some(&catalog),
        )
        .unwrap();
    CompileAttempt::finalize(session.seal(None).unwrap(), None).unwrap()
}

#[test]
fn intrinsic_reexports_preserve_identity_across_module_and_alias_boundaries() {
    for (head, arguments) in [
        ("Own", "NumericError"),
        ("Box", "i64"),
        ("Opt", "i64"),
        ("Res", "i64, NumericError"),
    ] {
        let facade = format!("pub import std::prelude::{head} as Handle\n");
        let bridge = "pub import facade::Handle as Item\n";
        let main = format!(
            "import bridge::Item\nfn expected(v: &{head}[{arguments}]) -> Unit = ()\nfn use(v: &Item[{arguments}]) -> Unit = expected(v)\n"
        );
        let attempt = check_files(&[
            ("main.eglur", &main),
            ("facade.eglur", &facade),
            ("bridge.eglur", bridge),
        ]);
        assert_eq!(
            attempt.disposition(),
            CompileDisposition::Succeeded,
            "{head}\n{}",
            attempt.render_human()
        );
    }
}

#[test]
fn default_intrinsic_inventory_absence_is_not_negative_evidence() {
    for (head, applied) in [
        ("Own", "Own[NumericError]"),
        ("Box", "Box[i64]"),
        ("Opt", "Opt[i64]"),
        ("Res", "Res[i64, NumericError]"),
    ] {
        let mut sources = vec![
            format!("fn use(v: &{applied}) -> Unit = {head}.missing(v)\n"),
            format!("fn use(v: &{applied}) -> Unit = (({head}.missing))(v)\n"),
            format!("fn use(v: &{applied}) -> Unit = v.missing()\n"),
            format!(
                "fn use() -> Unit {{ let f: Fn() -> Unit = {head}.missing\n_ = f\nyield () }}\n"
            ),
        ];
        if head != "Own" {
            sources.push(format!(
                "type Fixed = {applied}\nfn use(v: &Fixed) -> Unit = Fixed.missing(v)\n"
            ));
        }
        for source in sources {
            let attempt = check(&source);
            assert_eq!(
                attempt.disposition(),
                CompileDisposition::Unsupported,
                "{source}\n{}",
                attempt.render_human()
            );
            let json: serde_json::Value =
                serde_json::from_str(&attempt.development_attempt().unwrap().to_json()).unwrap();
            let entry = json["capability_trace"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["capability"] == "typecheck.trusted_inherent_methods")
                .unwrap();
            let inputs = entry["unavailable_inputs"].as_array().unwrap();
            assert!(inputs.iter().any(|input| {
                input["kind"] == "intrinsic_inherent_inventory"
                    && input["owner"] == format!("std::prelude::{head}")
            }));
            let CompileAttempt::Unsupported(unsupported) = &attempt else {
                unreachable!("the attempt disposition was checked above");
            };
            assert!(unsupported.diagnostics().diagnostics().is_empty());
        }
    }
}

#[test]
fn explicit_paths_and_import_aliases_preserve_intrinsic_type_identity() {
    for (head, arguments) in [
        ("Own", "NumericError"),
        ("Box", "i64"),
        ("Opt", "i64"),
        ("Res", "i64, NumericError"),
    ] {
        for source in [
            format!(
                "fn expected(v: &{head}[{arguments}]) -> Unit = ()\nfn use(v: &std::prelude::{head}[{arguments}]) -> Unit = expected(v)\n"
            ),
            format!(
                "import std::prelude::{head} as Imported\nfn expected(v: &{head}[{arguments}]) -> Unit = ()\nfn use(v: &Imported[{arguments}]) -> Unit = expected(v)\n"
            ),
            format!(
                "import std::prelude::{head}\nfn expected(v: &{head}[{arguments}]) -> Unit = ()\nfn use(v: &{head}[{arguments}]) -> Unit = expected(v)\n"
            ),
        ] {
            let attempt = check(&source);
            assert_eq!(
                attempt.disposition(),
                CompileDisposition::Succeeded,
                "{source}\n{}",
                attempt.render_human()
            );
        }
    }
}

#[test]
fn imported_intrinsics_keep_argument_domains_and_reserved_binding_rules() {
    for (source, code) in [
        (
            "import std::prelude::Box as Imported\nfn use(v: Imported[&i64]) -> Unit = ()\n",
            "REFERENCE_TYPE_POSITION_FORBIDDEN",
        ),
        (
            "import std::prelude::Box as Imported\nfn use(v: Imported[Own[NumericError]]) -> Unit = ()\n",
            "OWNERSHIP_TYPE_POSITION_FORBIDDEN",
        ),
        (
            "import std::prelude::Res as Imported\nfn use(v: Imported[i64]) -> Unit = ()\n",
            "TYPE_ARG_ARITY_MISMATCH",
        ),
        ("import std::prelude::Box as Opt\n", "RESERVED_BUILTIN_NAME"),
        ("struct Own {}\n", "RESERVED_BUILTIN_NAME"),
        (
            "type Fixed = Own[NumericError]\n",
            "OWNERSHIP_TYPE_POSITION_FORBIDDEN",
        ),
        ("fn use[T: Opt](v: T) -> Unit = ()\n", "TRAIT_BOUND_INVALID"),
        (
            "fn use(f: Fn(&Own[NumericError]) -> Unit) -> Unit = ()\n",
            "OWNERSHIP_TYPE_POSITION_FORBIDDEN",
        ),
        ("struct S {}\nimpl Box for S {}\n", "TRAIT_BOUND_INVALID"),
    ] {
        let attempt = check(source);
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
            code,
            "{source}\n{}",
            attempt.render_human()
        );
    }
}
