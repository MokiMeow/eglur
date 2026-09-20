use eglur_hir::{ConstructorPayload, ExprKind, FieldInitializer, Path};
use eglur_syntax::SyntaxKind;

use super::LoweredBody;

fn path(source: &LoweredBody, path: &Path, spelling: &str) {
    assert_eq!(path.text().as_deref(), Some(spelling));
    let names: Vec<_> = spelling.split("::").collect();
    assert_eq!(path.segments().len(), names.len());
    for (name, spelling) in path.segments().iter().zip(names) {
        source.identifier(name, spelling);
    }

    let eglur_hir::SourceOrigin::Written(span) = source.hir.origin(path.origin()) else {
        panic!("path must retain its written origin");
    };
    assert_eq!(span.file(), source.hir.file());
    let text = source.parsed.syntax().text();
    assert_eq!(
        &text[span.range().start().as_usize()..span.range().end().as_usize()],
        spelling,
    );
}

fn field(source: &LoweredBody, field: &FieldInitializer, name: &str, value: &str) {
    source.identifier(&field.name, name);
    source.written(
        field.origin,
        SyntaxKind::FieldInitializer,
        &format!("{name}: {value}"),
    );
}

#[test]
fn nested_tuples_and_arrays_preserve_heterogeneous_child_order() {
    let expression = "(1011, [1012, (1013, 1014), [1015, 1016]], (1017,), 1018)";
    let text = format!("fn sample() -> Unit = {expression}\n");
    let source = LoweredBody::new(&text, "sample");
    let root = source.expression(source.root);
    source.written(root.origin(), SyntaxKind::TupleExpr, expression);
    let ExprKind::Tuple(outer) = root.kind() else {
        panic!("expected outer tuple");
    };
    assert_eq!(outer.len(), 4);
    source.integer(outer[0], "1011");
    source.integer(outer[3], "1018");

    let array = source.expression(outer[1]);
    source.written(
        array.origin(),
        SyntaxKind::ArrayExpr,
        "[1012, (1013, 1014), [1015, 1016]]",
    );
    let ExprKind::Array(array) = array.kind() else {
        panic!("second tuple child must remain an array");
    };
    assert_eq!(array.len(), 3);
    source.integer(array[0], "1012");
    let tuple = source.expression(array[1]);
    source.written(tuple.origin(), SyntaxKind::TupleExpr, "(1013, 1014)");
    let ExprKind::Tuple(tuple) = tuple.kind() else {
        panic!("second array child must remain a tuple");
    };
    assert_eq!(tuple.len(), 2);
    source.integer(tuple[0], "1013");
    source.integer(tuple[1], "1014");

    let nested = source.expression(array[2]);
    source.written(nested.origin(), SyntaxKind::ArrayExpr, "[1015, 1016]");
    let ExprKind::Array(nested) = nested.kind() else {
        panic!("third array child must retain its nested array");
    };
    assert_eq!(nested.len(), 2);
    source.integer(nested[0], "1015");
    source.integer(nested[1], "1016");
    let single = source.expression(outer[2]);
    source.written(single.origin(), SyntaxKind::TupleExpr, "(1017,)");
    let ExprKind::Tuple(single) = single.kind() else {
        panic!("single-element tuple must not become a group");
    };
    assert_eq!(single.len(), 1);
    source.integer(single[0], "1017");
}

