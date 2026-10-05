use super::*;

#[test]
fn wide_generic_interfaces_preserve_contexts_under_reordering() {
    for public in [false, true] {
        for count in [8, 32, 128] {
            let visibility = if public { "pub " } else { "" };
            let parameters = (0..count)
                .map(|index| format!("T{index:03}"))
                .collect::<Vec<_>>()
                .join(", ");
            let wide = format!("{visibility}fn wide[{parameters}](value: Bool) -> Bool = value\n");
            let marker = "pub fn marker() -> Bool = true\n";
            let canonical = resolve(
                [file("main.eglur", &format!("{wide}{marker}"))],
                "main.eglur",
                false,
            );
            let reordered = resolve(
                [file("main.eglur", &format!("{marker}{wide}"))],
                "main.eglur",
                false,
            );
            assert_eq!(canonical.semantics, reordered.semantics);
        }
    }
}

#[test]
fn generic_defaults_and_reexports_keep_contexts_under_file_reordering() {
    let source = |reverse: bool| {
        let mut declarations = [
            "pub trait Echo[A] { fn echo[B](self: &Self, value: B) -> B = value }\n",
            concat!(
                "pub struct Wrap[X, Y] {\n",
                "  pub field stored: X\n",
                "  pub fn pass[C](self: &Self, value: C) -> C = value\n",
                "}\n",
            ),
            "impl[X, Y] Echo[X] for Wrap[X, Y] {}\n",
        ];
        if reverse {
            declarations.reverse();
        }
        declarations.concat()
    };
    let files = |reverse: bool| {
        let mut files = [
            file("dep.eglur", &source(reverse)),
            file(
                "bridge.eglur",
                "pub import dep::Wrap\npub import dep::Echo\n",
            ),
            file(
                "app.eglur",
                concat!(
                    "import bridge::Wrap\nimport bridge::Echo\n",
                    "pub fn call(value: Wrap[Bool, Str], item: Bool) -> Bool = value.echo(value.pass(item))\n",
                ),
            ),
        ];
        if reverse {
            files.reverse();
        }
        files
    };
    let canonical = resolve(files(false), "app.eglur", false);
    let reordered = resolve(files(true), "app.eglur", false);

    assert_eq!(canonical.semantics, reordered.semantics);
    assert_eq!(canonical.semantics.conformance().components().len(), 1);
}

#[test]
fn generic_reexports_retain_bound_metadata() {
    let files = |bound: &str| {
        [
            file(
                "dep.eglur",
                &format!(
                    concat!(
                        "pub trait Alpha {{}}\npub trait Beta {{}}\n",
                        "pub struct Holder[A] {{\n",
                        "  pub fn keep[B: {}](self: &Self, value: B) -> B = value\n",
                        "}}\n",
                    ),
                    bound,
                ),
            ),
            file("bridge.eglur", "pub import dep::Holder\n"),
            file(
                "app.eglur",
                "import bridge::Holder\npub fn marker() -> Unit = ()\n",
            ),
        ]
    };
    let before = resolve(files("Alpha"), "app.eglur", false);
    let after = resolve(files("Beta"), "app.eglur", false);

    assert_ne!(
        outputs(&before, "dep").interface(),
        outputs(&after, "dep").interface()
    );
    assert_ne!(
        outputs(&before, "bridge").interface(),
        outputs(&after, "bridge").interface()
    );
    assert_ne!(unit_key(&before, "app"), unit_key(&after, "app"));
}
