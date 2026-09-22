use eglur_hir::{
    AxisExprId, AxisExprKind, ExprId, ExprKind, FunctionBody, HirFile, ItemKind, LiteralKind, Name,
    SourceOrigin, TensorSchemaExpr, TensorSchemaFamily, lower,
};
use eglur_source::{FileId, FileSpan, SourceFile};
use eglur_syntax::ast::{AstNode, Expr as SyntaxExpr};
use eglur_syntax::{
    FrontendOutcome, Parse, SyntaxDiagnosticKind, SyntaxElement, SyntaxKind, SyntaxNode, parse,
};

const FAMILIES: [(&str, TensorSchemaFamily); 6] = [
    ("view", TensorSchemaFamily::View),
    ("rearrange", TensorSchemaFamily::Rearrange),
    ("inflate", TensorSchemaFamily::Inflate),
    ("reduce", TensorSchemaFamily::Reduce),
    ("contract", TensorSchemaFamily::Contract),
    ("einop", TensorSchemaFamily::Einop),
];

fn parse_expression(expression: &str) -> Parse {
    let text = format!("fn schema() -> Unit = {expression}\n");
    let source = SourceFile::new(FileId::new(43), text.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parsed) = parse(&source) else {
        panic!("UTF-8 source must reach the parser");
    };
    assert_eq!(parsed.syntax().text(), text);
    parsed
}

struct LoweredSchema {
    parsed: Parse,
    hir: HirFile,
    root: ExprId,
}

impl LoweredSchema {
    fn new(expression: &str) -> Self {
        let parsed = parse_expression(expression);
        assert!(
            parsed.is_success(),
            "{expression}: {:?}",
            parsed.diagnostics()
        );
        let hir = lower(&parsed);
        assert!(hir.invalid_occurrences().is_empty());
        let [item] = hir.roots() else {
            panic!("expected one function")
        };
        let ItemKind::Function(function) = hir.item(*item).kind() else {
            panic!("expected function")
        };
        assert_eq!(function.name.text(), Some("schema"));
        let FunctionBody::Expression(root) = function.body_form else {
            panic!("expected expression body")
        };
        assert_eq!(root.body(), function.body);
        Self { parsed, hir, root }
    }

    fn node(&self, kind: SyntaxKind, text: &str) -> SyntaxNode {
        let mut pending = vec![self.parsed.syntax()];
        let mut found = Vec::new();
        while let Some(node) = pending.pop() {
            pending.extend(node.children());
            if node.kind() == kind && node.text().trim() == text {
                found.push(node);
            }
        }
        assert_eq!(found.len(), 1, "unique {kind:?}: {text}");
        found.pop().unwrap()
    }

    fn schema(&self, id: ExprId, spelling: &str, family: TensorSchemaFamily) -> &TensorSchemaExpr {
        let expression = self.hir.body(id.body()).expression(id).unwrap();
        let node = self.node(SyntaxKind::TensorSchemaExpr, spelling);
        assert_eq!(
            self.hir.origin(expression.origin()),
            SourceOrigin::Written(node.span())
        );
        let facade =
            SyntaxExpr::cast(node.clone()).expect("generic expression facade accepts schemas");
        assert_eq!(facade.syntax().span(), node.span());
        assert_eq!(facade.syntax().text(), node.text());
        let ExprKind::TensorSchema(schema) = expression.kind() else {
            panic!("expected tensor schema")
        };
        assert_eq!(schema.family, family);
        schema
    }

    fn lists(
        &self,
        id: ExprId,
        spelling: &str,
        family: TensorSchemaFamily,
        expected: &[&[&str]; 2],
    ) {
        let schema = self.schema(id, spelling, family);
        let sides: Vec<_> = self
            .node(SyntaxKind::TensorSchemaExpr, spelling)
            .children()
            .filter(|node| node.kind() == SyntaxKind::TransformOperandList)
            .collect();
        assert_eq!(sides.len(), 2);
        for ((lists, syntax_side), spellings) in [&schema.operands, &schema.outputs]
            .into_iter()
            .zip(&sides)
            .zip(expected)
        {
            assert_eq!(lists.len(), spellings.len());
            let syntax_lists: Vec<_> = syntax_side
                .children()
                .filter(|node| node.kind() == SyntaxKind::TransformAxisList)
                .collect();
            assert_eq!(syntax_lists.len(), spellings.len());
            for ((list, syntax), spelling) in lists.iter().zip(syntax_lists).zip(*spellings) {
                assert_eq!(syntax.text().trim(), *spelling);
                assert_eq!(
                    self.hir.origin(list.origin),
                    SourceOrigin::Written(syntax.span())
                );
                let written_axes: Vec<_> = syntax.children().collect();
                assert_eq!(list.axes.len(), written_axes.len());
                for (&axis, written) in list.axes.iter().zip(written_axes) {
                    assert_eq!(axis.body(), id.body());
                    let axis = self.hir.body(id.body()).axis(axis).unwrap();
                    assert_eq!(
                        self.hir.origin(axis.origin()),
                        SourceOrigin::Written(written.span())
                    );
                }
            }
        }
    }

