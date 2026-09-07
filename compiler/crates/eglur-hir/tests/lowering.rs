use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use eglur_hir::{
    AttributeValue, AxisExprKind, CallSyntax, ConstructorPayload, DesugaringKind,
    EnumVariantPayload, ExprKind, FunctionBody, HirFile, ImportSource, InvalidCategory, ItemKind,
    LambdaSyntax, PatternElement, PatternKind, ReduceBySingleValue, SourceOrigin, StmtKind,
    StorageFieldSyntax, TensorSchemaFamily, TypeRefKind, lower,
};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, Parse, parse};
use serde::Deserialize;

#[derive(Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    entrypoint: String,
    source: CaseSource,
    expectation: Expectation,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CaseSource {
    Utf8 {
        text: String,
    },
    Bytes {
        #[serde(rename = "hex")]
        _hex: String,
    },
}

#[derive(Deserialize)]
struct Expectation {
    kind: String,
}

fn parse_text(file: u32, text: &str) -> Parse {
    let source = SourceFile::new(FileId::new(file), text.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parse) = parse(&source) else {
        panic!("UTF-8 test source should reach the parser");
    };
    parse
}

fn corpus() -> Corpus {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../tests/frontend/expectations.json");
    serde_json::from_slice(&fs::read(path).expect("front-end corpus should be readable"))
        .expect("front-end corpus should retain its projected schema")
}

#[test]
fn empty_match_lowers_without_synthesizing_arms() {
    let parse = parse_text(
        u32::MAX,
        "fn absurd(value: Empty) -> Never = match value {}\n",
    );
    assert!(parse.is_success(), "{:?}", parse.diagnostics());

    let hir = lower(&parse);
    assert!(hir.invalid_occurrences().is_empty());
    let arms = hir
        .bodies()
        .flat_map(|(_, body)| body.expressions())
        .find_map(|(_, expression)| match expression.kind() {
            ExprKind::Match(match_expression) => Some(&match_expression.arms),
            _ => None,
        })
        .expect("source should lower one match expression");
    assert!(arms.is_empty());
}

