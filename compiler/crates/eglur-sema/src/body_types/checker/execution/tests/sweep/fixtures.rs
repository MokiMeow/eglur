use super::*;

pub(super) fn with_inputs(
    text: &str,
    inspect: impl FnOnce(BodyPhaseInputs<'_, '_>, &ExternalNameEnvironment),
) {
    let error = crate::ExternalDefId::new("fixture::NumericError").unwrap();
    let module = crate::ModuleAddress::new(["fixture".into()]).unwrap();
    let external = ExternalNameEnvironment::new(
        [module.clone()],
        [crate::ExternalDeclaration::new(
            error.clone(),
            module,
            NameKey::new("NumericError", false).unwrap(),
            DeclarationKind::OpaqueType,
        )
        .unwrap()],
        [error.clone()],
    )
    .unwrap();
    let signatures =
        crate::TrustedSignatureEnvironment::new([crate::TrustedDeclarationSignature::new(
            error.clone(),
            crate::TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([]),
            },
        )])
        .unwrap();
    let items =
        crate::TrustedLanguageItemEnvironment::new([(LanguageItem::NumericError, error)]).unwrap();
    with_phase_inputs(
        text,
        &external,
        Some(crate::TrustedDeclarationInputs::new(&signatures, None).with_language_items(&items)),
        |inputs| inspect(inputs, &external),
    );
}

pub(super) fn sources() -> Vec<(&'static str, String, bool)> {
    let id = "fn id[T](value: T) -> T = value";
    let mut sources = vec![
        ("nested", format!("{id}\nfn inspect(x: i32) -> i32 = id({{ yield (id(x), x).0 }})"), true),
        ("explicit", format!("{id}\nfn inspect(x: i32) -> i32 = id::[i32](x)"), true),
        ("probe", format!("{id}\nfn inspect(x: i32) -> Unit {{\nlet out = id(x)\n}}"), true),
        ("function-value", format!("{id}\nfn inspect(x: i32) -> i32 {{\nlet f: Fn(i32) -> i32 = id\nyield f(x)\n}}"), true),
        ("wide-callable-value", "struct S {}\noverload fn choose[T](x: S, seed: T, tag: Bool) -> T = seed\noverload fn choose[T, U](x: T, seed: U, tag: i32) -> U = seed\nfn inspect() -> Unit {\nlet value: Fn((i32, i32, i32, i32), i32, i32) -> i32 = choose\nyield ()\n}".into(), true),
        ("branches", "fn inspect(flag: Bool, x: i32, y: i32) -> i32 {\nlet v: i32 = if flag { yield x } else { yield y }\nyield match v { _ if flag => x, _ => y }\n}".into(), true),
        ("pattern-bindings", format!("{id}\nfn inspect(input: ((i32, i32), (i32, i32))) -> i32 = match input {{ ((first, second), (third, fourth)) if true => id(first), ((a, b), (c, d)) => id(d) }}"), true),
        ("tuple-rest", format!("type Wide = (i32, Bool, i32, Bool, i32, Bool, i32, Bool)\n{id}\nfn inspect(input: Wide) -> i32 = match input {{ (first, ..) if true => id(first), (last, ..) => id(last) }}"), true),
        ("variant-rest", format!("enum Wide[T] {{ Wrap(T, Bool, T, Bool, T, Bool, T, Bool)\nEnd }}\n{id}\nfn inspect(input: Wide[i32]) -> i32 = match input {{ Wide::Wrap(first, ..) if true => id(first), Wide::Wrap(last, ..) => id(last), _ => 0 }}"), true),
        ("pattern-prefix-failure", format!("{id}\nfn inspect(input: ((i32, i32), i32)) -> i32 = match input {{ ((first, second), (bad, _)) => id(first), _ => 0 }}"), false),
        ("result", "fn inspect(x: Opt[i32]) -> Opt[i32] = result {\nlet y = x?\nyield x\n}".into(), true),
        ("member-reference", format!("struct Pair {{ field value: i32 }}\n{id}\nfn inspect(x: i32) -> i32 {{\nlet p = Pair {{ value: x }}\nyield id(* &p.value)\n}}"), true),
        ("ordinary-reduce-by", "struct Counter { fn reduce_by(self: &Self, pair: (i32, i32)) -> i32 = pair.0 }\nfn inspect(c: &Counter, x: i32) -> i32 = c.reduce_by((x, x))".into(), true),
        ("method-replay", "struct S { fn choose(self: &Self, left: i32, right: Bool) -> i32 = left }\nfn inspect(s: &S, x: i32, y: Bool) -> i32 = s.choose(right = y, left = x)".into(), true),
        ("overloaded-value", "overload fn target(x: i32) -> i32 = x\noverload fn target(x: Bool) -> Bool = x\nfn inspect(x: i32) -> i32 {\nlet f: Fn(i32) -> i32 = target\nyield f(x)\n}".into(), true),
        ("trait-obligation", "trait Marker {}\nstruct S {}\nimpl Marker for S {}\nfn id[T: Marker](x: T) -> T = x\nfn inspect(x: S) -> S = id(x)".into(), true),
        ("constant", "const VALUE: i32 = 3\nfn inspect() -> i32 = VALUE".into(), true),
        ("invalid-arguments", "fn target(left: i32, right: Bool) -> Unit = ()\nfn inspect(x: i32) -> Unit {\nlet bad = target(right = x, left = true)\nlet next: Bool = x\n}".into(), false),
        ("poisoned-match", "fn inspect(x: i32) -> i32 {\nlet bad: Bool = x\nyield match x { _ => missing, 0 => 0 }\n}".into(), false),
    ];
    for declaration in [
        "fn target(left: i32, right: Bool) -> Unit = ()",
        "overload fn target(left: i32, right: Bool) -> Unit = ()\noverload fn target(left: Bool, right: i32) -> Unit = ()",
    ] {
        sources.push((
            "named-replay",
            format!(
                "{declaration}\nfn inspect(x: i32, y: Bool) -> Unit = target(right = y, left = x)"
            ),
            true,
        ));
    }
    for expression in [
        "-x",
        "1 + x",
        "x + y",
        "((1 + 2) * 3) + x",
        "x + ((1 + 2) * 3)",
    ] {
        sources.push(("checked", format!("{id}\nfn inspect(x: i32, y: i32) -> Unit {{\nlet out = id(checked {{ yield {expression} }})\n}}"), true));
    }
    sources.extend(
        crate::body_types::checker::work::tests::proof_attachment::sources()
            .into_iter()
            .map(|source| ("proof-custody", source, true)),
    );
    sources
}
