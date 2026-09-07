use eglur_source::{FileId, SourceFile};
use eglur_syntax::ast::{AstNode, Expr, Pattern, Type};
use eglur_syntax::{
    FrontendOutcome, Parse, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken, parse,
};

const FILE_ID: FileId = FileId::new(41);
const SOURCE: &str = r#"// multibyte source marker: Ω
struct Counter {}

fn lower(value: Opt[i64], schema: ReduceSchema, counter: Counter) -> i64 {
  let chosen: i64 = match value {
    Some(inner) => inner,
    None => 0,
  }
  let single = schema.reduce_by(f)
  let ambiguous = counter.reduce_by((h, f))
  let phase_only = schema.reduce_by(((h w), f))
  let empty = counter.reduce_by()
  let phases = schema.reduce_by((h, f), (w, g))
  yield chosen
}
"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReduceBySyntaxCandidate {
    OrdinaryCall,
    SingleReducer,
    OrderedPhases,
}

#[derive(Clone, Copy, Debug)]
enum ResolvedReceiver {
    ReduceSchema,
    Other,
}

fn parse_source() -> (SourceFile, Parse) {
    let source = SourceFile::new(FILE_ID, SOURCE.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parse) = parse(&source) else {
        panic!("valid UTF-8 source should reach the parser");
    };
    assert!(parse.is_success(), "{:?}", parse.diagnostics());
    (source, parse)
}

#[test]
fn empty_match_is_a_valid_expression_with_no_arms() {
    let source = SourceFile::new(
        FILE_ID,
        b"fn absurd(value: Empty) -> Never = match value {}\n".to_vec(),
    );
    let FrontendOutcome::Parsed(parse) = parse(&source) else {
        panic!("valid UTF-8 source should reach the parser");
    };
    assert!(parse.is_success(), "{:?}", parse.diagnostics());

    let match_expression = descendants_in_source_order(&parse.syntax())
        .into_iter()
        .find(|node| node.kind() == SyntaxKind::MatchExpr)
        .expect("source should contain one match expression");
    assert_eq!(
        match_expression
            .children()
            .filter(|child| child.kind() == SyntaxKind::MatchArm)
            .count(),
        0
    );
}

fn descendants_in_source_order(root: &SyntaxNode) -> Vec<SyntaxNode> {
    fn visit(node: &SyntaxNode, output: &mut Vec<SyntaxNode>) {
        output.push(node.clone());
        for child in node.children() {
            visit(&child, output);
        }
    }

    let mut output = Vec::new();
    visit(root, &mut output);
    output
}

fn tokens_in_source_order(root: &SyntaxNode) -> Vec<SyntaxToken> {
    fn visit(node: &SyntaxNode, output: &mut Vec<SyntaxToken>) {
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Node(child) => visit(&child, output),
                SyntaxElement::Token(token) => output.push(token),
            }
        }
    }

    let mut output = Vec::new();
    visit(root, &mut output);
    output
}

fn reduce_by_candidates(node: &SyntaxNode) -> Vec<ReduceBySyntaxCandidate> {
    assert_eq!(node.kind(), SyntaxKind::ReduceByCandidateExpr);
    let arguments = node
        .children()
        .find(|child| child.kind() == SyntaxKind::ReduceByArgumentList)
        .expect("reduce_by candidate should own one argument list");
    let phases = arguments
        .children()
        .filter(|child| child.kind() == SyntaxKind::ReduceByPhase)
        .collect::<Vec<_>>();

    if phases.is_empty() {
        let direct_children = arguments.children().collect::<Vec<_>>();
        let mut candidates = vec![ReduceBySyntaxCandidate::OrdinaryCall];
        if direct_children.len() == 1 && Expr::can_cast(direct_children[0].kind()) {
            candidates.push(ReduceBySyntaxCandidate::SingleReducer);
        }
        return candidates;
    }

    let ordinary_call = phases.iter().all(|phase| {
        phase
            .children_with_tokens()
            .filter(|element| {
                matches!(element, SyntaxElement::Token(token) if token.kind() == SyntaxKind::Identifier)
            })
            .count()
            == 1
    });
    let mut candidates = Vec::with_capacity(3);
    if ordinary_call {
        candidates.push(ReduceBySyntaxCandidate::OrdinaryCall);
        if phases.len() == 1 {
            candidates.push(ReduceBySyntaxCandidate::SingleReducer);
        }
    }
    candidates.push(ReduceBySyntaxCandidate::OrderedPhases);
    candidates
}

fn select_reduce_by_candidate(
    candidates: &[ReduceBySyntaxCandidate],
    receiver: ResolvedReceiver,
) -> Option<ReduceBySyntaxCandidate> {
    let contains = |candidate| candidates.contains(&candidate);
    match receiver {
        ResolvedReceiver::ReduceSchema if contains(ReduceBySyntaxCandidate::OrderedPhases) => {
            Some(ReduceBySyntaxCandidate::OrderedPhases)
        }
        ResolvedReceiver::ReduceSchema if contains(ReduceBySyntaxCandidate::SingleReducer) => {
            Some(ReduceBySyntaxCandidate::SingleReducer)
        }
        ResolvedReceiver::ReduceSchema => None,
        ResolvedReceiver::Other if contains(ReduceBySyntaxCandidate::OrdinaryCall) => {
            Some(ReduceBySyntaxCandidate::OrdinaryCall)
        }
        ResolvedReceiver::Other => None,
    }
}