fn observe_hir_taxonomy(hir: &HirFile, observed: &mut BTreeSet<&'static str>) {
    if hir.module().is_some() {
        observed.insert("module");
    }

    for (_, item) in hir.items() {
        let tag = match item.kind() {
            ItemKind::Import(import) => {
                observed.insert(match import.source {
                    ImportSource::Single { .. } => "import.single",
                    ImportSource::Group { .. } => "import.group",
                });
                "item.import"
            }
            ItemKind::Function(function) => {
                observed.insert(match function.body_form {
                    FunctionBody::Absent => "function_body.absent",
                    FunctionBody::Expression(_) => "function_body.expression",
                    FunctionBody::Block(_) => "function_body.block",
                });
                "item.function"
            }
            ItemKind::Extern(_) => "item.extern",
            ItemKind::Trait(_) => "item.trait",
            ItemKind::Impl(_) => "item.impl",
            ItemKind::Struct(_) => "item.struct",
            ItemKind::Enum(declaration) => {
                for variant in &declaration.variants {
                    observed.insert(match variant.payload {
                        EnumVariantPayload::Absent => "enum_payload.absent",
                        EnumVariantPayload::Tuple(_) => "enum_payload.tuple",
                        EnumVariantPayload::Named(_) => "enum_payload.named",
                    });
                }
                "item.enum"
            }
            ItemKind::TypeAlias(_) => "item.type_alias",
            ItemKind::Const(_) => "item.const",
            ItemKind::AssociatedType(_) => "item.associated_type",
            ItemKind::AssociatedTypeBinding(_) => "item.associated_type_binding",
            ItemKind::StorageField(field) => {
                observed.insert(match field.syntax {
                    StorageFieldSyntax::Field => "storage.field",
                    StorageFieldSyntax::Var => "storage.var",
                });
                "item.storage_field"
            }
            ItemKind::Property(_) => "item.property",
            ItemKind::Invalid => "item.invalid",
        };
        observed.insert(tag);
        if !item.attributes().is_empty() {
            observed.insert("attribute");
        }
    }

    for (_, ty) in hir.type_refs() {
        observed.insert(match ty.kind() {
            TypeRefKind::Reference { .. } => "type.reference",
            TypeRefKind::Pointer { .. } => "type.pointer",
            TypeRefKind::Path { .. } => "type.path",
            TypeRefKind::Dynamic { .. } => "type.dynamic",
            TypeRefKind::Tuple(_) => "type.tuple",
            TypeRefKind::Function { .. } => "type.function",
            TypeRefKind::Group(_) => "type.group",
            TypeRefKind::Invalid => "type.invalid",
        });
    }

    for (_, body) in hir.bodies() {
        for (_, expression) in body.expressions() {
            let tag = match expression.kind() {
                ExprKind::Block { .. } => "expr.block",
                ExprKind::Binary { .. } => "expr.binary",
                ExprKind::Unary { .. } => "expr.unary",
                ExprKind::Reference { .. } => "expr.reference",
                ExprKind::Own(_) => "expr.own",
                ExprKind::Move(_) => "expr.move",
                ExprKind::DropOwn { .. } => "expr.drop_own",
                ExprKind::Call(call) => {
                    observed.insert(match call.syntax {
                        CallSyntax::Direct => "call.direct",
                        CallSyntax::ExplicitTypeArguments => "call.explicit_types",
                        CallSyntax::TrailingArgument => "call.trailing_argument",
                        CallSyntax::PostfixThen => "call.postfix_then",
                        CallSyntax::PostfixApply => "call.postfix_apply",
                    });
                    if call.partial_application {
                        observed.insert("call.partial_application");
                    }
                    "expr.call"
                }
                ExprKind::CatalogCall(_) => "expr.catalog_call",
                ExprKind::Member { .. } => "expr.member",
                ExprKind::TupleProjection { .. } => "expr.tuple_projection",
                ExprKind::Try(_) => "expr.try",
                ExprKind::DTypeCall(_) => "expr.dtype_call",
                ExprKind::ReduceByCandidate(_) => "expr.reduce_by_candidate",
                ExprKind::Literal(_) => "expr.literal",
                ExprKind::Name(_) => "expr.name",
                ExprKind::Hole => "expr.hole",
                ExprKind::Unit => "expr.unit",
                ExprKind::Group(_) => "expr.group",
                ExprKind::Unsafe(_) => "expr.unsafe",
                ExprKind::Tuple(_) => "expr.tuple",
                ExprKind::Array(_) => "expr.array",
                ExprKind::Map(_) => "expr.map",
                ExprKind::StructLiteral { .. } => "expr.struct_literal",
                ExprKind::VariantConstructor { payload, .. } => {
                    observed.insert(match payload {
                        ConstructorPayload::Absent => "constructor_payload.absent",
                        ConstructorPayload::Tuple(_) => "constructor_payload.tuple",
                        ConstructorPayload::Named(_) => "constructor_payload.named",
                    });
                    "expr.variant_constructor"
                }
                ExprKind::Lambda(lambda) => {
                    observed.insert(match lambda.syntax {
                        LambdaSyntax::Explicit => "lambda.explicit",
                        LambdaSyntax::Implicit => "lambda.implicit",
                    });
                    "expr.lambda"
                }
                ExprKind::If(_) => "expr.if",
                ExprKind::Match(_) => "expr.match",
                ExprKind::Result(_) => "expr.result",
                ExprKind::Checked(_) => "expr.checked",
                ExprKind::TensorSchema(schema) => {
                    observed.insert(match schema.family {
                        TensorSchemaFamily::View => "tensor.view",
                        TensorSchemaFamily::Rearrange => "tensor.rearrange",
                        TensorSchemaFamily::Inflate => "tensor.inflate",
                        TensorSchemaFamily::Reduce => "tensor.reduce",
                        TensorSchemaFamily::Contract => "tensor.contract",
                        TensorSchemaFamily::Einop => "tensor.einop",
                    });
                    "expr.tensor_schema"
                }
                ExprKind::Invalid => "expr.invalid",
            };
            observed.insert(tag);
        }

        for (_, statement) in body.statements() {
            observed.insert(match statement.kind() {
                StmtKind::Let { .. } => "stmt.let",
                StmtKind::Assign { .. } => "stmt.assign",
                StmtKind::While { .. } => "stmt.while",
                StmtKind::For { .. } => "stmt.for",
                StmtKind::Break => "stmt.break",
                StmtKind::Continue => "stmt.continue",
                StmtKind::Return(_) => "stmt.return",
                StmtKind::Expression(_) => "stmt.expression",
                StmtKind::Yield(_) => "stmt.yield",
                StmtKind::Invalid => "stmt.invalid",
            });
        }

        for (_, pattern) in body.patterns() {
            observed.insert(match pattern.kind() {
                PatternKind::Or(_) => "pattern.or",
                PatternKind::Alias { .. } => "pattern.alias",
                PatternKind::Binding(_) => "pattern.binding",
                PatternKind::Wildcard => "pattern.wildcard",
                PatternKind::Group(_) => "pattern.group",
                PatternKind::Literal(_) => "pattern.literal",
                PatternKind::Unit => "pattern.unit",
                PatternKind::Variant { .. } => "pattern.variant",
                PatternKind::Tuple(_) => "pattern.tuple",
                PatternKind::Struct { .. } => "pattern.struct",
                PatternKind::Invalid => "pattern.invalid",
            });
        }

        for (_, axis) in body.axes() {
            observed.insert(match axis.kind() {
                AxisExprKind::Name(_) => "axis.name",
                AxisExprKind::Integer(_) => "axis.integer",
                AxisExprKind::Pack(None) => "axis.anonymous_pack",
                AxisExprKind::Pack(Some(_)) => "axis.named_pack",
                AxisExprKind::Group(_) => "axis.group",
                AxisExprKind::Add { .. } => "axis.add",
                AxisExprKind::Multiply { .. } => "axis.multiply",
                AxisExprKind::Invalid => "axis.invalid",
            });
        }
    }
}