#[test]
fn nested_maps_keep_each_entry_key_value_and_source_order() {
    let expression = "([1101: [1102: 1103, 1104: 1105], 1106: 1107], [1108: 1109], [:])";
    let text = format!("fn sample() -> Unit = {expression}\n");
    let source = LoweredBody::new(&text, "sample");
    let ExprKind::Tuple(maps) = source.expression(source.root).kind() else {
        panic!("expected tuple of maps");
    };
    assert_eq!(maps.len(), 3);
    let outer = source.expression(maps[0]);
    source.written(
        outer.origin(),
        SyntaxKind::MapExpr,
        "[1101: [1102: 1103, 1104: 1105], 1106: 1107]",
    );
    let ExprKind::Map(outer) = outer.kind() else {
        panic!("expected first map");
    };
    assert_eq!(outer.len(), 2);
    source.integer(outer[0].key, "1101");
    source.written(
        outer[0].origin,
        SyntaxKind::MapEntry,
        "1101: [1102: 1103, 1104: 1105]",
    );
    source.integer(outer[1].key, "1106");
    source.integer(outer[1].value, "1107");
    source.written(outer[1].origin, SyntaxKind::MapEntry, "1106: 1107");

    let nested = source.expression(outer[0].value);
    source.written(
        nested.origin(),
        SyntaxKind::MapExpr,
        "[1102: 1103, 1104: 1105]",
    );
    let ExprKind::Map(nested) = nested.kind() else {
        panic!("first value must remain a nested map");
    };
    assert_eq!(nested.len(), 2);
    for (entry, (key, value)) in nested.iter().zip([("1102", "1103"), ("1104", "1105")]) {
        source.integer(entry.key, key);
        source.integer(entry.value, value);
        source.written(
            entry.origin,
            SyntaxKind::MapEntry,
            &format!("{key}: {value}"),
        );
    }

    let sibling = source.expression(maps[1]);
    source.written(sibling.origin(), SyntaxKind::MapExpr, "[1108: 1109]");
    let ExprKind::Map(sibling) = sibling.kind() else {
        panic!("expected sibling map");
    };
    assert_eq!(sibling.len(), 1);
    source.integer(sibling[0].key, "1108");
    source.integer(sibling[0].value, "1109");
    source.written(sibling[0].origin, SyntaxKind::MapEntry, "1108: 1109");
    let empty = source.expression(maps[2]);
    source.written(empty.origin(), SyntaxKind::MapExpr, "[:]");
    assert!(matches!(empty.kind(), ExprKind::Map(entries) if entries.is_empty()));
}

#[test]
fn records_and_named_payloads_keep_nested_fields_with_their_owners() {
    let detail = "Detail { code: 1202, limit: 1203 }";
    let choice = format!("Choice::Selected {{ inner: {detail}, active: 1204 }}");
    let expression = format!("Envelope {{ first: 1201, nested: {choice}, last: 1205 }}");
    let text = format!("fn sample() -> Unit = {expression}\n");
    let source = LoweredBody::new(&text, "sample");
    let root = source.expression(source.root);
    source.written(root.origin(), SyntaxKind::StructLiteralExpr, &expression);
    let ExprKind::StructLiteral {
        path: owner,
        fields,
    } = root.kind()
    else {
        panic!("expected outer record");
    };
    path(&source, owner, "Envelope");
    assert_eq!(fields.len(), 3);
    field(&source, &fields[0], "first", "1201");
    source.integer(fields[0].value, "1201");
    field(&source, &fields[1], "nested", &choice);
    field(&source, &fields[2], "last", "1205");
    source.integer(fields[2].value, "1205");

    let variant = source.expression(fields[1].value);
    source.written(
        variant.origin(),
        SyntaxKind::VariantConstructorExpr,
        &choice,
    );
    let ExprKind::VariantConstructor {
        path: owner,
        payload: ConstructorPayload::Named(fields),
    } = variant.kind()
    else {
        panic!("nested field must retain its named variant payload");
    };
    path(&source, owner, "Choice::Selected");
    assert_eq!(fields.len(), 2);
    field(&source, &fields[0], "inner", detail);
    field(&source, &fields[1], "active", "1204");
    source.integer(fields[1].value, "1204");

    let nested = source.expression(fields[0].value);
    source.written(nested.origin(), SyntaxKind::StructLiteralExpr, detail);
    let ExprKind::StructLiteral {
        path: owner,
        fields,
    } = nested.kind()
    else {
        panic!("inner field must retain its record");
    };
    path(&source, owner, "Detail");
    assert_eq!(fields.len(), 2);
    for (entry, (name, value)) in fields.iter().zip([("code", "1202"), ("limit", "1203")]) {
        field(&source, entry, name, value);
        source.integer(entry.value, value);
    }
}

