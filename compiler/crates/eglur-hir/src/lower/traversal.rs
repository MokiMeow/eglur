use super::control::ControlStep;
use eglur_source::FileSpan;
use eglur_syntax::SyntaxNode;

use super::{
    Builder, aggregate::AggregateStep, body::BodyStep, calls::CallStep, spine::SpineStep,
    tensor::ReduceStep,
};
use crate::{
    BodyId, CallArgument, ExprId, FieldInitializer, MapEntry, MatchArm, ReduceByPhase, StmtId,
};

pub(super) enum Step {
    Expression(Option<SyntaxNode>, FileSpan),
    Spine(SpineStep),
    Control(ControlStep),
    Body(BodyStep),
    Call(CallStep),
    Aggregate(AggregateStep),
    Reduce(ReduceStep),
}

#[derive(Default)]
pub(super) struct Traversal {
    pending: Vec<(BodyId, Step)>,
    // A completion consumes only its own suffix, leaving earlier siblings intact.
    // Separate result stacks keep expression, statement, and collection results typed.
    pub values: Vec<ExprId>,
    pub statements: Vec<StmtId>,
    pub arguments: Vec<CallArgument>,
    pub fields: Vec<FieldInitializer>,
    pub entries: Vec<MapEntry>,
    pub arms: Vec<MatchArm>,
    pub phases: Vec<ReduceByPhase>,
}

impl Traversal {
    pub fn push(&mut self, body: BodyId, step: Step) {
        self.pending.push((body, step));
    }

    pub fn expression(&mut self, body: BodyId, node: Option<SyntaxNode>, fallback: FileSpan) {
        self.push(body, Step::Expression(node, fallback));
    }

    pub fn value(&mut self) -> ExprId {
        self.values.pop().expect("scheduled expression is lowered")
    }
}

pub(super) fn take_tail<T>(values: &mut Vec<T>, count: usize) -> Box<[T]> {
    let start = values
        .len()
        .checked_sub(count)
        .expect("scheduled children are lowered");
    values.split_off(start).into_boxed_slice()
}

impl Builder {
    pub(super) fn lower_expr(
        &mut self,
        body: BodyId,
        root: Option<SyntaxNode>,
        fallback: FileSpan,
    ) -> ExprId {
        let mut traversal = Traversal::default();
        traversal.expression(body, root, fallback);

        while let Some((body, step)) = traversal.pending.pop() {
            match step {
                Step::Expression(node, fallback) => {
                    self.enter_expr(body, node, fallback, &mut traversal)
                }
                Step::Control(step) => self.resume_control(body, step, &mut traversal),
                Step::Spine(step) => self.resume_spine(body, step, &mut traversal),
                Step::Body(step) => self.resume_body(body, step, &mut traversal),
                Step::Call(step) => self.resume_call(body, step, &mut traversal),
                Step::Aggregate(step) => self.resume_aggregate(body, step, &mut traversal),
                Step::Reduce(step) => self.resume_reduce(body, step, &mut traversal),
            }
        }

        assert!(
            traversal.statements.is_empty()
                && traversal.arguments.is_empty()
                && traversal.fields.is_empty()
                && traversal.entries.is_empty()
                && traversal.arms.is_empty()
                && traversal.phases.is_empty(),
            "all child collections are consumed"
        );
        assert_eq!(
            traversal.values.len(),
            1,
            "expression traversal produces one result"
        );
        traversal.value()
    }
}