    fn axis(&self, id: AxisExprId, kind: SyntaxKind, text: &str) -> &AxisExprKind {
        assert_eq!(id.body(), self.root.body());
        let axis = self.hir.body(self.root.body()).axis(id).unwrap();
        assert_eq!(
            self.hir.origin(axis.origin()),
            SourceOrigin::Written(self.node(kind, text).span())
        );
        axis.kind()
    }

    fn atom_token(&self, text: &str, kind: SyntaxKind) -> FileSpan {
        let node = self.node(SyntaxKind::AxisAtom, text);
        let mut tokens = node
            .children_with_tokens()
            .filter_map(|element| match element {
                SyntaxElement::Token(token) if token.kind() == kind => Some(token),
                _ => None,
            });
        let token = tokens.next().expect("expected atom token");
        assert!(tokens.next().is_none());
        assert_eq!(token.text(), text);
        token.span()
    }

    fn name(&self, id: AxisExprId, text: &str) {
        let AxisExprKind::Name(name) = self.axis(id, SyntaxKind::AxisAtom, text) else {
            panic!("expected axis name")
        };
        self.name_token(name, text, text, SyntaxKind::Identifier);
    }

    fn name_token(&self, name: &Name, text: &str, token: &str, kind: SyntaxKind) {
        assert_eq!(name.text(), Some(text));
        assert!(!name.is_bang());
        assert_eq!(
            self.hir.origin(name.origin()),
            SourceOrigin::Written(self.atom_token(token, kind))
        );
    }

    fn binary(&self, id: AxisExprId, text: &str, multiply: bool) -> (AxisExprId, AxisExprId) {
        match (self.axis(id, SyntaxKind::AxisBinaryExpr, text), multiply) {
            (AxisExprKind::Add { left, right }, false)
            | (AxisExprKind::Multiply { left, right }, true) => (*left, *right),
            _ => panic!("wrong axis operator for {text}"),
        }
    }

    fn group(&self, id: AxisExprId, text: &str) -> AxisExprId {
        let AxisExprKind::Group(inner) = self.axis(id, SyntaxKind::AxisAtom, text) else {
            panic!("expected axis group")
        };
        *inner
    }
}

#[test]
fn all_families_preserve_distinct_input_and_output_axis_order() {
    for (word, family) in FAMILIES {
        let spelling = format!("{word}{{ left middle -> right last }}");
        let source = LoweredSchema::new(&spelling);
        source.lists(
            source.root,
            &spelling,
            family,
            &[&["left middle"], &["right last"]],
        );
        let schema = source.schema(source.root, &spelling, family);
        assert_eq!(schema.operands[0].axes.len(), 2);
        assert_eq!(schema.outputs[0].axes.len(), 2);
        for (id, text) in schema.operands[0]
            .axes
            .iter()
            .chain(schema.outputs[0].axes.iter())
            .zip(["left", "middle", "right", "last"])
        {
            source.name(*id, text);
        }
    }
}

#[test]
fn multiple_list_families_preserve_list_order_and_empty_neighbors() {
    for (word, family) in FAMILIES
        .into_iter()
        .filter(|(_, family)| *family != TensorSchemaFamily::Reduce)
    {
        let outputs: &[&str] = if family == TensorSchemaFamily::Contract {
            &["outcome tail"]
        } else {
            &["", "outcome tail", "", "final"]
        };
        let spelling = format!(
            "{word}{{ first second, , third, -> {} }}",
            outputs.join(", ")
        );
        let source = LoweredSchema::new(&spelling);
        source.lists(
            source.root,
            &spelling,
            family,
            &[&["first second", "", "third", ""], outputs],
        );
        let schema = source.schema(source.root, &spelling, family);
        assert_eq!(
            schema
                .operands
                .iter()
                .map(|list| list.axes.len())
                .collect::<Vec<_>>(),
            [2, 0, 1, 0]
        );
        source.name(schema.operands[0].axes[0], "first");
        source.name(schema.operands[0].axes[1], "second");
        source.name(schema.operands[2].axes[0], "third");
        let index = usize::from(family != TensorSchemaFamily::Contract);
        source.name(schema.outputs[index].axes[0], "outcome");
        source.name(schema.outputs[index].axes[1], "tail");
        if family != TensorSchemaFamily::Contract {
            assert_eq!(
                schema
                    .outputs
                    .iter()
                    .map(|list| list.axes.len())
                    .collect::<Vec<_>>(),
                [0, 2, 0, 1]
            );
            source.name(schema.outputs[3].axes[0], "final");
        }
    }
}

