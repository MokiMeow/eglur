use super::*;
use crate::body_types::checker::test_support::root;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Child {
    Integer,
    Boolean,
    Diverging,
    Poisoned(usize),
    Unperformed,
    Deferred,
    NotApplicable,
}

fn child(
    checker: &mut BodyChecker<'_, '_, '_>,
    state: Child,
    roots: &[Poison],
) -> Outcome<TypedExpression> {
    match state {
        Child::Integer | Child::Boolean => {
            let primitive = if state == Child::Integer {
                PrimitiveType::I64
            } else {
                PrimitiveType::Bool
            };
            let ty = checker.primitive(primitive).unwrap();
            Outcome::Known(checker.value_expression(ty))
        }
        Child::Diverging => {
            Outcome::Known(checker.value_expression(checker.signatures.types().never()))
        }
        Child::Poisoned(index) => Outcome::Poisoned(roots[index].clone()),
        Child::Unperformed => Outcome::Unperformed,
        Child::Deferred => Outcome::Deferred,
        Child::NotApplicable => Outcome::NotApplicable,
    }
}

fn run(
    checker: &mut BodyChecker<'_, '_, '_>,
    scrutinee: Child,
    guard: Child,
    value: Child,
    later: Child,
) -> Outcome<TypedExpression> {
    let owner = checker
        .collection
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| {
            declaration
                .name()
                .is_some_and(|name| name.text() == "probe")
        })
        .unwrap()
        .id();
    let (module, source, hir, item) = checker.source_item(owner).unwrap();
    let ItemKind::Function(function) = item.kind() else {
        panic!("function expected")
    };
    let body = hir.body(function.body);
    let (expression_id, expression) = body
        .expressions()
        .find_map(|(id, expression)| match expression.kind() {
            ExprKind::Match(value) => Some((id, value.clone())),
            _ => None,
        })
        .unwrap();
    let context = BodyContext {
        module,
        source,
        hir,
        body,
    };
    let roots = (0..4)
        .map(|index| root(checker.diagnostics, index))
        .collect::<Vec<_>>();
    let integer = checker.primitive(PrimitiveType::I64).unwrap();
    let boolean = checker.primitive(PrimitiveType::Bool).unwrap();
    let callable = ControlRegionKey::Callable(owner);
    checker.current_owner = Some(owner);
    checker.enter_control_region(callable.clone()).unwrap();
    reset();
    let initial = child(checker, scrutinee, &roots);
    let mut plan = checker
        .resume_match_scrutinee(
            source,
            ExpressionKey::new(module, expression_id),
            expression,
            TypeRequest::definitive(Some(integer)),
            initial,
        )
        .unwrap();
    let mut visited = Vec::new();
    let result = loop {
        let BranchPlan::MatchArm { region, frame, .. } = plan else {
            let BranchPlan::Complete(outcome) = plan else {
                panic!("terminal plan expected")
            };
            break outcome;
        };
        let index = frame.next_arm;
        visited.push(index);
        checker.enter_control_region(region.clone()).unwrap();
        let prepared = checker.prepare_match_arm(context, frame).unwrap();
        let (frame, path) = match prepared {
            BranchPlan::Expression {
                expression,
                continuation: BranchContinuation::MatchGuard { frame, .. },
                ..
            } => {
                let outcome = child(checker, guard, &roots);
                checker
                    .record_expression(ExpressionKey::new(module, expression), outcome.clone())
                    .unwrap();
                let BranchPlan::MatchBody { frame, path, .. } = checker
                    .resume_match_guard(source, frame, boolean, outcome)
                    .unwrap()
                else {
                    panic!("guard body expected")
                };
                (frame, path)
            }
            BranchPlan::MatchBody { frame, path, .. } => (frame, path),
            _ => panic!("arm body expected"),
        };
        let expression = frame.expression.arms[index].value;
        let outcome = child(
            checker,
            if index == 0 {
                value
            } else if index == 1 {
                later
            } else {
                Child::Integer
            },
            &roots,
        );
        checker
            .record_expression(ExpressionKey::new(module, expression), outcome.clone())
            .unwrap();
        let BranchPlan::CompleteMatchArm(frame) = checker
            .resume_match_value(source, frame, path, outcome)
            .unwrap()
        else {
            panic!("completed arm expected")
        };
        checker.leave_control_region(&region).unwrap();
        plan = checker.advance_match(source, frame).unwrap();
    };
    checker.leave_control_region(&callable).unwrap();
    assert_eq!(visited, [0, 1, 2]);
    assert!(checker.region_stack.is_empty());
    assert_eq!(
        work().completed,
        usize::from(matches!(&result, Outcome::Poisoned(_)))
    );
    result
}

#[test]
fn match_worklist_preserves_partial_precedence_and_later_poison() {
    for scrutinee in [
        Child::Integer,
        Child::Poisoned(0),
        Child::Unperformed,
        Child::Deferred,
        Child::NotApplicable,
    ] {
        for guard in [
            Child::Boolean,
            Child::Integer,
            Child::Poisoned(1),
            Child::Unperformed,
            Child::Deferred,
            Child::NotApplicable,
        ] {
            for value in [
                Child::Integer,
                Child::Boolean,
                Child::Diverging,
                Child::Poisoned(2),
                Child::Unperformed,
                Child::Deferred,
                Child::NotApplicable,
            ] {
                for later in [Child::Integer, Child::Poisoned(3)] {
                    with_checker(
                        "fn probe(x: i64) -> i64 = match x { 0 if true => 1, 1 => 2, _ => 3 }\n",
                        |checker| {
                            let result = run(checker, scrutinee, guard, value, later);
                            let poisoned = matches!(scrutinee, Child::Poisoned(_))
                                || matches!(guard, Child::Poisoned(_))
                                || guard == Child::Integer
                                || matches!(value, Child::Poisoned(_))
                                || matches!(later, Child::Poisoned(_))
                                || (guard == Child::Boolean && value == Child::Boolean);
                            let unperformed = scrutinee != Child::Integer
                                || matches!(guard, Child::Unperformed | Child::NotApplicable)
                                || matches!(value, Child::Unperformed | Child::NotApplicable);
                            let deferred = guard == Child::Deferred || value == Child::Deferred;
                            let correct = match result {
                                Outcome::Poisoned(_) => poisoned,
                                Outcome::Unperformed => !poisoned && unperformed,
                                Outcome::Deferred => !poisoned && !unperformed && deferred,
                                Outcome::Known(value) => {
                                    !poisoned
                                        && !unperformed
                                        && !deferred
                                        && value.normal_continuation().is_present()
                                }
                                Outcome::NotApplicable => false,
                            };
                            assert!(correct, "{scrutinee:?}/{guard:?}/{value:?}/{later:?}");
                        },
                    );
                }
            }
        }
    }
}