#[test]
fn every_valid_production_compilation_unit_lowers_without_invalid_structure() {
    let mut lowered = 0;
    for (index, case) in corpus().cases.into_iter().enumerate() {
        let CaseSource::Utf8 { text } = case.source else {
            continue;
        };
        if case.entrypoint != "compilation_unit" || case.expectation.kind != "success" {
            continue;
        }
        let parse = parse_text(
            u32::try_from(index).expect("corpus index should fit u32"),
            &text,
        );
        assert!(
            parse.is_success(),
            "corpus case {index}: {:?}",
            parse.diagnostics()
        );
        let hir = lower(&parse);
        assert!(
            hir.invalid_occurrences().is_empty(),
            "corpus case {index}: {:?}",
            hir.invalid_occurrences()
        );
        assert_eq!(hir.syntax_causes().len(), 0, "corpus case {index}");
        lowered += 1;
    }
    assert!(
        lowered > 1_800,
        "unexpectedly narrow production corpus: {lowered}"
    );
}

#[test]
fn production_corpus_reaches_every_source_semantic_hir_form() {
    let mut observed = BTreeSet::new();
    for (index, case) in corpus().cases.into_iter().enumerate() {
        let CaseSource::Utf8 { text } = case.source else {
            continue;
        };
        if case.entrypoint != "compilation_unit" || case.expectation.kind != "success" {
            continue;
        }
        let parse = parse_text(
            u32::try_from(index).expect("corpus index should fit u32"),
            &text,
        );
        let hir = lower(&parse);
        observe_hir_taxonomy(&hir, &mut observed);
    }
    let grouped = parse_text(
        u32::MAX,
        r#"fn grouped(value: (i64)) -> i64 {
  let (bound) = value
  yield bound
}
"#,
    );
    assert!(grouped.is_success(), "{:?}", grouped.diagnostics());
    observe_hir_taxonomy(&lower(&grouped), &mut observed);

    let expected = BTreeSet::from([
        "module",
        "attribute",
        "item.import",
        "item.function",
        "item.extern",
        "item.trait",
        "item.impl",
        "item.struct",
        "item.enum",
        "item.type_alias",
        "item.const",
        "item.associated_type",
        "item.associated_type_binding",
        "item.storage_field",
        "item.property",
        "import.single",
        "import.group",
        "function_body.absent",
        "function_body.expression",
        "function_body.block",
        "storage.field",
        "storage.var",
        "enum_payload.absent",
        "enum_payload.tuple",
        "enum_payload.named",
        "type.reference",
        "type.pointer",
        "type.path",
        "type.dynamic",
        "type.tuple",
        "type.function",
        "type.group",
        "expr.block",
        "expr.binary",
        "expr.unary",
        "expr.reference",
        "expr.own",
        "expr.move",
        "expr.drop_own",
        "expr.call",
        "expr.catalog_call",
        "expr.member",
        "expr.tuple_projection",
        "expr.try",
        "expr.dtype_call",
        "expr.reduce_by_candidate",
        "expr.literal",
        "expr.name",
        "expr.hole",
        "expr.unit",
        "expr.group",
        "expr.unsafe",
        "expr.tuple",
        "expr.array",
        "expr.map",
        "expr.struct_literal",
        "expr.variant_constructor",
        "expr.lambda",
        "expr.if",
        "expr.match",
        "expr.result",
        "expr.checked",
        "expr.tensor_schema",
        "call.direct",
        "call.explicit_types",
        "call.trailing_argument",
        "call.postfix_then",
        "call.postfix_apply",
        "call.partial_application",
        "constructor_payload.absent",
        "constructor_payload.tuple",
        "constructor_payload.named",
        "lambda.explicit",
        "lambda.implicit",
        "tensor.view",
        "tensor.rearrange",
        "tensor.inflate",
        "tensor.reduce",
        "tensor.contract",
        "tensor.einop",
        "stmt.let",
        "stmt.assign",
        "stmt.while",
        "stmt.for",
        "stmt.break",
        "stmt.continue",
        "stmt.return",
        "stmt.expression",
        "stmt.yield",
        "pattern.or",
        "pattern.alias",
        "pattern.binding",
        "pattern.wildcard",
        "pattern.group",
        "pattern.literal",
        "pattern.unit",
        "pattern.variant",
        "pattern.tuple",
        "pattern.struct",
        "axis.name",
        "axis.integer",
        "axis.anonymous_pack",
        "axis.named_pack",
        "axis.group",
        "axis.add",
        "axis.multiply",
    ]);
    let missing = expected.difference(&observed).copied().collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "production HIR coverage gaps: {missing:?}"
    );
}

