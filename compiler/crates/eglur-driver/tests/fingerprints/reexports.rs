use super::*;

#[test]
fn primitive_shadowing_changes_interface_and_body_semantic_identity() {
    let files = |shadow: bool| {
        vec![file(
            "main.eglur",
            &format!(
                "{}pub fn identity(value: i32) -> i32 = value\n",
                if shadow { "type i32 = Bool\n" } else { "" }
            ),
        )]
    };
    let primitive = resolve(files(false), "main.eglur", false);
    let shadowed = resolve(files(true), "main.eglur", false);
    let explicit = resolve(
        [file(
            "main.eglur",
            "type i32 = Bool\npub fn identity(value: Bool) -> Bool = value\n",
        )],
        "main.eglur",
        false,
    );
    assert_eq!(
        outputs(&shadowed, "main").interface(),
        outputs(&explicit, "main").interface()
    );
    assert_ne!(
        outputs(&primitive, "main").interface(),
        outputs(&shadowed, "main").interface()
    );
    assert_ne!(
        outputs(&primitive, "main").body(),
        outputs(&shadowed, "main").body()
    );
}

#[test]
fn primitive_reexports_fingerprint_identity_not_local_import_positions() {
    let files = |ty: &str, moved: bool| {
        vec![
            file(
                "dep.eglur",
                &format!(
                    "{}pub import std::prelude::{ty} as Scalar\n",
                    if moved { "const unused: i32 = 0\n" } else { "" }
                ),
            ),
            file("bridge.eglur", "pub import dep::Scalar\n"),
            file("app.eglur", "pub import bridge::Scalar as Number\n"),
        ]
    };
    let canonical = resolve(files("i32", false), "app.eglur", false);
    let mut reordered = files("i32", true);
    reordered.reverse();
    let moved = resolve(reordered, "app.eglur", true);
    let changed = resolve(files("Bool", false), "app.eglur", false);
    for name in ["dep", "bridge", "app"] {
        assert_eq!(
            outputs(&canonical, name).interface(),
            outputs(&moved, name).interface()
        );
        assert_ne!(
            outputs(&canonical, name).interface(),
            outputs(&changed, name).interface()
        );
    }
}

#[test]
fn wide_reexported_families_are_allocation_order_independent() {
    for count in [8, 32, 128] {
        let files = |reverse: bool| {
            let mut methods = (0..count)
                .map(|index| {
                    format!("pub fn method_{index:03}[T](self: &Self, value: T) -> T = value\n")
                })
                .collect::<Vec<_>>();
            if reverse {
                methods.reverse();
            }
            let unrelated = "pub fn unrelated() -> Bool = true\n";
            let owner = format!("pub struct Host[A] {{\n{}}}\n", methods.concat());
            let source = if reverse {
                format!("{unrelated}{owner}")
            } else {
                format!("{owner}{unrelated}")
            };
            let mut files = [
                file("dep.eglur", &source),
                file("bridge.eglur", "pub import dep::Host\n"),
                file(
                    "app.eglur",
                    if reverse {
                        "pub import bridge::Host as Right\npub import bridge::Host as Left\n"
                    } else {
                        "pub import bridge::Host as Left\npub import bridge::Host as Right\n"
                    },
                ),
            ];
            if reverse {
                files.reverse();
            }
            files
        };
        let canonical = resolve(files(false), "app.eglur", false);
        let reordered = resolve(files(true), "app.eglur", true);
        assert_eq!(canonical.semantics, reordered.semantics);
    }
}

#[test]
fn reexports_preserve_nested_trait_enum_and_property_contracts() {
    for (name, template) in [
        (
            "Provider",
            "pub trait Provider { type Item\nfn value(self: &Self) -> TYPE }\n",
        ),
        ("Event", "pub enum Event { Named { value: TYPE } }\n"),
        (
            "Host",
            concat!(
                "pub struct Host {\n",
                "pub field value: TYPE\n",
                "pub prop exposed: TYPE {\n",
                "fn get_exposed(self: &Self) -> TYPE = self.value\n",
                "}\n}\n",
            ),
        ),
    ] {
        let files = |ty| {
            [
                file("dep.eglur", &template.replace("TYPE", ty)),
                file("bridge.eglur", &format!("pub import dep::{name}\n")),
                file("app.eglur", &format!("pub import bridge::{name}\n")),
            ]
        };
        let before = resolve(files("Bool"), "app.eglur", false);
        let after = resolve(files("Str"), "app.eglur", false);
        for module in ["dep", "bridge", "app"] {
            assert_ne!(
                outputs(&before, module).interface(),
                outputs(&after, module).interface()
            );
            assert_eq!(
                outputs(&before, module).effect(),
                outputs(&after, module).effect()
            );
        }
    }
}

#[test]
fn private_members_do_not_change_reexported_contracts() {
    let files = |ty| {
        [
            file(
                "dep.eglur",
                &format!(
                    "pub struct Host {{\n\
                     fn hidden(self: &Self, value: {ty}) -> {ty} = value\n\
                     pub fn exposed(self: &Self) -> Bool = true\n}}\n"
                ),
            ),
            file("bridge.eglur", "pub import dep::Host\n"),
            file("app.eglur", "pub import bridge::Host\n"),
        ]
    };
    let before = resolve(files("Bool"), "app.eglur", false);
    let after = resolve(files("Str"), "app.eglur", false);
    assert_ne!(
        outputs(&before, "dep").body(),
        outputs(&after, "dep").body()
    );
    for module in ["dep", "bridge", "app"] {
        assert_eq!(
            outputs(&before, module).interface(),
            outputs(&after, module).interface()
        );
        assert_eq!(
            outputs(&before, module).effect(),
            outputs(&after, module).effect()
        );
    }
}
