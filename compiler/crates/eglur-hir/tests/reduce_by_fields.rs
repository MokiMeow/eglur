use eglur_hir::{
    CallArgument, CallSyntax, DesugaringKind, Expr, ExprId, ExprKind, FunctionBody, HirFile,
    ItemKind, Name, OriginId, ReduceByCandidateExpr, ReduceByOrdinaryCandidate,
    ReduceBySingleValue, SourceOrigin, TensorSchemaFamily, lower,
};
use eglur_source::{ByteOffset, ByteRange, FileId, FileSpan, SourceFile};
use eglur_syntax::{FrontendOutcome, Parse, SyntaxElement, SyntaxKind, parse};

struct LoweredReduceBy {
    parsed: Parse,
    hir: HirFile,
    root: ExprId,
    spelling: String,
    arguments_span: FileSpan,
}

impl LoweredReduceBy {
    fn new(receiver: &str, arguments: &str) -> Self {
        let spelling = format!("{receiver}.reduce_by({arguments})");
        let text = format!("fn inspect() -> Unit = {spelling}\n");
        let source = SourceFile::new(FileId::new(47), text.as_bytes().to_vec());
        let FrontendOutcome::Parsed(parsed) = parse(&source) else {
            panic!("expected UTF-8 source")
        };
        assert_eq!(parsed.syntax().text(), text);
        assert!(
            parsed.is_success(),
            "{spelling}: {:?}",
            parsed.diagnostics()
        );
        let hir = lower(&parsed);
        assert!(hir.invalid_occurrences().is_empty());
        let [item] = hir.roots() else {
            panic!("expected one root")
        };
        let ItemKind::Function(function) = hir.item(*item).kind() else {
            panic!("expected function")
        };
        assert_eq!(function.name.text(), Some("inspect"));
        let FunctionBody::Expression(root) = function.body_form else {
            panic!("expected expression body")
        };
        assert_eq!(root.body(), function.body);

        let prefix = format!("fn inspect() -> Unit = {receiver}.reduce_by");
        let start = ByteOffset::new(u32::try_from(prefix.len()).unwrap());
        let end = ByteOffset::new(u32::try_from(prefix.len() + arguments.len() + 2).unwrap());
        let arguments_span = FileSpan::new(source.id(), ByteRange::new(start, end).unwrap());
        Self {
            parsed,
            hir,
            root,
            spelling,
            arguments_span,
        }
    }

    fn span(&self, kind: SyntaxKind, spelling: &str) -> FileSpan {
        let mut pending = vec![self.parsed.syntax()];
        let mut found = Vec::new();
        while let Some(node) = pending.pop() {
            if node.kind() == kind && node.text().trim() == spelling {
                found.push(node.span());
            }
            for element in node.children_with_tokens() {
                match element {
                    SyntaxElement::Node(child) => pending.push(child),
                    SyntaxElement::Token(token)
                        if token.kind() == kind && token.text() == spelling =>
                    {
                        found.push(token.span())
                    }
                    SyntaxElement::Token(_) => {}
                }
            }
        }
        assert_eq!(found.len(), 1, "unique {kind:?}: {spelling}");
        found[0]
    }

    fn written(&self, origin: OriginId, kind: SyntaxKind, spelling: &str) {
        assert_eq!(
            self.hir.origin(origin),
            SourceOrigin::Written(self.span(kind, spelling))
        );
    }

    fn derived(&self, origin: OriginId, kind: DesugaringKind) {
        assert_eq!(
            self.hir.origin(origin),
            SourceOrigin::Derived {
                source: self.span(SyntaxKind::ReduceByCandidateExpr, &self.spelling),
                kind,
            }
        );
    }

    fn expression(&self, id: ExprId) -> &Expr {
        assert_eq!(id.body(), self.root.body());
        self.hir.body(self.root.body()).expression(id).unwrap()
    }

    fn candidate(&self) -> &ReduceByCandidateExpr {
        let expression = self.expression(self.root);
        self.written(
            expression.origin(),
            SyntaxKind::ReduceByCandidateExpr,
            &self.spelling,
        );
        let ExprKind::ReduceByCandidate(candidate) = expression.kind() else {
            panic!("expected unresolved candidates")
        };
        assert_eq!(
            self.hir.origin(candidate.arguments_origin),
            SourceOrigin::Written(self.arguments_span)
        );
        self.expression(candidate.receiver);
        candidate
    }

    fn identifier(&self, name: &Name, text: &str, bang: bool) {
        assert_eq!(name.text(), Some(text));
        assert_eq!(name.is_bang(), bang);
        if bang {
            self.written(
                name.origin(),
                SyntaxKind::BangIdentifier,
                &format!("{text}!"),
            );
        } else {
            self.written(name.origin(), SyntaxKind::Identifier, text);
        }
    }