#[test]
fn public_facade_preserves_typed_partitions_source_identity_and_byte_spans() {
    let (source, parse) = parse_source();
    assert_eq!(parse.file(), FILE_ID);

    let root = parse.root();
    let items = root.items().collect::<Vec<_>>();
    assert_eq!(
        items
            .iter()
            .map(|item| item.syntax().kind())
            .collect::<Vec<_>>(),
        [SyntaxKind::StructDecl, SyntaxKind::FnDecl]
    );

    let syntax = parse.syntax();
    let nodes = descendants_in_source_order(&syntax);
    let expressions = nodes
        .iter()
        .cloned()
        .filter_map(Expr::cast)
        .collect::<Vec<_>>();
    let types = nodes
        .iter()
        .cloned()
        .filter_map(Type::cast)
        .collect::<Vec<_>>();
    let patterns = nodes
        .iter()
        .cloned()
        .filter_map(Pattern::cast)
        .collect::<Vec<_>>();
    assert!(!expressions.is_empty());
    assert!(!types.is_empty());
    assert!(!patterns.is_empty());
    assert!(
        expressions
            .iter()
            .any(|expression| expression.syntax().kind() == SyntaxKind::MatchExpr)
    );
    assert!(
        patterns
            .iter()
            .any(|pattern| pattern.syntax().kind() == SyntaxKind::VariantPattern)
    );

    for node in &nodes {
        let span = node.span();
        assert_eq!(span.file(), FILE_ID);
        let range = span.range();
        let exact =
            std::str::from_utf8(&source.bytes()[range.start().as_usize()..range.end().as_usize()])
                .expect("syntax node boundaries should remain UTF-8 boundaries");
        assert_eq!(node.text(), exact);
    }

    for token in tokens_in_source_order(&syntax) {
        let span = token.span();
        assert_eq!(span.file(), FILE_ID);
        let range = span.range();
        let exact =
            std::str::from_utf8(&source.bytes()[range.start().as_usize()..range.end().as_usize()])
                .expect("syntax token boundaries should remain UTF-8 boundaries");
        assert_eq!(token.text(), exact);
    }
}

#[test]
fn reduce_by_candidates_survive_receiver_resolution_without_source_reparse() {
    let (_, parse) = parse_source();
    let reduce_by = descendants_in_source_order(&parse.syntax())
        .into_iter()
        .filter(|node| node.kind() == SyntaxKind::ReduceByCandidateExpr)
        .collect::<Vec<_>>();
    assert_eq!(reduce_by.len(), 5);

    let candidates = reduce_by
        .iter()
        .map(reduce_by_candidates)
        .collect::<Vec<_>>();
    assert_eq!(
        candidates,
        [
            vec![
                ReduceBySyntaxCandidate::OrdinaryCall,
                ReduceBySyntaxCandidate::SingleReducer,
            ],
            vec![
                ReduceBySyntaxCandidate::OrdinaryCall,
                ReduceBySyntaxCandidate::SingleReducer,
                ReduceBySyntaxCandidate::OrderedPhases,
            ],
            vec![ReduceBySyntaxCandidate::OrderedPhases],
            vec![ReduceBySyntaxCandidate::OrdinaryCall],
            vec![
                ReduceBySyntaxCandidate::OrdinaryCall,
                ReduceBySyntaxCandidate::OrderedPhases,
            ],
        ]
    );

    assert_eq!(
        select_reduce_by_candidate(&candidates[0], ResolvedReceiver::ReduceSchema),
        Some(ReduceBySyntaxCandidate::SingleReducer)
    );
    assert_eq!(
        select_reduce_by_candidate(&candidates[1], ResolvedReceiver::ReduceSchema),
        Some(ReduceBySyntaxCandidate::OrderedPhases)
    );
    assert_eq!(
        select_reduce_by_candidate(&candidates[1], ResolvedReceiver::Other),
        Some(ReduceBySyntaxCandidate::OrdinaryCall)
    );
    assert_eq!(
        select_reduce_by_candidate(&candidates[2], ResolvedReceiver::Other),
        None
    );
    assert_eq!(
        select_reduce_by_candidate(&candidates[3], ResolvedReceiver::ReduceSchema),
        None
    );
    assert_eq!(
        select_reduce_by_candidate(&candidates[3], ResolvedReceiver::Other),
        Some(ReduceBySyntaxCandidate::OrdinaryCall)
    );
    assert_eq!(
        select_reduce_by_candidate(&candidates[4], ResolvedReceiver::ReduceSchema),
        Some(ReduceBySyntaxCandidate::OrderedPhases)
    );
}
