use super::expression::ExpressionPlan;
use super::statements::{BlockFrame, StatementContinuation, StatementPlan};
use super::*;

enum Cleanup {
    Region(ControlRegionKey),
    PathBarrier(Option<usize>),
}

enum Step {
    Expression(ExprId, TypeRequest),
    Publish(ExpressionKey),
    Block(BlockFrame),
    Statement(StmtId, Option<TypeId>),
    ResumeStatement(StatementContinuation),
    ResumeBlock(BlockFrame),
    Cleanup(Cleanup),
}

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn type_expression_steps(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let mut pending = vec![Step::Expression(expression, request)];
        let result = self.run_expression_steps(context, &mut pending);
        let Err(mut error) = result else {
            return result;
        };

        // Unwind lexical scopes without publishing or resuming failed work.
        while let Some(step) = pending.pop() {
            if let Step::Cleanup(cleanup) = step
                && let Err(cleanup_error) = self.finish_typing_scope(cleanup)
            {
                error = cleanup_error;
            }
        }
        Err(error)
    }

    fn run_expression_steps(
        &mut self,
        context: BodyContext<'_, '_>,
        pending: &mut Vec<Step>,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let mut expression_value = None;
        let mut statement_value = None;
        while let Some(step) = pending.pop() {
            match step {
                Step::Expression(expression, request) => {
                    match self.prepare_expression(context, expression, request)? {
                        ExpressionPlan::Complete(outcome) => expression_value = Some(outcome),
                        ExpressionPlan::Publish { key, outcome } => {
                            expression_value = Some(outcome);
                            pending.push(Step::Publish(key));
                        }
                        ExpressionPlan::Block {
                            key,
                            statements,
                            expected,
                        } => {
                            pending.push(Step::Publish(key));
                            if self.block_region_overrides.last() != Some(&expression) {
                                let region = ControlRegionKey::Block(key);
                                self.enter_control_region(region.clone())?;
                                pending.push(Step::Cleanup(Cleanup::Region(region)));
                            }
                            pending.push(Step::Block(BlockFrame::new(statements, expected)));
                        }
                        ExpressionPlan::Forward {
                            key,
                            expression,
                            request,
                        } => {
                            pending.push(Step::Publish(key));
                            pending.push(Step::Expression(expression, request));
                        }
                    }
                }
                Step::Publish(key) => {
                    let outcome = expression_value
                        .as_ref()
                        .expect("expression publication follows its evaluation");
                    self.record_expression(key, outcome.clone())?;
                }
                Step::Block(mut block) => {
                    if let Some((statement, expected, path)) = block.next_statement() {
                        let barrier = self.push_control_path_barrier(path);
                        pending.push(Step::ResumeBlock(block));
                        pending.push(Step::Cleanup(Cleanup::PathBarrier(barrier)));
                        pending.push(Step::Statement(statement, expected));
                    } else {
                        expression_value = Some(self.finish_block(block)?);
                    }
                }
                Step::Statement(statement, expected) => {
                    let plan = self.prepare_statement(context, statement, expected)?;
                    self.schedule_statement(plan, &mut statement_value, pending);
                }
                Step::ResumeStatement(continuation) => {
                    let value = expression_value
                        .take()
                        .expect("statement continuation follows its child expression");
                    let plan = self.resume_statement(context, continuation, value)?;
                    self.schedule_statement(plan, &mut statement_value, pending);
                }
                Step::ResumeBlock(block) => {
                    let value = statement_value
                        .take()
                        .expect("block continuation follows its statement");
                    pending.push(Step::Block(self.resume_block(block, value)?));
                }
                Step::Cleanup(cleanup) => self.finish_typing_scope(cleanup)?,
            }
        }
        Ok(expression_value.expect("expression work produces its final result"))
    }

    fn schedule_statement(
        &mut self,
        plan: StatementPlan,
        value: &mut Option<Outcome<BlockStep>>,
        pending: &mut Vec<Step>,
    ) {
        match plan {
            StatementPlan::Complete(outcome) => *value = Some(outcome),
            StatementPlan::Expression {
                expression,
                request,
                path,
                continuation,
            } => {
                let barrier = self.push_control_path_barrier(path);
                pending.push(Step::ResumeStatement(continuation));
                pending.push(Step::Cleanup(Cleanup::PathBarrier(barrier)));
                pending.push(Step::Expression(expression, request));
            }
        }
    }

    fn finish_typing_scope(&mut self, cleanup: Cleanup) -> Result<(), BodyTypingError> {
        match cleanup {
            Cleanup::Region(region) => self.leave_control_region(&region),
            Cleanup::PathBarrier(barrier) => self.pop_control_path_barrier(barrier),
        }
    }
}