#[test]
fn an_empty_axis_list_is_present_on_each_side() {
    for (word, family) in FAMILIES {
        for (left, right) in [("", ""), ("input", ""), ("", "output")] {
            let spelling = format!("{word}{{ {left} -> {right} }}");
            let source = LoweredSchema::new(&spelling);
            source.lists(source.root, &spelling, family, &[&[left], &[right]]);
            let schema = source.schema(source.root, &spelling, family);
            assert_eq!(schema.operands[0].axes.len(), usize::from(!left.is_empty()));
            assert_eq!(schema.outputs[0].axes.len(), usize::from(!right.is_empty()));
            if !left.is_empty() {
                source.name(schema.operands[0].axes[0], left);
            }
            if !right.is_empty() {
                source.name(schema.outputs[0].axes[0], right);
            }
        }
    }
}

#[test]
fn shallow_axis_operators_keep_precedence_association_and_grouping() {
    let spelling = "view{ a + b * c + d ... ...batch 23 -> (e + f) * g * h (i * j) }";
    let source = LoweredSchema::new(spelling);
    source.lists(
        source.root,
        spelling,
        TensorSchemaFamily::View,
        &[
            &["a + b * c + d ... ...batch 23"],
            &["(e + f) * g * h (i * j)"],
        ],
    );
    let schema = source.schema(source.root, spelling, TensorSchemaFamily::View);
    let input = &schema.operands[0].axes;
    assert_eq!(input.len(), 4);
    let (sum, d) = source.binary(input[0], "a + b * c + d", false);
    let (a, product) = source.binary(sum, "a + b * c", false);
    let (b, c) = source.binary(product, "b * c", true);
    for (id, name) in [(a, "a"), (b, "b"), (c, "c"), (d, "d")] {
        source.name(id, name);
    }

    assert!(matches!(
        source.axis(input[1], SyntaxKind::AxisAtom, "..."),
        AxisExprKind::Pack(None)
    ));
    let AxisExprKind::Pack(Some(pack)) = source.axis(input[2], SyntaxKind::AxisAtom, "...batch")
    else {
        panic!("expected named pack")
    };
    source.name_token(pack, "batch", "...batch", SyntaxKind::AxisPackNamed);
    let AxisExprKind::Integer(integer) = source.axis(input[3], SyntaxKind::AxisAtom, "23") else {
        panic!("expected integer")
    };
    assert_eq!(integer.kind(), LiteralKind::Integer);
    assert_eq!(integer.spelling(), "23");
    assert_eq!(
        source.hir.origin(integer.origin()),
        SourceOrigin::Written(source.atom_token("23", SyntaxKind::Integer))
    );

    let output = &schema.outputs[0].axes;
    assert_eq!(output.len(), 2);
    let (product, h) = source.binary(output[0], "(e + f) * g * h", true);
    let (group, g) = source.binary(product, "(e + f) * g", true);
    let sum = source.group(group, "(e + f)");
    let (e, f) = source.binary(sum, "e + f", false);
    let product = source.group(output[1], "(i * j)");
    let (i, j) = source.binary(product, "i * j", true);
    for (id, name) in [(e, "e"), (f, "f"), (g, "g"), (h, "h"), (i, "i"), (j, "j")] {
        source.name(id, name);
    }
}

#[test]
fn schema_axes_belong_to_the_enclosing_lambda_not_its_caller() {
    let schema_spelling = "reduce{ source -> target }";
    let source = LoweredSchema::new(&format!("{{ || {schema_spelling} }}"));
    let ExprKind::Lambda(lambda) = source
        .hir
        .body(source.root.body())
        .expression(source.root)
        .unwrap()
        .kind()
    else {
        panic!("expected lambda")
    };
    let tail = lambda.tail.unwrap();
    assert_eq!(tail.body(), lambda.body);
    assert!(
        source
            .hir
            .body(source.root.body())
            .expression(tail)
            .is_none()
    );
    source.lists(
        tail,
        schema_spelling,
        TensorSchemaFamily::Reduce,
        &[&["source"], &["target"]],
    );
    let schema = source.schema(tail, schema_spelling, TensorSchemaFamily::Reduce);
    for (&id, text) in schema.operands[0]
        .axes
        .iter()
        .chain(schema.outputs[0].axes.iter())
        .zip(["source", "target"])
    {
        assert!(source.hir.body(source.root.body()).axis(id).is_none());
        assert_eq!(id.body(), lambda.body);
        let AxisExprKind::Name(name) = source.hir.body(lambda.body).axis(id).unwrap().kind() else {
            panic!("expected axis name")
        };
        source.name_token(name, text, text, SyntaxKind::Identifier);
    }
}

#[test]
fn fixed_cardinality_schemas_reject_extra_lists() {
    for spelling in [
        "reduce{ left, extra -> outcome }",
        "reduce{ source -> first, extra }",
        "contract{ first, second -> outcome, extra }",
    ] {
        let parsed = parse_expression(spelling);
        assert!(!parsed.is_success(), "{spelling}");
        assert!(
            parsed
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.kind() == SyntaxDiagnosticKind::ParseUnexpectedToken)
        );
        assert!(!lower(&parsed).invalid_occurrences().is_empty());
    }
}
