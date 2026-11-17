use eglur_hir::{
    ExprKind, FunctionBody, ItemKind, LetSyntax, Mutability, PatternElement, PatternKind, StmtKind,
    TypeRefKind, UnaryOperator,
};
use eglur_syntax::SyntaxKind;

use super::LoweredBody;

#[test]
fn bindings_preserve_syntax_mutability_annotation_and_initializer_independently() {
    for (prefix, syntax, mutability) in [
        ("let", LetSyntax::Let, Mutability::Immutable),
        ("let mut", LetSyntax::Let, Mutability::Mutable),
        ("var", LetSyntax::Var, Mutability::Mutable),
    ] {
        for annotation in ["", ": Marker"] {
            for initializer in ["", " = 401"] {
                let declaration = format!("{prefix} binding{annotation}{initializer}");
                let text = format!("fn sample() -> Unit {{\n{declaration}\nyield ()\n}}\n");
                let source = LoweredBody::new(&text, "sample");
                let statements = source.block(source.root);
                assert_eq!(statements.len(), 2);
                let statement = source.statement(statements[0]);
                source.written(statement.origin(), SyntaxKind::LetStmt, &declaration);
                let StmtKind::Let {
                    syntax: actual_syntax,
                    mutability: actual_mutability,
                    pattern,
                    ty,
                    initializer: value,
                } = statement.kind()
                else {
                    panic!("expected a binding statement");
                };
                assert_eq!(*actual_syntax, syntax);
                assert_eq!(*actual_mutability, mutability);
                source.binding(*pattern, "binding");
                assert_eq!(ty.is_some(), !annotation.is_empty());
                if let Some(ty) = ty {
                    let ty = source.hir.type_ref(*ty);
                    let TypeRefKind::Path { path, arguments } = ty.kind() else {
                        panic!("annotation must retain its path type");
                    };
                    assert_eq!(path.text().as_deref(), Some("Marker"));
                    assert!(arguments.is_empty());
                    source.written(ty.origin(), SyntaxKind::PathType, "Marker");
                }
                assert_eq!(value.is_some(), !initializer.is_empty());
                if let Some(value) = value {
                    source.integer(*value, "401");
                }
                assert!(matches!(
                    source.statement(statements[1]).kind(),
                    StmtKind::Yield(_)
                ));
            }
        }
    }
}

#[test]
fn tuple_binding_keeps_pattern_and_initializer_children_in_source_order() {
    let source = LoweredBody::new(
        "fn sample() -> Unit {\nlet (left, right) = (411, 412)\n}\n",
        "sample",
    );
    let [statement] = source.block(source.root) else {
        panic!("expected one binding");
    };
    source.written(
        source.statement(*statement).origin(),
        SyntaxKind::LetStmt,
        "let (left, right) = (411, 412)",
    );
    let StmtKind::Let {
        pattern,
        initializer: Some(value),
        ..
    } = source.statement(*statement).kind()
    else {
        panic!("expected initialized binding");
    };
    assert_eq!(pattern.body(), source.root.body());
    let pattern = source
        .hir
        .body(source.root.body())
        .pattern(*pattern)
        .unwrap();
    source.written(pattern.origin(), SyntaxKind::TuplePattern, "(left, right)");
    let PatternKind::Tuple(elements) = pattern.kind() else {
        panic!("expected tuple pattern");
    };
    assert_eq!(elements.len(), 2);
    for (element, name) in elements.iter().zip(["left", "right"]) {
        let PatternElement::Pattern(pattern) = element else {
            panic!("tuple should not contain a rest element");
        };
        source.binding(*pattern, name);
    }
    let value = source.expression(*value);
    source.written(value.origin(), SyntaxKind::TupleExpr, "(411, 412)");
    let ExprKind::Tuple(values) = value.kind() else {
        panic!("expected tuple initializer");
    };
    assert_eq!(values.len(), 2);
    source.integer(values[0], "411");
    source.integer(values[1], "412");
}