    fn name(&self, id: ExprId, spelling: &str) {
        let expression = self.expression(id);
        self.written(expression.origin(), SyntaxKind::NameExpr, spelling);
        let ExprKind::Name(path) = expression.kind() else {
            panic!("expected name {spelling}")
        };
        let [name] = path.segments() else {
            panic!("expected unqualified name")
        };
        self.identifier(
            name,
            spelling.trim_end_matches('!'),
            spelling.ends_with('!'),
        );
        assert_eq!(
            self.hir.origin(path.origin()),
            self.hir.origin(name.origin())
        );
    }

    fn ordinary<'a>(&self, candidate: &'a ReduceByCandidateExpr) -> &'a ReduceByOrdinaryCandidate {
        let ordinary = candidate
            .ordinary
            .as_ref()
            .expect("ordinary candidate must survive");
        self.derived(ordinary.origin, DesugaringKind::ReduceByOrdinaryCandidate);
        assert_eq!(ordinary.call.syntax, CallSyntax::Direct);
        assert!(ordinary.call.type_arguments.is_empty());
        assert!(!ordinary.call.partial_application);
        let callee = self.expression(ordinary.call.callee);
        self.derived(callee.origin(), DesugaringKind::ReduceByOrdinaryCandidate);
        let ExprKind::Member { receiver, member } = callee.kind() else {
            panic!("ordinary callee must be a member")
        };
        assert_eq!(*receiver, candidate.receiver);
        self.identifier(member, "reduce_by", false);
        ordinary
    }

    fn argument(
        &self,
        argument: &CallArgument,
        spelling: &str,
        kind: SyntaxKind,
        name: Option<&str>,
    ) -> ExprId {
        self.written(argument.origin, kind, spelling);
        match (&argument.name, name) {
            (None, None) => {}
            (Some(actual), Some(expected)) => self.identifier(actual, expected, false),
            _ => panic!("wrong argument name for {spelling}"),
        }
        self.expression(argument.value);
        argument.value
    }

    fn phases(&self, candidate: &ReduceByCandidateExpr, expected: &[(&str, &[&str], &str)]) {
        let phases = candidate
            .ordered_phases
            .as_ref()
            .expect("phase interpretation must survive");
        self.derived(
            phases.origin,
            DesugaringKind::ReduceByOrderedPhasesCandidate,
        );
        assert_eq!(phases.phases.len(), expected.len());
        if let Some(ordinary) = &candidate.ordinary {
            assert_eq!(ordinary.call.arguments.len(), expected.len());
            assert_eq!(ordinary.candidate_names.len(), expected.len());
        }
        for (index, (phase, (spelling, names, reducer))) in
            phases.phases.iter().zip(expected).enumerate()
        {
            self.written(phase.origin, SyntaxKind::ReduceByPhase, spelling);
            assert_eq!(phase.axes.len(), names.len());
            for (name, expected) in phase.axes.iter().zip(*names) {
                self.identifier(name, expected, false);
            }
            self.name(phase.reducer, reducer);

            if let Some(ordinary) = &candidate.ordinary {
                assert_eq!(phase.axes.len(), 1);
                let argument = &ordinary.call.arguments[index];
                assert!(argument.name.is_none());
                assert_eq!(
                    self.hir.origin(argument.origin),
                    self.hir.origin(phase.origin)
                );
                let tuple = self.expression(argument.value);
                assert_eq!(
                    self.hir.origin(tuple.origin()),
                    self.hir.origin(phase.origin)
                );
                let ExprKind::Tuple(pair) = tuple.kind() else {
                    panic!("ordinary interpretation must retain the tuple")
                };
                assert_eq!(pair.len(), 2);
                assert_eq!(pair[0], ordinary.candidate_names[index]);
                assert_eq!(pair[1], phase.reducer);
                let axis = self.expression(pair[0]);
                assert_eq!(
                    self.hir.origin(axis.origin()),
                    self.hir.origin(phase.axes[0].origin())
                );
                let ExprKind::Name(path) = axis.kind() else {
                    panic!("tuple first element must be its candidate name")
                };
                let [name] = path.segments() else {
                    panic!("expected one candidate name")
                };
                self.identifier(name, names[0], false);
                assert_eq!(
                    self.hir.origin(path.origin()),
                    self.hir.origin(phase.axes[0].origin())
                );
            }
        }
    }
}