#[test]
fn recovered_compilation_units_retain_structural_invalid_provenance() {
    let mut lowered = 0;
    for (index, case) in corpus().cases.into_iter().enumerate() {
        let CaseSource::Utf8 { text } = case.source else {
            continue;
        };
        if case.entrypoint != "compilation_unit" || case.expectation.kind != "parse_failure" {
            continue;
        }
        let parse = parse_text(
            u32::try_from(index).expect("corpus index should fit u32"),
            &text,
        );
        assert!(!parse.is_success(), "corpus case {index}");
        let hir = lower(&parse);
        let parser_spans = parse
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.span())
            .collect::<Vec<_>>();
        let hir_spans = hir
            .syntax_causes()
            .map(|(_, span)| span)
            .collect::<Vec<_>>();
        assert_eq!(hir_spans, parser_spans, "corpus case {index}");
        assert!(
            !hir.invalid_occurrences().is_empty(),
            "corpus case {index} lost its recovery boundary"
        );
        for occurrence in hir.invalid_occurrences() {
            if occurrence.category() == InvalidCategory::ReportedSyntax {
                assert_eq!(occurrence.syntax_causes().len(), 1, "corpus case {index}");
            } else {
                assert!(
                    !occurrence.syntax_causes().is_empty(),
                    "corpus case {index} has cause-free {occurrence:?}; invalid={:?}, parser={parser_spans:?}",
                    hir.origin(occurrence.origin()).source(),
                );
            }
            for &cause in occurrence.syntax_causes() {
                assert!(
                    parser_spans.contains(&hir.syntax_cause(cause)),
                    "corpus case {index} references an unknown syntax cause"
                );
            }
        }
        lowered += 1;
    }
    assert!(
        lowered > 50,
        "unexpectedly narrow recovery corpus: {lowered}"
    );
}