#[test]
fn assignments_keep_the_target_separate_from_the_value() {
    for (target, kind) in [
        ("_", SyntaxKind::HoleExpr),
        ("slot", SyntaxKind::NameExpr),
        ("holder.value", SyntaxKind::MemberExpr),
        ("*pointer", SyntaxKind::UnaryExpr),
        ("pair.1", SyntaxKind::TupleProjectionExpr),
    ] {
        let assignment = format!("{target} = 501");
        let text = format!("fn sample() -> Unit {{\n{assignment}\n}}\n");
        let source = LoweredBody::new(&text, "sample");
        let [statement] = source.block(source.root) else {
            panic!("expected one assignment");
        };
        let statement = source.statement(*statement);
        source.written(statement.origin(), SyntaxKind::AssignStmt, &assignment);
        let StmtKind::Assign {
            target: place,
            value,
        } = statement.kind()
        else {
            panic!("expected assignment");
        };
        source.integer(*value, "501");
        let target_expression = source.expression(*place);
        source.written(target_expression.origin(), kind, target);
        match (target, target_expression.kind()) {
            ("_", ExprKind::Hole) => {}
            ("slot", ExprKind::Name(_)) => source.name(*place, "slot"),
            ("holder.value", ExprKind::Member { receiver, member }) => {
                source.name(*receiver, "holder");
                source.identifier(member, "value");
            }
            ("*pointer", ExprKind::Unary { operator, operand }) => {
                assert_eq!(*operator, UnaryOperator::Dereference);
                source.name(*operand, "pointer");
            }
            ("pair.1", ExprKind::TupleProjection { receiver, index }) => {
                source.name(*receiver, "pair");
                assert_eq!(index.as_ref(), "1");
            }
            (_, actual) => panic!("wrong target for {target}: {actual:?}"),
        }
    }
}

#[test]
fn nested_and_sibling_loops_keep_their_headers_bodies_and_statement_order() {
    let inner_for = "for element in collection {\ncontinue\n}";
    let while_body = format!("{{\ndestination = 701\n{inner_for}\nbreak\n}}");
    let while_statement = format!("while keep_going {while_body}");
    let outer_for = "for entry in other_collection {\nsink = 702\n}";
    let text = format!("fn sample() -> Unit {{\n{while_statement}\n{outer_for}\nreturn\n}}\n");
    let source = LoweredBody::new(&text, "sample");
    let statements = source.block(source.root);
    assert_eq!(statements.len(), 3);
    let while_node = source.statement(statements[0]);
    source.written(while_node.origin(), SyntaxKind::WhileStmt, &while_statement);
    let StmtKind::While { condition, body } = while_node.kind() else {
        panic!("first statement must remain while");
    };
    source.name(*condition, "keep_going");
    source.written(
        source.expression(*body).origin(),
        SyntaxKind::BlockExpr,
        &while_body,
    );
    let children = source.block(*body);
    assert_eq!(children.len(), 3);
    let StmtKind::Assign { target, value } = source.statement(children[0]).kind() else {
        panic!("first while child must be assignment");
    };
    source.name(*target, "destination");
    source.integer(*value, "701");
    assert!(matches!(
        source.statement(children[2]).kind(),
        StmtKind::Break
    ));
    source.written(
        source.statement(children[2]).origin(),
        SyntaxKind::BreakStmt,
        "break",
    );

    for (statement, binding_name, iterable_name, spelling, nested) in [
        (children[1], "element", "collection", inner_for, true),
        (statements[1], "entry", "other_collection", outer_for, false),
    ] {
        let statement = source.statement(statement);
        source.written(statement.origin(), SyntaxKind::ForStmt, spelling);
        let StmtKind::For {
            binding,
            iterable,
            body,
        } = statement.kind()
        else {
            panic!("expected for statement");
        };
        source.identifier(binding, binding_name);
        source.name(*iterable, iterable_name);
        let [child] = source.block(*body) else {
            panic!("for body should have exactly one statement");
        };
        if nested {
            source.written(
                source.expression(*body).origin(),
                SyntaxKind::BlockExpr,
                "{\ncontinue\n}",
            );
            assert!(matches!(
                source.statement(*child).kind(),
                StmtKind::Continue
            ));
            source.written(
                source.statement(*child).origin(),
                SyntaxKind::ContinueStmt,
                "continue",
            );
        } else {
            source.written(
                source.expression(*body).origin(),
                SyntaxKind::BlockExpr,
                "{\nsink = 702\n}",
            );
            let StmtKind::Assign { target, value } = source.statement(*child).kind() else {
                panic!("outer for body should retain its assignment");
            };
            source.name(*target, "sink");
            source.integer(*value, "702");
        }
    }
    assert!(matches!(
        source.statement(statements[2]).kind(),
        StmtKind::Return(None)
    ));
}

