use eglur_hir::{AxisExprKind, ExprKind, HirFile, ItemKind, TensorSchemaFamily, lower};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, parse};

fn lower_text(text: &str, success: bool) -> HirFile {
    let source = SourceFile::new(FileId::new(9), text.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parsed) = parse(&source) else {
        panic!("UTF-8 source must parse")
    };
    assert_eq!(parsed.is_success(), success, "{:?}", parsed.diagnostics());
    lower(&parsed)
}

#[test]
fn long_axis_products_lower_on_the_default_stack() {
    let text = format!(
        "fn schema() -> Unit = view{{ a{} -> a }}\n",
        " * a".repeat(4000)
    );
    let hir = lower_text(&text, true);
    assert!(hir.invalid_occurrences().is_empty());
    let count = hir
        .bodies()
        .flat_map(|(_, body)| body.axes())
        .filter(|(_, axis)| matches!(axis.kind(), AxisExprKind::Multiply { .. }))
        .count();
    assert_eq!(count, 4000);
}

#[test]
fn every_schema_family_preserves_long_input_sums_and_output_products() {
    for (name, family) in [
        ("view", TensorSchemaFamily::View),
        ("rearrange", TensorSchemaFamily::Rearrange),
        ("inflate", TensorSchemaFamily::Inflate),
        ("reduce", TensorSchemaFamily::Reduce),
        ("contract", TensorSchemaFamily::Contract),
        ("einop", TensorSchemaFamily::Einop),
    ] {
        let text = format!(
            "fn schema() -> Unit = {name}{{ a{} -> b{} }}\n",
            " + a".repeat(4000),
            " * b".repeat(4000)
        );
        let hir = lower_text(&text, true);
        assert!(hir.invalid_occurrences().is_empty());
        let mut counts = [0; 2];
        let mut schemas = 0;
        for (_, body) in hir.bodies() {
            for (_, expression) in body.expressions() {
                if let ExprKind::TensorSchema(schema) = expression.kind() {
                    assert_eq!(schema.family, family);
                    assert_eq!(schema.operands.len(), 1);
                    assert_eq!(schema.outputs.len(), 1);
                    schemas += 1;
                }
            }
            for (id, axis) in body.axes() {
                let (left, right) = match axis.kind() {
                    AxisExprKind::Add { left, right } => {
                        counts[0] += 1;
                        (left, right)
                    }
                    AxisExprKind::Multiply { left, right } => {
                        counts[1] += 1;
                        (left, right)
                    }
                    _ => continue,
                };
                assert!(left.local() < right.local() && right.local() < id.local());
                let parent = hir.origin(axis.origin()).source().range();
                for child in [left, right] {
                    let child = body.axis(*child).unwrap();
                    assert!(axis.origin().get() < child.origin().get());
                    let span = hir.origin(child.origin()).source().range();
                    assert!(parent.start() <= span.start() && span.end() <= parent.end());
                }
            }
        }
        assert_eq!(counts, [4000; 2]);
        assert_eq!(schemas, 1);
    }
}

#[test]
fn mixed_grouped_axes_retain_integer_and_pack_meanings() {
    let text = format!(
        "fn schema() -> Unit = rearrange{{ a{}{} -> a }}\n",
        " + (2 * c) + (d * b)".repeat(1000),
        " ...pack ...".repeat(1000)
    );
    let hir = lower_text(&text, true);
    assert!(hir.invalid_occurrences().is_empty());
    let mut counts = [0; 6];
    for (_, body) in hir.bodies() {
        for (id, axis) in body.axes() {
            match axis.kind() {
                AxisExprKind::Add { .. } => counts[0] += 1,
                AxisExprKind::Multiply { .. } => counts[1] += 1,
                AxisExprKind::Group(inner) => {
                    assert!(inner.local() < id.local());
                    counts[2] += 1;
                }
                AxisExprKind::Integer(literal) => {
                    assert_eq!(literal.spelling(), "2");
                    let span = hir.origin(literal.origin()).source().range();
                    assert_eq!(&text[span.start().as_usize()..span.end().as_usize()], "2");
                    counts[3] += 1;
                }
                AxisExprKind::Pack(Some(name)) => {
                    assert_eq!(name.text(), Some("pack"));
                    let span = hir.origin(name.origin()).source().range();
                    assert_eq!(
                        &text[span.start().as_usize()..span.end().as_usize()],
                        "...pack"
                    );
                    counts[4] += 1;
                }
                AxisExprKind::Pack(None) => counts[5] += 1,
                AxisExprKind::Name(_) => {}
                AxisExprKind::Invalid => panic!("valid syntax must retain its axes"),
            }
        }
    }
    assert_eq!(counts, [2000, 2000, 2000, 1000, 1000, 1000]);
}

#[test]
fn malformed_long_axes_keep_shallow_recovery_and_following_items() {
    for suffix in [" *", " +", " + (2 *)"] {
        let source = |count| {
            format!(
                "fn schema() -> Unit = view{{ a{}{suffix} -> a }}\nfn next() -> Unit = ()\n",
                " * a".repeat(count)
            )
        };
        let shallow = lower_text(&source(1), false);
        let deep = lower_text(&source(4000), false);
        let roots = |hir: &HirFile| {
            hir.roots()
                .iter()
                .map(|root| match hir.item(*root).kind() {
                    ItemKind::Function(function) => function.name.text().map(str::to_owned),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(roots(&deep), roots(&shallow));
        assert!(roots(&deep).contains(&Some("next".to_owned())));
        assert!(!deep.invalid_occurrences().is_empty());
        assert!(
            deep.invalid_occurrences()
                .iter()
                .all(|invalid| !invalid.syntax_causes().is_empty())
        );
    }
}