#[test]
fn candidate_presence_matches_the_finite_source_matrix() {
    for (arguments, expected) in [
        ("", [true, false, false]),
        ("select", [true, true, false]),
        ("build(31)", [true, true, false]),
        ("reducer = select", [true, false, false]),
        ("(axis, first), second, mode = third", [true, false, false]),
        ("(height, total)", [true, true, true]),
        ("(width, count), (depth, product),", [true, false, true]),
        ("((row column), combine)", [false, false, true]),
        (
            "((row column), combine), (batch, finish)",
            [false, false, true],
        ),
        ("((axis), select)", [true, true, false]),
    ] {
        let source = LoweredReduceBy::new("subject", arguments);
        let candidate = source.candidate();
        source.name(candidate.receiver, "subject");
        assert_eq!(
            [
                candidate.ordinary.is_some(),
                candidate.single_reducer.is_some(),
                candidate.ordered_phases.is_some()
            ],
            expected,
            "{arguments}"
        );
        if candidate.ordinary.is_some() {
            source.ordinary(candidate);
        }
        if let Some(single) = &candidate.single_reducer {
            source.derived(
                single.origin,
                DesugaringKind::ReduceBySingleReducerCandidate,
            );
        }
        if let Some(phases) = &candidate.ordered_phases {
            source.derived(
                phases.origin,
                DesugaringKind::ReduceByOrderedPhasesCandidate,
            );
        }
    }
}

#[test]
fn empty_arguments_keep_a_present_empty_ordinary_candidate() {
    let source = LoweredReduceBy::new("subject", "");
    let candidate = source.candidate();
    source.name(candidate.receiver, "subject");
    let ordinary = source.ordinary(candidate);
    assert!(ordinary.call.arguments.is_empty());
    assert!(ordinary.candidate_names.is_empty());
    assert!(candidate.single_reducer.is_none());
    assert!(candidate.ordered_phases.is_none());
}

#[test]
fn single_expression_candidates_share_the_complete_ordinary_argument() {
    for arguments in ["select!", "build(31)"] {
        let source = LoweredReduceBy::new("subject", arguments);
        let candidate = source.candidate();
        let ordinary = source.ordinary(candidate);
        assert!(ordinary.candidate_names.is_empty());
        assert_eq!(ordinary.call.arguments.len(), 1);
        assert!(candidate.ordered_phases.is_none());
        let single = candidate.single_reducer.as_ref().unwrap();
        source.derived(
            single.origin,
            DesugaringKind::ReduceBySingleReducerCandidate,
        );
        let ReduceBySingleValue::Expression(value) = single.value else {
            panic!("expected expression candidate")
        };
        assert_eq!(value, ordinary.call.arguments[0].value);
        let kind = if arguments == "select!" {
            SyntaxKind::NameExpr
        } else {
            SyntaxKind::CallExpr
        };
        source.argument(&ordinary.call.arguments[0], arguments, kind, None);
        if arguments == "select!" {
            source.name(value, arguments);
        } else {
            let expression = source.expression(value);
            source.written(expression.origin(), SyntaxKind::CallExpr, arguments);
            let ExprKind::Call(call) = expression.kind() else {
                panic!("expected complete reducer call")
            };
            source.name(call.callee, "build");
            assert_eq!(call.arguments.len(), 1);
            let argument = source.argument(&call.arguments[0], "31", SyntaxKind::LiteralExpr, None);
            let ExprKind::Literal(literal) = source.expression(argument).kind() else {
                panic!("expected nested argument")
            };
            assert_eq!(literal.spelling(), "31");
        }
    }
}

#[test]
fn named_and_mixed_ordinary_arguments_keep_names_values_and_tuple_roles() {
    let source = LoweredReduceBy::new("subject", "reducer = select, mode = policy");
    let candidate = source.candidate();
    assert!(candidate.single_reducer.is_none());
    assert!(candidate.ordered_phases.is_none());
    let ordinary = source.ordinary(candidate);
    assert!(ordinary.candidate_names.is_empty());
    assert_eq!(ordinary.call.arguments.len(), 2);
    for (argument, name, value) in [
        (&ordinary.call.arguments[0], "reducer", "select"),
        (&ordinary.call.arguments[1], "mode", "policy"),
    ] {
        let id = source.argument(
            argument,
            &format!("{name} = {value}"),
            SyntaxKind::NamedArgument,
            Some(name),
        );
        source.name(id, value);
    }

    let source = LoweredReduceBy::new("subject", "(axis, head), middle, option = tail");
    let candidate = source.candidate();
    assert!(candidate.single_reducer.is_none());
    assert!(candidate.ordered_phases.is_none());
    let ordinary = source.ordinary(candidate);
    assert!(ordinary.candidate_names.is_empty());
    assert_eq!(ordinary.call.arguments.len(), 3);
    let tuple = source.argument(
        &ordinary.call.arguments[0],
        "(axis, head)",
        SyntaxKind::TupleExpr,
        None,
    );
    let expression = source.expression(tuple);
    source.written(expression.origin(), SyntaxKind::TupleExpr, "(axis, head)");
    let ExprKind::Tuple(elements) = expression.kind() else {
        panic!("expected ordinary tuple")
    };
    assert_eq!(elements.len(), 2);
    source.name(elements[0], "axis");
    source.name(elements[1], "head");
    let middle = source.argument(
        &ordinary.call.arguments[1],
        "middle",
        SyntaxKind::NameExpr,
        None,
    );
    source.name(middle, "middle");
    let tail = source.argument(
        &ordinary.call.arguments[2],
        "option = tail",
        SyntaxKind::NamedArgument,
        Some("option"),
    );
    source.name(tail, "tail");
}