#[test]
fn positional_constructor_payloads_keep_nested_values_and_siblings_in_order() {
    let expression = "Choice::Batch(1301, (1302, 1303), [1304, 1305], Other::Leaf(1306), 1307)";
    let text = format!("fn sample() -> Unit = {expression}\n");
    let source = LoweredBody::new(&text, "sample");
    let root = source.expression(source.root);
    source.written(
        root.origin(),
        SyntaxKind::VariantConstructorExpr,
        expression,
    );
    let ExprKind::VariantConstructor {
        path: owner,
        payload: ConstructorPayload::Tuple(values),
    } = root.kind()
    else {
        panic!("expected positional variant payload");
    };
    path(&source, owner, "Choice::Batch");
    assert_eq!(values.len(), 5);
    source.integer(values[0], "1301");
    source.integer(values[4], "1307");

    let tuple = source.expression(values[1]);
    source.written(tuple.origin(), SyntaxKind::TupleExpr, "(1302, 1303)");
    let ExprKind::Tuple(tuple) = tuple.kind() else {
        panic!("second payload value must remain a tuple");
    };
    assert_eq!(tuple.len(), 2);
    source.integer(tuple[0], "1302");
    source.integer(tuple[1], "1303");
    let array = source.expression(values[2]);
    source.written(array.origin(), SyntaxKind::ArrayExpr, "[1304, 1305]");
    let ExprKind::Array(array) = array.kind() else {
        panic!("third payload value must remain an array");
    };
    assert_eq!(array.len(), 2);
    source.integer(array[0], "1304");
    source.integer(array[1], "1305");

    let nested = source.expression(values[3]);
    source.written(
        nested.origin(),
        SyntaxKind::VariantConstructorExpr,
        "Other::Leaf(1306)",
    );
    let ExprKind::VariantConstructor {
        path: owner,
        payload: ConstructorPayload::Tuple(nested),
    } = nested.kind()
    else {
        panic!("fourth payload value must remain a variant constructor");
    };
    path(&source, owner, "Other::Leaf");
    assert_eq!(nested.len(), 1);
    source.integer(nested[0], "1306");
}

#[test]
fn empty_aggregates_and_constructor_payload_presence_remain_distinct() {
    for (expression, kind) in [
        ("()", SyntaxKind::UnitExpr),
        ("[]", SyntaxKind::ArrayExpr),
        ("[:]", SyntaxKind::MapExpr),
        ("Record {}", SyntaxKind::StructLiteralExpr),
        ("Choice::Absent", SyntaxKind::VariantConstructorExpr),
        ("Choice::Empty()", SyntaxKind::VariantConstructorExpr),
        ("Choice::Named {}", SyntaxKind::VariantConstructorExpr),
    ] {
        let text = format!("fn sample() -> Unit = {expression}\n");
        let source = LoweredBody::new(&text, "sample");
        let root = source.expression(source.root);
        source.written(root.origin(), kind, expression);
        match (expression, root.kind()) {
            ("()", ExprKind::Unit) => {}
            ("[]", ExprKind::Array(values)) => assert!(values.is_empty()),
            ("[:]", ExprKind::Map(entries)) => assert!(entries.is_empty()),
            (
                "Record {}",
                ExprKind::StructLiteral {
                    path: owner,
                    fields,
                },
            ) => {
                path(&source, owner, "Record");
                assert!(fields.is_empty());
            }
            (
                "Choice::Absent",
                ExprKind::VariantConstructor {
                    path: owner,
                    payload: ConstructorPayload::Absent,
                },
            ) => {
                path(&source, owner, "Choice::Absent");
            }
            (
                "Choice::Empty()",
                ExprKind::VariantConstructor {
                    path: owner,
                    payload: ConstructorPayload::Tuple(values),
                },
            ) => {
                path(&source, owner, "Choice::Empty");
                assert!(values.is_empty());
            }
            (
                "Choice::Named {}",
                ExprKind::VariantConstructor {
                    path: owner,
                    payload: ConstructorPayload::Named(fields),
                },
            ) => {
                path(&source, owner, "Choice::Named");
                assert!(fields.is_empty());
            }
            (_, actual) => panic!("wrong empty form for {expression}: {actual:?}"),
        }
    }
}