#[test]
fn lowering_is_deterministic_and_body_local_ids_do_not_alias() {
    let parse = parse_text(
        7,
        r#"fn first(x: i64) -> i64 = x
fn second(y: i64) -> i64 = y
"#,
    );
    assert!(parse.is_success(), "{:?}", parse.diagnostics());
    let first = lower(&parse);
    let second = lower(&parse);
    assert_eq!(first, second);

    let bodies = first
        .roots()
        .iter()
        .map(|id| match first.item(*id).kind() {
            ItemKind::Function(function) => function.body,
            kind => panic!("expected function, found {kind:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(bodies.len(), 2);
    assert_ne!(bodies[0], bodies[1]);

    let first_expr = first.body(bodies[0]).expressions().next().unwrap().0;
    let second_expr = first.body(bodies[1]).expressions().next().unwrap().0;
    assert_eq!(first_expr.local(), second_expr.local());
    assert_ne!(first_expr, second_expr);
    assert!(first.body(bodies[0]).expression(second_expr).is_none());
}

#[test]
fn call_rewrites_and_tensor_candidates_retain_exact_written_origins() {
    let parse = parse_text(
        11,
        r#"fn add(a: i64, b: i64) -> i64 = wrapping_add(a, b)
fn id(x: i64) -> i64 = x
fn run(f: Fn(i64) -> i64) -> i64 = f(1)

fn main() -> i64 {
  let a = 1.then(id)
  let b = (a, 2).apply(add)
  let partial = a.then(_)
  let c = run { |x| x }
  let schema = reduce{ h w -> h }.reduce_by((w, id))
  let shaped = view{ h w -> w h }
  yield b
}
"#,
    );
    assert!(parse.is_success(), "{:?}", parse.diagnostics());
    let hir = lower(&parse);
    assert!(hir.invalid_occurrences().is_empty());

    let main_body = hir
        .roots()
        .iter()
        .filter_map(|id| match hir.item(*id).kind() {
            ItemKind::Function(function) if function.name.text() == Some("main") => {
                Some(function.body)
            }
            _ => None,
        })
        .next()
        .expect("main body should exist");

    let mut call_kinds = Vec::new();
    let mut saw_postfix_callable_hole = false;
    let mut schema_family = None;
    let mut reduce_candidates = None;
    for (_, expression) in hir.body(main_body).expressions() {
        match expression.kind() {
            ExprKind::Call(call) => {
                call_kinds.push(call.syntax);
                if call.syntax == CallSyntax::PostfixThen
                    && matches!(
                        hir.body(main_body)
                            .expression(call.callee)
                            .map(|expr| expr.kind()),
                        Some(ExprKind::Hole)
                    )
                {
                    assert!(call.partial_application);
                    saw_postfix_callable_hole = true;
                }
                if matches!(
                    call.syntax,
                    CallSyntax::PostfixThen | CallSyntax::PostfixApply
                ) {
                    assert!(matches!(
                        hir.origin(expression.origin()),
                        SourceOrigin::Derived { .. }
                    ));
                }
                if call.syntax == CallSyntax::TrailingArgument {
                    assert!(matches!(
                        hir.origin(expression.origin()),
                        SourceOrigin::Written(_)
                    ));
                    let argument = call
                        .arguments
                        .last()
                        .expect("trailing syntax should produce one argument");
                    let value = hir
                        .body(main_body)
                        .expression(argument.value)
                        .expect("trailing value should stay in the caller body");
                    assert_eq!(
                        hir.origin(argument.origin).source(),
                        hir.origin(value.origin()).source()
                    );
                    assert_eq!(
                        hir.origin(argument.origin).desugaring(),
                        Some(DesugaringKind::TrailingArgument)
                    );
                }
            }
            ExprKind::TensorSchema(schema) => schema_family = Some(schema.family),
            ExprKind::ReduceByCandidate(candidate) => {
                reduce_candidates = Some((expression.origin(), candidate.clone()));
            }
            _ => {}
        }
    }
    assert!(call_kinds.contains(&CallSyntax::PostfixThen));
    assert!(call_kinds.contains(&CallSyntax::PostfixApply));
    assert!(call_kinds.contains(&CallSyntax::TrailingArgument));
    assert!(saw_postfix_callable_hole);
    assert_eq!(schema_family, Some(TensorSchemaFamily::View));

    let (surface_origin, candidate) = reduce_candidates.expect("reduce_by should remain distinct");
    let ordinary = candidate
        .ordinary
        .expect("ordinary interpretation should survive");
    let single = candidate
        .single_reducer
        .expect("single reducer interpretation should survive");
    let phases = candidate
        .ordered_phases
        .expect("ordered phase interpretation should survive");
    assert_eq!(ordinary.call.arguments.len(), 1);
    assert_eq!(ordinary.candidate_names.len(), 1);
    let argument = ordinary.call.arguments[0].value;
    let ExprKind::Tuple(pair) = hir
        .body(main_body)
        .expression(argument)
        .expect("ordinary phase pair should be a complete tuple expression")
        .kind()
    else {
        panic!("ordinary phase pair should lower to a tuple")
    };
    assert_eq!(pair[0], ordinary.candidate_names[0]);
    let reducer = pair[1];
    assert!(matches!(
        single.value,
        ReduceBySingleValue::PhasePair {
            reducer: shared,
            ..
        } if shared == reducer
    ));
    assert_eq!(phases.phases[0].reducer, reducer);
    assert_eq!(
        hir.origin(ordinary.origin).desugaring(),
        Some(DesugaringKind::ReduceByOrdinaryCandidate)
    );
    assert_eq!(
        hir.origin(single.origin).desugaring(),
        Some(DesugaringKind::ReduceBySingleReducerCandidate)
    );
    assert_eq!(
        hir.origin(phases.origin).desugaring(),
        Some(DesugaringKind::ReduceByOrderedPhasesCandidate)
    );
    let written = hir.origin(surface_origin).source();
    assert_eq!(hir.origin(ordinary.origin).source(), written);
    assert_eq!(hir.origin(single.origin).source(), written);
    assert_eq!(hir.origin(phases.origin).source(), written);
    assert_eq!(written.file(), FileId::new(11));
}

#[test]
fn tuple_patterns_and_lambda_surface_forms_keep_all_written_structure() {
    let parse = parse_text(
        19,
        r#"fn main!() -> Unit {
  let pair = (1, 2)
  let (left, right) = pair
  let explicit = { |x| x }
  let placeholder = { _.to_str() }
  let thunk = spawn! { 1 }
  let trailing = run { f(_) }
  yield ()
}
"#,
    );
    assert!(parse.is_success(), "{:?}", parse.diagnostics());
    let hir = lower(&parse);
    assert!(hir.invalid_occurrences().is_empty());
    let ItemKind::Function(main) = hir.item(hir.roots()[0]).kind() else {
        panic!("root should be a function");
    };

    let tuple = hir
        .body(main.body)
        .patterns()
        .find_map(|(_, pattern)| match pattern.kind() {
            PatternKind::Tuple(elements) => Some(elements),
            _ => None,
        })
        .expect("tuple binding should remain a tuple pattern");
    assert_eq!(tuple.len(), 2);
    assert!(
        tuple
            .iter()
            .all(|element| matches!(element, PatternElement::Pattern(_)))
    );

    let mut lambda_syntax = Vec::new();
    let mut trailing_callees = Vec::new();
    for (_, body) in hir.bodies() {
        for (_, expression) in body.expressions() {
            match expression.kind() {
                ExprKind::Lambda(lambda) => {
                    lambda_syntax.push(lambda.syntax);
                    assert!(matches!(
                        hir.origin(expression.origin()),
                        SourceOrigin::Written(_)
                    ));
                }
                ExprKind::Call(call) if call.syntax == CallSyntax::TrailingArgument => {
                    assert!(matches!(
                        hir.origin(expression.origin()),
                        SourceOrigin::Written(_)
                    ));
                    let argument = call
                        .arguments
                        .last()
                        .expect("trailing syntax should produce one argument");
                    assert_eq!(
                        hir.origin(argument.origin).desugaring(),
                        Some(DesugaringKind::TrailingArgument)
                    );
                    let ExprKind::Name(path) = body
                        .expression(call.callee)
                        .expect("trailing call callee should remain in its body")
                        .kind()
                    else {
                        panic!("trailing call should preserve its written name callee");
                    };
                    trailing_callees.push(
                        path.last()
                            .and_then(|name| name.text())
                            .expect("callee should have text")
                            .to_owned(),
                    );
                }
                _ => {}
            }
        }
    }
    assert!(lambda_syntax.contains(&LambdaSyntax::Explicit));
    assert!(lambda_syntax.contains(&LambdaSyntax::Implicit));
    trailing_callees.sort();
    assert_eq!(trailing_callees, ["run", "spawn"]);
}

#[test]
fn attribute_arguments_retain_their_complete_written_ranges() {
    let source = r#"#[probe(key = nested(1), "value")]
fn decorated() -> Unit = ()
"#;
    let parse = parse_text(23, source);
    assert!(parse.is_success(), "{:?}", parse.diagnostics());
    let hir = lower(&parse);
    assert!(hir.invalid_occurrences().is_empty());

    let attributes = hir.item(hir.roots()[0]).attributes();
    assert_eq!(attributes.len(), 1);
    let arguments = &attributes[0].meta.arguments;
    assert_eq!(arguments.len(), 2);
    assert!(matches!(arguments[0].value, AttributeValue::Meta(_)));
    assert!(matches!(arguments[1].value, AttributeValue::Literal(_)));

    let written = arguments
        .iter()
        .map(|argument| {
            let range = hir.origin(argument.origin).source().range();
            &source[range.start().as_usize()..range.end().as_usize()]
        })
        .collect::<Vec<_>>();
    assert_eq!(written, ["key = nested(1)", "\"value\""]);
}

#[test]
fn direct_integer_minus_retains_one_literal_and_its_complete_source_span() {
    let text = "fn value() -> i8 = - /* sign */ 128\n";
    let parse = parse_text(91, text);
    assert!(parse.is_success());
    let hir = lower(&parse);
    let (_, body) = hir.bodies().next().unwrap();
    let literals = body
        .expressions()
        .filter_map(|(_, expression)| match expression.kind() {
            ExprKind::Literal(literal) => Some(literal),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(literals.len(), 1);
    assert_eq!(literals[0].spelling(), "-128");
    let SourceOrigin::Written(span) = hir.origin(literals[0].origin()) else {
        panic!("written span");
    };
    assert_eq!(
        &text[span.range().start().get() as usize..span.range().end().get() as usize],
        "- /* sign */ 128"
    );
}

#[test]
fn nonliteral_and_grouped_negation_keep_operator_structure() {
    for source in [
        "fn value(x: i8) -> i8 = -x\n",
        "fn value() -> i8 = -(128)\n",
        "fn value() -> i8 = --128\n",
    ] {
        let parse = parse_text(92, source);
        assert!(parse.is_success(), "{source}");
        let hir = lower(&parse);
        assert!(
            hir.bodies()
                .flat_map(|(_, body)| body.expressions())
                .any(|(_, expression)| matches!(
                    expression.kind(),
                    ExprKind::Unary {
                        operator: eglur_hir::UnaryOperator::Negate,
                        ..
                    }
                )),
            "{source}"
        );
    }
}