#[test]
fn return_yield_and_expression_statements_keep_distinct_value_channels() {
    for (spelling, kind, expected) in [
        ("return", SyntaxKind::ReturnStmt, None),
        ("return 801", SyntaxKind::ReturnStmt, Some("801")),
        ("yield 802", SyntaxKind::YieldStmt, Some("802")),
        ("803", SyntaxKind::ExprStmt, Some("803")),
    ] {
        let text = format!("fn sample() -> Unit {{\n{spelling}\n}}\n");
        let source = LoweredBody::new(&text, "sample");
        let [id] = source.block(source.root) else {
            panic!("expected one statement");
        };
        let statement = source.statement(*id);
        source.written(statement.origin(), kind, spelling);
        let value = match (kind, statement.kind()) {
            (SyntaxKind::ReturnStmt, StmtKind::Return(value)) => *value,
            (SyntaxKind::YieldStmt, StmtKind::Yield(value))
            | (SyntaxKind::ExprStmt, StmtKind::Expression(value)) => Some(*value),
            (_, actual) => panic!("wrong statement channel: {actual:?}"),
        };
        assert_eq!(value.is_some(), expected.is_some());
        if let (Some(value), Some(spelling)) = (value, expected) {
            source.integer(value, spelling);
        }
    }
}

#[test]
fn sibling_functions_cannot_share_body_local_expression_statement_or_pattern_ids() {
    let text = concat!(
        "fn first() -> Unit {\nlet left = 901\nyield 902\n}\n",
        "fn second() -> Unit {\nlet right = 903\nyield 904\n}\n",
    );
    let mut source = LoweredBody::new(text, "first");
    let first = source.root;
    let second = source
        .hir
        .roots()
        .iter()
        .find_map(|&id| match source.hir.item(id).kind() {
            ItemKind::Function(function) if function.name.text() == Some("second") => {
                let FunctionBody::Block(root) = function.body_form else {
                    panic!("second function must retain its block body");
                };
                assert_eq!(root.body(), function.body);
                Some(root)
            }
            _ => None,
        })
        .unwrap();
    assert_ne!(first.body(), second.body());

    for (root, foreign, binding, initial, result) in [
        (first, second.body(), "left", "901", "902"),
        (second, first.body(), "right", "903", "904"),
    ] {
        source.root = root;
        assert!(source.hir.body(foreign).expression(root).is_none());
        let statements = source.block(source.root);
        assert_eq!(statements.len(), 2);
        let StmtKind::Let {
            pattern,
            initializer: Some(value),
            ..
        } = source.statement(statements[0]).kind()
        else {
            panic!("expected initialized binding");
        };
        source.binding(*pattern, binding);
        source.integer(*value, initial);
        let StmtKind::Yield(value) = source.statement(statements[1]).kind() else {
            panic!("expected final yield");
        };
        source.integer(*value, result);
        assert!(source.hir.body(foreign).statement(statements[0]).is_none());
        assert!(source.hir.body(foreign).pattern(*pattern).is_none());
    }
}
