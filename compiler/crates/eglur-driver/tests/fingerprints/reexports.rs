use super::*;

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