#[test]
fn singleton_phase_pair_shares_axis_reducer_and_origins_across_candidates() {
    let source = LoweredReduceBy::new("subject", "(height, total!)");
    let candidate = source.candidate();
    source.ordinary(candidate);
    source.phases(candidate, &[("(height, total!)", &["height"], "total!")]);
    let single = candidate.single_reducer.as_ref().unwrap();
    source.derived(
        single.origin,
        DesugaringKind::ReduceBySingleReducerCandidate,
    );
    let ReduceBySingleValue::PhasePair {
        axis,
        reducer,
        origin,
    } = &single.value
    else {
        panic!("expected phase-pair alternative")
    };
    let phase = &candidate.ordered_phases.as_ref().unwrap().phases[0];
    source.identifier(axis, "height", false);
    assert_eq!(*reducer, phase.reducer);
    assert_eq!(source.hir.origin(*origin), source.hir.origin(phase.origin));
}

#[test]
fn multiple_singleton_phases_keep_order_and_ordinary_tuple_correspondence() {
    let source = LoweredReduceBy::new("subject", "\n(width, count),\n(depth, product!),\n");
    let candidate = source.candidate();
    source.ordinary(candidate);
    assert!(candidate.single_reducer.is_none());
    source.phases(
        candidate,
        &[
            ("(width, count)", &["width"], "count"),
            ("(depth, product!)", &["depth"], "product!"),
        ],
    );
}

#[test]
fn grouped_axes_keep_only_ordered_phases_without_an_ordinary_fallback() {
    for arguments in [
        "((row column), combine)",
        "((row column), combine), (batch, finish!)",
    ] {
        let source = LoweredReduceBy::new("subject", arguments);
        let candidate = source.candidate();
        assert!(candidate.ordinary.is_none());
        assert!(candidate.single_reducer.is_none());
        let mut expected: Vec<(&str, &[&str], &str)> =
            vec![("((row column), combine)", &["row", "column"], "combine")];
        if arguments.contains("finish") {
            expected.push(("(batch, finish!)", &["batch"], "finish!"));
        }
        source.phases(candidate, &expected);
    }
}

#[test]
fn a_grouped_single_name_remains_an_ordinary_tuple_not_phase_axes() {
    let source = LoweredReduceBy::new("subject", "((axis), select)");
    let candidate = source.candidate();
    let ordinary = source.ordinary(candidate);
    assert!(ordinary.candidate_names.is_empty());
    assert!(candidate.ordered_phases.is_none());
    assert_eq!(ordinary.call.arguments.len(), 1);
    let tuple = source.argument(
        &ordinary.call.arguments[0],
        "((axis), select)",
        SyntaxKind::TupleExpr,
        None,
    );
    let ReduceBySingleValue::Expression(shared) = candidate.single_reducer.as_ref().unwrap().value
    else {
        panic!("expected expression candidate")
    };
    assert_eq!(shared, tuple);
    let ExprKind::Tuple(elements) = source.expression(tuple).kind() else {
        panic!("expected tuple")
    };
    assert_eq!(elements.len(), 2);
    let group = source.expression(elements[0]);
    source.written(group.origin(), SyntaxKind::GroupExpr, "(axis)");
    let ExprKind::Group(inner) = group.kind() else {
        panic!("expected explicit name grouping")
    };
    source.name(*inner, "axis");
    source.name(elements[1], "select");
}

#[test]
fn a_schema_receiver_does_not_discard_other_syntactic_candidates() {
    let source = LoweredReduceBy::new("reduce{ input -> }", "(height, total)");
    let candidate = source.candidate();
    let receiver = source.expression(candidate.receiver);
    source.written(
        receiver.origin(),
        SyntaxKind::TensorSchemaExpr,
        "reduce{ input -> }",
    );
    let ExprKind::TensorSchema(schema) = receiver.kind() else {
        panic!("expected retained schema receiver")
    };
    assert_eq!(schema.family, TensorSchemaFamily::Reduce);
    source.ordinary(candidate);
    assert!(matches!(
        candidate.single_reducer.as_ref().unwrap().value,
        ReduceBySingleValue::PhasePair { .. }
    ));
    source.phases(candidate, &[("(height, total)", &["height"], "total")]);
}
