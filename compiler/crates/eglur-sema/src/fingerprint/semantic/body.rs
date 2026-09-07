use std::collections::{BTreeMap, BTreeSet};

use eglur_hir::{
    AxisExprId, AxisExprKind, BinaryOperator, CallArgument, CallSyntax, ConstructorPayload, ExprId,
    ExprKind, FieldPattern, FunctionBody, HirFile, ItemKind, LambdaSyntax, LetSyntax, Literal,
    LiteralKind, Mutability, Name, Path, PatternElement, PatternId, PatternKind, PatternPayload,
    PointerMutability, ReduceBySingleValue, StmtId, StmtKind, TensorSchemaFamily, TypeRefId,
    TypeRefKind, UnaryOperator,
};

use crate::fingerprint::identity::{StableTraitRef, StableType, declaration_binder_positions};
use crate::{
    BodyFingerprint, BodyTypes, CallableInstance, CanonicalFingerprintBuilder, CanonicalTarget,
    CheckedDispatch, CheckedInvocation, CheckedNumericOperation, CheckedNumericOperator,
    Declaration, DeclarationCollection, DeclarationSignatures, EvidenceEntry, EvidenceSource,
    ExpressionKey, FactState, FingerprintDigest, FingerprintDomain, GenericParameterId,
    InvocationFact, InvocationTarget, MemberAccess, MemberKind, ModuleAddress, ModuleId,
    NameLookupDecision, NameLookupPlan, NameResolution, NormalContinuation, NumericFaultCause,
    ReduceByCommitment, ResolutionKey, ResolvedName, SemanticSource, StableDeclarationKey,
    StableDeclarationKeys, StableTargetKey, TraitEnvironment, TraitEvidence, TypedPlace,
    TypedPlaceKind, WitnessSlotId,
};

use super::SemanticFingerprintError;

pub(super) struct BodyProjection {
    modules: BTreeMap<ModuleAddress, BodyFingerprint>,
    declarations: BTreeMap<crate::DefId, FingerprintDigest>,
}

impl BodyProjection {
    #[allow(
        clippy::too_many_arguments,
        reason = "checked-body projection consumes independently owned semantic products"
    )]
    pub(super) fn compute(
        declarations: &DeclarationCollection,
        names: &NameResolution,
        signatures: &DeclarationSignatures,
        sources: &[SemanticSource<'_>],
        body_types: &BodyTypes,
        trait_environment: &TraitEnvironment,
        trait_evidence: &TraitEvidence,
        keys: &StableDeclarationKeys,
    ) -> Result<Self, SemanticFingerprintError> {
        let source_by_file = sources
            .iter()
            .map(|source| (source.file(), source))
            .collect::<BTreeMap<_, _>>();
        let mut module_fingerprints = BTreeMap::new();
        let mut declaration_fingerprints = BTreeMap::new();

        for module in declarations.modules() {
            let source = source_by_file
                .get(&module.file())
                .copied()
                .ok_or(SemanticFingerprintError::MissingSource(module.file()))?;
            let hir = source
                .hir()
                .ok_or(SemanticFingerprintError::MissingSource(module.file()))?;
            let mut bodies = Vec::new();
            for declaration in module.declarations() {
                let Some(root) = declaration_body_root(declaration, hir)? else {
                    continue;
                };
                let FactState::Known(summary) = body_types.body(declaration.id()) else {
                    return Err(SemanticFingerprintError::IncompleteBodyFact("body summary"));
                };
                let binders =
                    declaration_binder_positions(declarations, signatures, declaration.id())?;
                let stable_declaration = keys.source(declaration.id()).cloned().ok_or(
                    SemanticFingerprintError::UnknownStableDeclaration(declaration.id()),
                )?;
                let mut nodes = NodeHasher {
                    module: module.id(),
                    hir,
                    declarations,
                    names,
                    signatures,
                    body_types,
                    trait_environment,
                    trait_evidence,
                    keys,
                    binders,
                    expressions: BTreeMap::new(),
                    statements: BTreeMap::new(),
                    patterns: BTreeMap::new(),
                    axes: BTreeMap::new(),
                    types: BTreeMap::new(),
                    active: BTreeSet::new(),
                };
                let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Body);
                builder.write_u8(0);
                stable_declaration.write_to(&mut builder);
                keys.stable_type(signatures, summary.result(), &nodes.binders)?
                    .write_to(&mut builder);
                write_declaration_body(&mut builder, root, &mut nodes)?;
                let digest = builder.finish();
                declaration_fingerprints.insert(declaration.id(), digest);
                bodies.push((stable_declaration, digest));
            }

            bodies.sort_by(|left, right| left.0.cmp(&right.0));
            let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Body);
            builder.write_u8(1);
            builder.write_str(module.address().as_str());
            builder.write_len(bodies.len());
            for (declaration, digest) in bodies {
                declaration.write_to(&mut builder);
                builder.write_digest(digest);
            }
            module_fingerprints.insert(
                module.address().clone(),
                BodyFingerprint::new(builder.finish()),
            );
        }

        Ok(Self {
            modules: module_fingerprints,
            declarations: declaration_fingerprints,
        })
    }

    pub(super) const fn modules(&self) -> &BTreeMap<ModuleAddress, BodyFingerprint> {
        &self.modules
    }

    pub(super) fn declaration(&self, declaration: crate::DefId) -> Option<FingerprintDigest> {
        self.declarations.get(&declaration).copied()
    }
}

#[derive(Clone, Copy)]
enum DeclarationBodyRoot<'hir> {
    Function {
        declaration: &'hir eglur_hir::FunctionDecl,
        root: ExprId,
        form: FunctionBody,
    },
    Const(ExprId),
    Field(ExprId),
}

fn declaration_body_root<'hir>(
    declaration: &Declaration,
    hir: &'hir HirFile,
) -> Result<Option<DeclarationBodyRoot<'hir>>, SemanticFingerprintError> {
    let Some(item) = declaration.item() else {
        return Ok(None);
    };
    Ok(match hir.item(item).kind() {
        ItemKind::Function(function) => match function.body_form {
            FunctionBody::Absent => None,
            FunctionBody::Expression(root) | FunctionBody::Block(root) => {
                Some(DeclarationBodyRoot::Function {
                    declaration: function,
                    root,
                    form: function.body_form,
                })
            }
        },
        ItemKind::Const(value) => Some(DeclarationBodyRoot::Const(value.value)),
        ItemKind::StorageField(field) => field
            .initializer
            .map(|initializer| DeclarationBodyRoot::Field(initializer.expression)),
        ItemKind::Extern(_)
        | ItemKind::Import(_)
        | ItemKind::Trait(_)
        | ItemKind::Impl(_)
        | ItemKind::Struct(_)
        | ItemKind::Enum(_)
        | ItemKind::TypeAlias(_)
        | ItemKind::AssociatedType(_)
        | ItemKind::AssociatedTypeBinding(_)
        | ItemKind::Property(_)
        | ItemKind::Invalid => None,
    })
}

fn write_declaration_body(
    builder: &mut CanonicalFingerprintBuilder,
    root: DeclarationBodyRoot<'_>,
    nodes: &mut NodeHasher<'_>,
) -> Result<(), SemanticFingerprintError> {
    match root {
        DeclarationBodyRoot::Function {
            declaration,
            root,
            form,
        } => {
            builder.write_u8(0);
            builder.write_u8(match form {
                FunctionBody::Absent => unreachable!("absent functions have no body projection"),
                FunctionBody::Expression(_) => 0,
                FunctionBody::Block(_) => 1,
            });
            builder.write_len(declaration.parameters.len());
            for parameter in &declaration.parameters {
                builder.write_digest(nodes.pattern(parameter.pattern)?);
                write_optional_type(builder, parameter.ty, nodes)?;
            }
            builder.write_digest(nodes.expression(root)?);
        }
        DeclarationBodyRoot::Const(root) => {
            builder.write_u8(1);
            builder.write_digest(nodes.expression(root)?);
        }
        DeclarationBodyRoot::Field(root) => {
            builder.write_u8(2);
            builder.write_digest(nodes.expression(root)?);
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum NodeKey {
    Expression(ExprId),
    Statement(StmtId),
    Pattern(PatternId),
    Axis(AxisExprId),
    Type(TypeRefId),
}

struct NodeHasher<'a> {
    module: ModuleId,
    hir: &'a HirFile,
    declarations: &'a DeclarationCollection,
    names: &'a NameResolution,
    signatures: &'a DeclarationSignatures,
    body_types: &'a BodyTypes,
    trait_environment: &'a TraitEnvironment,
    trait_evidence: &'a TraitEvidence,
    keys: &'a StableDeclarationKeys,
    binders: BTreeMap<GenericParameterId, u32>,
    expressions: BTreeMap<ExprId, FingerprintDigest>,
    statements: BTreeMap<StmtId, FingerprintDigest>,
    patterns: BTreeMap<PatternId, FingerprintDigest>,
    axes: BTreeMap<AxisExprId, FingerprintDigest>,
    types: BTreeMap<TypeRefId, FingerprintDigest>,
    active: BTreeSet<NodeKey>,
}

impl NodeHasher<'_> {
    fn expression(&mut self, id: ExprId) -> Result<FingerprintDigest, SemanticFingerprintError> {
        if let Some(&digest) = self.expressions.get(&id) {
            return Ok(digest);
        }
        let key = NodeKey::Expression(id);
        if !self.active.insert(key) {
            return Err(SemanticFingerprintError::CyclicBodyNode);
        }
        let kind = self
            .hir
            .body(id.body())
            .expression(id)
            .ok_or(SemanticFingerprintError::IncompleteBodyFact("expression"))?
            .kind()
            .clone();
        let result: Result<FingerprintDigest, SemanticFingerprintError> = (|| {
            let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Body);
            builder.write_u8(2);
            self.write_expression_kind(&mut builder, id, &kind)?;
            if matches!(kind, ExprKind::Hole) {
                let key = ExpressionKey::new(self.module, id);
                match self.body_types.expression(key) {
                    FactState::Known(_) => {
                        builder.write_u8(1);
                        self.write_expression_facts(&mut builder, id)?;
                    }
                    FactState::Unperformed | FactState::NotApplicable => builder.write_u8(0),
                    FactState::Poisoned(_) => {
                        return Err(SemanticFingerprintError::IncompleteBodyFact(
                            "expression type",
                        ));
                    }
                }
            } else {
                self.write_expression_facts(&mut builder, id)?;
            }
            Ok(builder.finish())
        })();
        self.active.remove(&key);
        let digest = result?;
        self.expressions.insert(id, digest);
        Ok(digest)
    }

    fn statement(&mut self, id: StmtId) -> Result<FingerprintDigest, SemanticFingerprintError> {
        if let Some(&digest) = self.statements.get(&id) {
            return Ok(digest);
        }
        let key = NodeKey::Statement(id);
        if !self.active.insert(key) {
            return Err(SemanticFingerprintError::CyclicBodyNode);
        }
        let kind = self
            .hir
            .body(id.body())
            .statement(id)
            .ok_or(SemanticFingerprintError::IncompleteBodyFact("statement"))?
            .kind()
            .clone();
        let result: Result<FingerprintDigest, SemanticFingerprintError> = (|| {
            let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Body);
            builder.write_u8(3);
            self.write_statement_kind(&mut builder, &kind)?;
            Ok(builder.finish())
        })();
        self.active.remove(&key);
        let digest = result?;
        self.statements.insert(id, digest);
        Ok(digest)
    }

    fn pattern(&mut self, id: PatternId) -> Result<FingerprintDigest, SemanticFingerprintError> {
        if let Some(&digest) = self.patterns.get(&id) {
            return Ok(digest);
        }
        let key = NodeKey::Pattern(id);
        if !self.active.insert(key) {
            return Err(SemanticFingerprintError::CyclicBodyNode);
        }
        let kind = self
            .hir
            .body(id.body())
            .pattern(id)
            .ok_or(SemanticFingerprintError::IncompleteBodyFact("pattern"))?
            .kind()
            .clone();
        let result: Result<FingerprintDigest, SemanticFingerprintError> = (|| {
            let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Body);
            builder.write_u8(4);
            self.write_pattern_kind(&mut builder, &kind)?;
            self.write_resolution(
                &mut builder,
                ResolutionKey::Pattern {
                    module: self.module,
                    pattern: id,
                },
            )?;
            Ok(builder.finish())
        })();
        self.active.remove(&key);
        let digest = result?;
        self.patterns.insert(id, digest);
        Ok(digest)
    }

    fn axis(&mut self, id: AxisExprId) -> Result<FingerprintDigest, SemanticFingerprintError> {
        if let Some(&digest) = self.axes.get(&id) {
            return Ok(digest);
        }
        let key = NodeKey::Axis(id);
        if !self.active.insert(key) {
            return Err(SemanticFingerprintError::CyclicBodyNode);
        }
        let kind = self
            .hir
            .body(id.body())
            .axis(id)
            .ok_or(SemanticFingerprintError::IncompleteBodyFact("tensor axis"))?
            .kind()
            .clone();
        let result: Result<FingerprintDigest, SemanticFingerprintError> = (|| {
            let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Body);
            builder.write_u8(5);
            self.write_axis_kind(&mut builder, &kind)?;
            Ok(builder.finish())
        })();
        self.active.remove(&key);
        let digest = result?;
        self.axes.insert(id, digest);
        Ok(digest)
    }

    fn ty(&mut self, id: TypeRefId) -> Result<FingerprintDigest, SemanticFingerprintError> {
        if let Some(&digest) = self.types.get(&id) {
            return Ok(digest);
        }
        let key = NodeKey::Type(id);
        if !self.active.insert(key) {
            return Err(SemanticFingerprintError::CyclicBodyNode);
        }
        let kind = self.hir.type_ref(id).kind().clone();
        let result: Result<FingerprintDigest, SemanticFingerprintError> = (|| {
            let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Body);
            builder.write_u8(6);
            self.write_type_kind(&mut builder, &kind)?;
            self.write_resolution(
                &mut builder,
                ResolutionKey::Type {
                    module: self.module,
                    ty: id,
                },
            )?;
            Ok(builder.finish())
        })();
        self.active.remove(&key);
        let digest = result?;
        self.types.insert(id, digest);
        Ok(digest)
    }

    fn write_expression_kind(
        &mut self,
        builder: &mut CanonicalFingerprintBuilder,
        id: ExprId,
        kind: &ExprKind,
    ) -> Result<(), SemanticFingerprintError> {
        match kind {
            ExprKind::Block { statements } => {
                builder.write_u8(0);
                write_statement_digests(builder, statements, self)?;
            }
            ExprKind::Binary {
                operator,
                left,
                right,
            } => {
                builder.write_u8(1);
                builder.write_u8(binary_operator_tag(*operator));
                builder.write_digest(self.expression(*left)?);
                builder.write_digest(self.expression(*right)?);
            }
            ExprKind::Unary { operator, operand } => {
                builder.write_u8(2);
                builder.write_u8(unary_operator_tag(*operator));
                builder.write_digest(self.expression(*operand)?);
            }
            ExprKind::Reference { mutability, value } => {
                builder.write_u8(3);
                write_mutability(builder, *mutability);
                builder.write_digest(self.expression(*value)?);
            }
            ExprKind::Own(name) => {
                builder.write_u8(4);
                write_hir_name(builder, name);
            }
            ExprKind::Move(name) => {
                builder.write_u8(5);
                write_hir_name(builder, name);
            }
            ExprKind::DropOwn {
                explicit_move,
                name,
            } => {
                builder.write_u8(6);
                builder.write_bool(*explicit_move);
                write_hir_name(builder, name);
            }
            ExprKind::Call(call) => {
                builder.write_u8(7);
                builder.write_digest(self.expression(call.callee)?);
                write_call_arguments(builder, &call.arguments, self)?;
                write_type_digests(builder, &call.type_arguments, self)?;
                builder.write_u8(call_syntax_tag(call.syntax));
                builder.write_bool(call.partial_application);
            }
            ExprKind::CatalogCall(call) => {
                builder.write_u8(8);
                write_path(builder, &call.owner);
                write_type_digests(builder, &call.owner_arguments, self)?;
                write_hir_name(builder, &call.member);
                write_call_arguments(builder, &call.arguments, self)?;
            }
            ExprKind::Member { receiver, member } => {
                builder.write_u8(9);
                builder.write_digest(self.expression(*receiver)?);
                write_hir_name(builder, member);
            }
            ExprKind::TupleProjection { receiver, index } => {
                builder.write_u8(10);
                builder.write_digest(self.expression(*receiver)?);
                builder.write_str(index);
            }
            ExprKind::Try(operand) => {
                builder.write_u8(11);
                builder.write_digest(self.expression(*operand)?);
            }
            ExprKind::DTypeCall(call) => {
                builder.write_u8(12);
                builder.write_digest(self.expression(call.callee)?);
                builder.write_digest(self.ty(call.dtype)?);
                write_call_arguments(builder, &call.arguments, self)?;
            }
            ExprKind::ReduceByCandidate(candidate) => {
                builder.write_u8(13);
                let key = ExpressionKey::new(self.module, id);
                match self.body_types.reduce_by_commitment(key) {
                    FactState::Known(ReduceByCommitment::Ordinary) => {
                        builder.write_u8(0);
                        let ordinary = candidate
                            .ordinary
                            .as_ref()
                            .expect("ordinary commitment retains an ordinary candidate");
                        builder.write_digest(self.expression(ordinary.call.callee)?);
                        write_call_arguments(builder, &ordinary.call.arguments, self)?;
                        builder.write_u8(call_syntax_tag(ordinary.call.syntax));
                        builder.write_bool(ordinary.call.partial_application);
                    }
                    FactState::Known(ReduceByCommitment::TensorSingle) => {
                        builder.write_u8(1);
                        builder.write_digest(self.expression(candidate.receiver)?);
                        let single = candidate
                            .single_reducer
                            .as_ref()
                            .expect("single commitment retains a reducer");
                        match &single.value {
                            ReduceBySingleValue::Expression(value) => {
                                builder.write_u8(0);
                                builder.write_digest(self.expression(*value)?);
                            }
                            ReduceBySingleValue::PhasePair { axis, reducer, .. } => {
                                builder.write_u8(1);
                                write_hir_name(builder, axis);
                                builder.write_digest(self.expression(*reducer)?);
                            }
                        }
                    }
                    FactState::Known(ReduceByCommitment::TensorPhases) => {
                        builder.write_u8(2);
                        builder.write_digest(self.expression(candidate.receiver)?);
                        let phases = candidate
                            .ordered_phases
                            .as_ref()
                            .expect("phase commitment retains ordered phases");
                        builder.write_len(phases.phases.len());
                        for phase in &phases.phases {
                            builder.write_len(phase.axes.len());
                            for axis in &phase.axes {
                                write_hir_name(builder, axis);
                            }
                            builder.write_digest(self.expression(phase.reducer)?);
                        }
                    }
                    FactState::Poisoned(_) | FactState::Unperformed | FactState::NotApplicable => {
                        return Err(SemanticFingerprintError::IncompleteBodyFact(
                            "reduce_by commitment",
                        ));
                    }
                }
            }
            ExprKind::Literal(literal) => {
                builder.write_u8(14);
                write_literal(builder, literal);
            }
            ExprKind::Name(path) => {
                builder.write_u8(15);
                write_path(builder, path);
            }
            ExprKind::Hole => builder.write_u8(16),
            ExprKind::Unit => builder.write_u8(17),
            ExprKind::Group(inner) => {
                builder.write_u8(18);
                builder.write_digest(self.expression(*inner)?);
            }
            ExprKind::Unsafe(inner) => {
                builder.write_u8(19);
                builder.write_digest(self.expression(*inner)?);
            }
            ExprKind::Tuple(values) => {
                builder.write_u8(20);
                write_expression_digests(builder, values, self)?;
            }
            ExprKind::Array(values) => {
                builder.write_u8(21);
                write_expression_digests(builder, values, self)?;
            }
            ExprKind::Map(entries) => {
                builder.write_u8(22);
                builder.write_len(entries.len());
                for entry in entries {
                    builder.write_digest(self.expression(entry.key)?);
                    builder.write_digest(self.expression(entry.value)?);
                }
            }
            ExprKind::StructLiteral { path, fields } => {
                builder.write_u8(23);
                write_path(builder, path);
                builder.write_len(fields.len());
                for field in fields {
                    write_hir_name(builder, &field.name);
                    builder.write_digest(self.expression(field.value)?);
                }
            }
            ExprKind::VariantConstructor { path, payload } => {
                builder.write_u8(24);
                write_path(builder, path);
                match payload {
                    ConstructorPayload::Absent => builder.write_u8(0),
                    ConstructorPayload::Tuple(values) => {
                        builder.write_u8(1);
                        write_expression_digests(builder, values, self)?;
                    }
                    ConstructorPayload::Named(fields) => {
                        builder.write_u8(2);
                        builder.write_len(fields.len());
                        for field in fields {
                            write_hir_name(builder, &field.name);
                            builder.write_digest(self.expression(field.value)?);
                        }
                    }
                }
            }
            ExprKind::Lambda(lambda) => {
                builder.write_u8(25);
                builder.write_u8(match lambda.syntax {
                    LambdaSyntax::Explicit => 0,
                    LambdaSyntax::Implicit => 1,
                });
                builder.write_len(lambda.parameters.len());
                for parameter in &lambda.parameters {
                    builder.write_digest(self.pattern(parameter.pattern)?);
                    write_optional_type(builder, parameter.ty, self)?;
                }
                write_statement_digests(builder, &lambda.statements, self)?;
                write_optional_expression(builder, lambda.tail, self)?;
            }
            ExprKind::If(value) => {
                builder.write_u8(26);
                builder.write_digest(self.expression(value.condition)?);
                builder.write_digest(self.expression(value.then_branch)?);
                write_optional_expression(builder, value.else_branch, self)?;
                write_optional_expression(builder, value.unknown_branch, self)?;
            }
            ExprKind::Match(value) => {
                builder.write_u8(27);
                builder.write_digest(self.expression(value.scrutinee)?);
                builder.write_len(value.arms.len());
                for arm in &value.arms {
                    builder.write_digest(self.pattern(arm.pattern)?);
                    builder.write_bool(arm.guard.is_some());
                    if let Some(guard) = &arm.guard {
                        builder.write_digest(self.expression(guard.condition)?);
                    }
                    builder.write_digest(self.expression(arm.value)?);
                }
            }
            ExprKind::Result(inner) => {
                builder.write_u8(28);
                builder.write_digest(self.expression(*inner)?);
            }
            ExprKind::Checked(inner) => {
                builder.write_u8(29);
                builder.write_digest(self.expression(*inner)?);
            }
            ExprKind::TensorSchema(schema) => {
                builder.write_u8(30);
                builder.write_u8(tensor_family_tag(schema.family));
                builder.write_len(schema.operands.len());
                for list in &schema.operands {
                    write_axis_digests(builder, &list.axes, self)?;
                }
                builder.write_len(schema.outputs.len());
                for list in &schema.outputs {
                    write_axis_digests(builder, &list.axes, self)?;
                }
            }
            ExprKind::Invalid => builder.write_u8(31),
        }
        Ok(())
    }

    fn write_statement_kind(
        &mut self,
        builder: &mut CanonicalFingerprintBuilder,
        kind: &StmtKind,
    ) -> Result<(), SemanticFingerprintError> {
        match kind {
            StmtKind::Let {
                syntax,
                mutability,
                pattern,
                ty,
                initializer,
            } => {
                builder.write_u8(0);
                builder.write_u8(match syntax {
                    LetSyntax::Let => 0,
                    LetSyntax::Var => 1,
                });
                write_mutability(builder, *mutability);
                builder.write_digest(self.pattern(*pattern)?);
                write_optional_type(builder, *ty, self)?;
                write_optional_expression(builder, *initializer, self)?;
            }
            StmtKind::Assign { target, value } => {
                builder.write_u8(1);
                builder.write_digest(self.expression(*target)?);
                builder.write_digest(self.expression(*value)?);
            }
            StmtKind::While { condition, body } => {
                builder.write_u8(2);
                builder.write_digest(self.expression(*condition)?);
                builder.write_digest(self.expression(*body)?);
            }
            StmtKind::For {
                binding,
                iterable,
                body,
            } => {
                builder.write_u8(3);
                write_hir_name(builder, binding);
                builder.write_digest(self.expression(*iterable)?);
                builder.write_digest(self.expression(*body)?);
            }
            StmtKind::Break => builder.write_u8(4),
            StmtKind::Continue => builder.write_u8(5),
            StmtKind::Return(value) => {
                builder.write_u8(6);
                write_optional_expression(builder, *value, self)?;
            }
            StmtKind::Expression(value) => {
                builder.write_u8(7);
                builder.write_digest(self.expression(*value)?);
            }
            StmtKind::Yield(value) => {
                builder.write_u8(8);
                builder.write_digest(self.expression(*value)?);
            }
            StmtKind::Invalid => builder.write_u8(9),
        }
        Ok(())
    }

    fn write_pattern_kind(
        &mut self,
        builder: &mut CanonicalFingerprintBuilder,
        kind: &PatternKind,
    ) -> Result<(), SemanticFingerprintError> {
        match kind {
            PatternKind::Or(alternatives) => {
                builder.write_u8(0);
                write_pattern_digests(builder, alternatives, self)?;
            }
            PatternKind::Alias { name, pattern } => {
                builder.write_u8(1);
                write_hir_name(builder, name);
                builder.write_digest(self.pattern(*pattern)?);
            }
            PatternKind::Binding(name) => {
                builder.write_u8(2);
                write_hir_name(builder, name);
            }
            PatternKind::Wildcard => builder.write_u8(3),
            PatternKind::Group(inner) => {
                builder.write_u8(4);
                builder.write_digest(self.pattern(*inner)?);
            }
            PatternKind::Literal(literal) => {
                builder.write_u8(5);
                write_literal(builder, literal);
            }
            PatternKind::Unit => builder.write_u8(6),
            PatternKind::Variant { path, payload } => {
                builder.write_u8(7);
                write_path(builder, path);
                write_pattern_payload(builder, payload, self)?;
            }
            PatternKind::Tuple(elements) => {
                builder.write_u8(8);
                write_pattern_elements(builder, elements, self)?;
            }
            PatternKind::Struct { path, fields } => {
                builder.write_u8(9);
                builder.write_bool(path.is_some());
                if let Some(path) = path {
                    write_path(builder, path);
                }
                write_field_patterns(builder, fields, self)?;
            }
            PatternKind::Invalid => builder.write_u8(10),
        }
        Ok(())
    }

    fn write_axis_kind(
        &mut self,
        builder: &mut CanonicalFingerprintBuilder,
        kind: &AxisExprKind,
    ) -> Result<(), SemanticFingerprintError> {
        match kind {
            AxisExprKind::Name(name) => {
                builder.write_u8(0);
                write_hir_name(builder, name);
            }
            AxisExprKind::Integer(literal) => {
                builder.write_u8(1);
                write_literal(builder, literal);
            }
            AxisExprKind::Pack(name) => {
                builder.write_u8(2);
                builder.write_bool(name.is_some());
                if let Some(name) = name {
                    write_hir_name(builder, name);
                }
            }
            AxisExprKind::Group(inner) => {
                builder.write_u8(3);
                builder.write_digest(self.axis(*inner)?);
            }
            AxisExprKind::Add { left, right } => {
                builder.write_u8(4);
                builder.write_digest(self.axis(*left)?);
                builder.write_digest(self.axis(*right)?);
            }
            AxisExprKind::Multiply { left, right } => {
                builder.write_u8(5);
                builder.write_digest(self.axis(*left)?);
                builder.write_digest(self.axis(*right)?);
            }
            AxisExprKind::Invalid => builder.write_u8(6),
        }
        Ok(())
    }

    fn write_type_kind(
        &mut self,
        builder: &mut CanonicalFingerprintBuilder,
        kind: &TypeRefKind,
    ) -> Result<(), SemanticFingerprintError> {
        match kind {
            TypeRefKind::Reference {
                mutability,
                referent,
            } => {
                builder.write_u8(0);
                write_mutability(builder, *mutability);
                builder.write_digest(self.ty(*referent)?);
            }
            TypeRefKind::Pointer {
                mutability,
                referent,
            } => {
                builder.write_u8(1);
                builder.write_u8(match mutability {
                    PointerMutability::Const => 0,
                    PointerMutability::Mutable => 1,
                });
                builder.write_digest(self.ty(*referent)?);
            }
            TypeRefKind::Path { path, arguments } => {
                builder.write_u8(2);
                write_path(builder, path);
                write_type_digests(builder, arguments, self)?;
            }
            TypeRefKind::Dynamic { trait_ref } => {
                builder.write_u8(3);
                builder.write_digest(self.ty(*trait_ref)?);
            }
            TypeRefKind::Tuple(elements) => {
                builder.write_u8(4);
                write_type_digests(builder, elements, self)?;
            }
            TypeRefKind::Function { parameters, result } => {
                builder.write_u8(5);
                write_type_digests(builder, parameters, self)?;
                builder.write_digest(self.ty(*result)?);
            }
            TypeRefKind::Group(inner) => {
                builder.write_u8(6);
                builder.write_digest(self.ty(*inner)?);
            }
            TypeRefKind::Invalid => builder.write_u8(7),
        }
        Ok(())
    }

    fn write_resolution(
        &self,
        builder: &mut CanonicalFingerprintBuilder,
        key: ResolutionKey,
    ) -> Result<(), SemanticFingerprintError> {
        let resolution = match self.names.fact(key) {
            FactState::Known(resolution) => Some(resolution),
            FactState::Unperformed => {
                match self.names.deferred(key).map(NameLookupPlan::decision) {
                    Some(NameLookupDecision::Resolved(resolution)) => Some(resolution),
                    Some(_) => {
                        return Err(SemanticFingerprintError::IncompleteBodyFact(
                            "deferred name resolution",
                        ));
                    }
                    None => None,
                }
            }
            FactState::NotApplicable => None,
            FactState::Poisoned(_) => {
                return Err(SemanticFingerprintError::IncompleteBodyFact(
                    "name resolution",
                ));
            }
        };
        match resolution {
            Some(ResolvedName::Local(_)) => builder.write_u8(1),
            Some(ResolvedName::Declarations(targets)) => {
                builder.write_u8(2);
                let mut stable = targets
                    .iter()
                    .map(|target| {
                        self.keys.target(target).ok_or_else(|| match target {
                            CanonicalTarget::Source(declaration) => {
                                SemanticFingerprintError::UnknownStableDeclaration(*declaration)
                            }
                            CanonicalTarget::External(_) => {
                                unreachable!("external targets are stable")
                            }
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                stable.sort();
                stable.dedup();
                builder.write_len(stable.len());
                for target in stable {
                    target.write_to(builder);
                }
            }
            None => builder.write_u8(0),
        }
        Ok(())
    }

    fn write_expression_facts(
        &mut self,
        builder: &mut CanonicalFingerprintBuilder,
        expression: ExprId,
    ) -> Result<(), SemanticFingerprintError> {
        let key = ExpressionKey::new(self.module, expression);
        let FactState::Known(typed) = self.body_types.expression(key) else {
            return Err(SemanticFingerprintError::IncompleteBodyFact(
                "expression type",
            ));
        };
        self.keys
            .stable_type(self.signatures, typed.ty(), &self.binders)?
            .write_to(builder);
        builder.write_u8(match typed.normal_continuation() {
            NormalContinuation::Absent => 0,
            NormalContinuation::Present => 1,
        });
        self.write_place(builder, typed.place())?;
        self.write_resolution(
            builder,
            ResolutionKey::Expression {
                module: self.module,
                expression,
            },
        )?;
        self.write_member(builder, key)?;
        self.write_invocation(builder, key)?;
        self.write_checked_numeric(builder, key)?;
        self.write_trait_evidence(builder, key)?;
        Ok(())
    }

    fn write_place(
        &mut self,
        builder: &mut CanonicalFingerprintBuilder,
        place: Option<&TypedPlace>,
    ) -> Result<(), SemanticFingerprintError> {
        let Some(place) = place else {
            builder.write_u8(0);
            return Ok(());
        };
        builder.write_u8(1);
        builder.write_bool(place.is_writable());
        match place.kind() {
            TypedPlaceKind::Local(_) => builder.write_u8(0),
            TypedPlaceKind::StoredField { receiver, field } => {
                builder.write_u8(1);
                builder.write_digest(self.expression(*receiver)?);
                self.keys
                    .target(field)
                    .ok_or_else(|| stable_target_error(field))?
                    .write_to(builder);
            }
            TypedPlaceKind::TupleElement { receiver, index } => {
                builder.write_u8(2);
                builder.write_digest(self.expression(*receiver)?);
                builder.write_len(*index);
            }
            TypedPlaceKind::Dereference { reference } => {
                builder.write_u8(3);
                builder.write_digest(self.expression(*reference)?);
            }
        }
        Ok(())
    }

    fn write_member(
        &self,
        builder: &mut CanonicalFingerprintBuilder,
        key: ExpressionKey,
    ) -> Result<(), SemanticFingerprintError> {
        match self.body_types.member(key) {
            FactState::Known(member) => {
                builder.write_u8(1);
                write_member_access(builder, member, self.signatures, self.keys, &self.binders)?;
            }
            FactState::Unperformed | FactState::NotApplicable => builder.write_u8(0),
            FactState::Poisoned(_) => {
                return Err(SemanticFingerprintError::IncompleteBodyFact(
                    "member access",
                ));
            }
        }
        Ok(())
    }

    fn write_invocation(
        &mut self,
        builder: &mut CanonicalFingerprintBuilder,
        key: ExpressionKey,
    ) -> Result<(), SemanticFingerprintError> {
        match self.body_types.invocation(key) {
            FactState::Known(invocation) => {
                builder.write_u8(1);
                self.write_invocation_fact(builder, invocation)?;
            }
            FactState::Unperformed | FactState::NotApplicable => builder.write_u8(0),
            FactState::Poisoned(_) => {
                return Err(SemanticFingerprintError::IncompleteBodyFact("invocation"));
            }
        }
        Ok(())
    }

    fn write_invocation_fact(
        &mut self,
        builder: &mut CanonicalFingerprintBuilder,
        invocation: &InvocationFact,
    ) -> Result<(), SemanticFingerprintError> {
        match invocation.target() {
            InvocationTarget::TensorConstructor {
                declaration,
                constructor,
                dtype,
            } => {
                builder.write_u8(2);
                self.keys
                    .target(declaration)
                    .ok_or_else(|| stable_target_error(declaration))?
                    .write_to(builder);
                builder.write_u8(match constructor {
                    crate::TensorSourceConstructor::Zeros => 0,
                    crate::TensorSourceConstructor::Ones => 1,
                    crate::TensorSourceConstructor::Nan => 2,
                    crate::TensorSourceConstructor::Rand => 3,
                });
                self.keys
                    .stable_type(self.signatures, *dtype, &self.binders)?
                    .write_to(builder);
            }
            InvocationTarget::Callable(id) => {
                builder.write_u8(0);
                let instance = self
                    .body_types
                    .callable_instance(*id)
                    .ok_or(SemanticFingerprintError::UnknownCallableInstance)?;
                self.write_callable_instance(builder, instance)?;
            }
            InvocationTarget::Constructor {
                declaration,
                substitution,
            } => {
                builder.write_u8(1);
                self.keys
                    .target(declaration)
                    .ok_or_else(|| stable_target_error(declaration))?
                    .write_to(builder);
                self.write_substitution(builder, *substitution)?;
            }
        }
        builder.write_len(invocation.arguments().len());
        for &argument in invocation.arguments() {
            builder.write_digest(self.expression(argument)?);
        }
        builder.write_len(invocation.evaluation_order().len());
        for &argument in invocation.evaluation_order() {
            builder.write_digest(self.expression(argument)?);
        }
        Ok(())
    }

    fn write_callable_instance(
        &self,
        builder: &mut CanonicalFingerprintBuilder,
        instance: &CallableInstance,
    ) -> Result<(), SemanticFingerprintError> {
        let signature = self
            .signatures
            .callable(instance.signature())
            .ok_or(SemanticFingerprintError::UnknownCallableInstance)?;
        self.keys
            .target(signature.declaration())
            .ok_or_else(|| stable_target_error(signature.declaration()))?
            .write_to(builder);
        self.write_substitution(builder, instance.substitution())
    }

    fn write_substitution(
        &self,
        builder: &mut CanonicalFingerprintBuilder,
        substitution: crate::SubstId,
    ) -> Result<(), SemanticFingerprintError> {
        let types = self.signatures.types().substitution(substitution)?;
        builder.write_len(types.len());
        for &ty in types {
            self.keys
                .stable_type(self.signatures, ty, &self.binders)?
                .write_to(builder);
        }
        Ok(())
    }

    fn write_checked_numeric(
        &self,
        builder: &mut CanonicalFingerprintBuilder,
        key: ExpressionKey,
    ) -> Result<(), SemanticFingerprintError> {
        match self.body_types.checked_numeric(key) {
            FactState::Known(operation) => {
                builder.write_u8(1);
                write_checked_numeric_operation(
                    builder,
                    operation,
                    self.signatures,
                    self.keys,
                    &self.binders,
                )?;
            }
            FactState::Unperformed | FactState::NotApplicable => builder.write_u8(0),
            FactState::Poisoned(_) => {
                return Err(SemanticFingerprintError::IncompleteBodyFact(
                    "checked numeric operation",
                ));
            }
        }
        Ok(())
    }

    fn write_trait_evidence(
        &self,
        builder: &mut CanonicalFingerprintBuilder,
        key: ExpressionKey,
    ) -> Result<(), SemanticFingerprintError> {
        match self.trait_evidence.invocation(key) {
            FactState::Known(invocation) => {
                builder.write_u8(1);
                self.write_checked_invocation(builder, invocation)?;
            }
            FactState::Unperformed | FactState::NotApplicable => builder.write_u8(0),
            FactState::Poisoned(_) => {
                return Err(SemanticFingerprintError::IncompleteTraitEvidence);
            }
        }
        Ok(())
    }

    fn write_checked_invocation(
        &self,
        builder: &mut CanonicalFingerprintBuilder,
        invocation: &CheckedInvocation,
    ) -> Result<(), SemanticFingerprintError> {
        let environment = self
            .trait_evidence
            .environment(invocation.evidence_environment())
            .ok_or(SemanticFingerprintError::UnknownEvidenceEnvironment)?;
        let mut entries = environment
            .entries()
            .iter()
            .map(|entry| self.stable_evidence_entry(entry))
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort();
        entries.dedup();
        builder.write_len(entries.len());
        for entry in &entries {
            entry.write_to(builder);
        }

        match invocation.dispatch() {
            CheckedDispatch::Direct(target) => {
                builder.write_u8(0);
                self.keys
                    .target(target)
                    .ok_or_else(|| stable_target_error(target))?
                    .write_to(builder);
            }
            CheckedDispatch::Bound { witness, method } => {
                builder.write_u8(1);
                self.keys
                    .target(method)
                    .ok_or_else(|| stable_target_error(method))?
                    .write_to(builder);
                self.stable_witness(*witness)?.write_to(builder);
            }
            CheckedDispatch::ImplTemplate {
                implementation,
                callable,
                substitution,
            } => {
                builder.write_u8(2);
                self.keys
                    .target(implementation)
                    .ok_or_else(|| stable_target_error(implementation))?
                    .write_to(builder);
                self.keys
                    .target(callable)
                    .ok_or_else(|| stable_target_error(callable))?
                    .write_to(builder);
                self.write_substitution(builder, *substitution)?;
            }
        }
        Ok(())
    }

    fn stable_evidence_entry(
        &self,
        entry: &EvidenceEntry,
    ) -> Result<StableEvidenceEntry, SemanticFingerprintError> {
        let parameter = match entry.parameter() {
            GenericParameterId::Source(declaration) => {
                StableGenericParameter::Source(self.keys.source(*declaration).cloned().ok_or(
                    SemanticFingerprintError::UnknownStableDeclaration(*declaration),
                )?)
            }
            GenericParameterId::Trusted { owner, index } => StableGenericParameter::Trusted {
                owner: owner.clone(),
                index: *index,
            },
        };
        let source = match entry.source() {
            EvidenceSource::Bound(witness) => {
                StableEvidenceSource::Bound(self.stable_witness(*witness)?)
            }
            EvidenceSource::Implementation {
                implementation,
                substitution,
            } => {
                let types = self.signatures.types().substitution(*substitution)?;
                let substitution = types
                    .iter()
                    .map(|&ty| self.keys.stable_type(self.signatures, ty, &self.binders))
                    .collect::<Result<Vec<_>, _>>()?;
                StableEvidenceSource::Implementation {
                    implementation: self
                        .keys
                        .target(implementation)
                        .ok_or_else(|| stable_target_error(implementation))?,
                    substitution: substitution.into_boxed_slice(),
                }
            }
        };
        Ok(StableEvidenceEntry {
            parameter,
            subject: self
                .keys
                .stable_type(self.signatures, entry.subject(), &self.binders)?,
            trait_ref: self.keys.stable_trait_ref(
                self.signatures,
                entry.trait_ref(),
                &self.binders,
            )?,
            source,
            destination: entry
                .destination()
                .map(|witness| self.stable_witness(witness))
                .transpose()?,
        })
    }

    fn stable_witness(
        &self,
        witness: WitnessSlotId,
    ) -> Result<StableWitness, SemanticFingerprintError> {
        let obligation = self
            .trait_environment
            .witness_obligation(witness)
            .ok_or(SemanticFingerprintError::UnknownBoundWitness)?;
        let binders =
            declaration_binder_positions(self.declarations, self.signatures, witness.owner())?;
        Ok(StableWitness {
            owner: self.keys.source(witness.owner()).cloned().ok_or(
                SemanticFingerprintError::UnknownStableDeclaration(witness.owner()),
            )?,
            subject: self
                .keys
                .stable_type(self.signatures, obligation.subject(), &binders)?,
            trait_ref: self.keys.stable_trait_ref(
                self.signatures,
                obligation.trait_ref(),
                &binders,
            )?,
        })
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct StableEvidenceEntry {
    parameter: StableGenericParameter,
    subject: StableType,
    trait_ref: StableTraitRef,
    source: StableEvidenceSource,
    destination: Option<StableWitness>,
}

impl StableEvidenceEntry {
    fn write_to(&self, builder: &mut CanonicalFingerprintBuilder) {
        self.parameter.write_to(builder);
        self.subject.write_to(builder);
        self.trait_ref.write_to(builder);
        self.source.write_to(builder);
        match &self.destination {
            Some(destination) => {
                builder.write_u8(1);
                destination.write_to(builder);
            }
            None => builder.write_u8(0),
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum StableGenericParameter {
    Source(StableDeclarationKey),
    Trusted {
        owner: crate::ExternalDefId,
        index: u32,
    },
}

impl StableGenericParameter {
    fn write_to(&self, builder: &mut CanonicalFingerprintBuilder) {
        match self {
            Self::Source(declaration) => {
                builder.write_u8(0);
                declaration.write_to(builder);
            }
            Self::Trusted { owner, index } => {
                builder.write_u8(1);
                builder.write_str(owner.as_str());
                builder.write_u32(*index);
            }
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum StableEvidenceSource {
    Bound(StableWitness),
    Implementation {
        implementation: StableTargetKey,
        substitution: Box<[StableType]>,
    },
}

impl StableEvidenceSource {
    fn write_to(&self, builder: &mut CanonicalFingerprintBuilder) {
        match self {
            Self::Bound(witness) => {
                builder.write_u8(0);
                witness.write_to(builder);
            }
            Self::Implementation {
                implementation,
                substitution,
            } => {
                builder.write_u8(1);
                implementation.write_to(builder);
                builder.write_len(substitution.len());
                for ty in substitution {
                    ty.write_to(builder);
                }
            }
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct StableWitness {
    owner: StableDeclarationKey,
    subject: StableType,
    trait_ref: StableTraitRef,
}

impl StableWitness {
    fn write_to(&self, builder: &mut CanonicalFingerprintBuilder) {
        self.owner.write_to(builder);
        self.subject.write_to(builder);
        self.trait_ref.write_to(builder);
    }
}

fn write_member_access(
    builder: &mut CanonicalFingerprintBuilder,
    member: &MemberAccess,
    signatures: &DeclarationSignatures,
    keys: &StableDeclarationKeys,
    binders: &BTreeMap<GenericParameterId, u32>,
) -> Result<(), SemanticFingerprintError> {
    keys.target(member.declaration())
        .ok_or_else(|| stable_target_error(member.declaration()))?
        .write_to(builder);
    builder.write_u8(match member.kind() {
        MemberKind::StoredField {
            mutability: Mutability::Immutable,
        } => 0,
        MemberKind::StoredField {
            mutability: Mutability::Mutable,
        } => 1,
        MemberKind::Property => 2,
    });
    keys.stable_type(signatures, member.ty(), binders)?
        .write_to(builder);
    Ok(())
}

fn write_checked_numeric_operation(
    builder: &mut CanonicalFingerprintBuilder,
    operation: &CheckedNumericOperation,
    signatures: &DeclarationSignatures,
    keys: &StableDeclarationKeys,
    binders: &BTreeMap<GenericParameterId, u32>,
) -> Result<(), SemanticFingerprintError> {
    builder.write_u8(match operation.operator() {
        CheckedNumericOperator::Add => 0,
        CheckedNumericOperator::Subtract => 1,
        CheckedNumericOperator::Multiply => 2,
        CheckedNumericOperator::Divide => 3,
        CheckedNumericOperator::Remainder => 4,
        CheckedNumericOperator::Negate => 5,
    });
    keys.stable_type(signatures, operation.result(), binders)?
        .write_to(builder);
    builder.write_len(operation.faults().len());
    for fault in operation.faults() {
        builder.write_u8(match fault {
            NumericFaultCause::Overflow => 0,
            NumericFaultCause::DivisionByZero => 1,
            NumericFaultCause::RemainderByZero => 2,
        });
    }
    Ok(())
}

fn write_expression_digests(
    builder: &mut CanonicalFingerprintBuilder,
    expressions: &[ExprId],
    nodes: &mut NodeHasher<'_>,
) -> Result<(), SemanticFingerprintError> {
    builder.write_len(expressions.len());
    for &expression in expressions {
        builder.write_digest(nodes.expression(expression)?);
    }
    Ok(())
}

fn write_statement_digests(
    builder: &mut CanonicalFingerprintBuilder,
    statements: &[StmtId],
    nodes: &mut NodeHasher<'_>,
) -> Result<(), SemanticFingerprintError> {
    builder.write_len(statements.len());
    for &statement in statements {
        builder.write_digest(nodes.statement(statement)?);
    }
    Ok(())
}

fn write_pattern_digests(
    builder: &mut CanonicalFingerprintBuilder,
    patterns: &[PatternId],
    nodes: &mut NodeHasher<'_>,
) -> Result<(), SemanticFingerprintError> {
    builder.write_len(patterns.len());
    for &pattern in patterns {
        builder.write_digest(nodes.pattern(pattern)?);
    }
    Ok(())
}

fn write_axis_digests(
    builder: &mut CanonicalFingerprintBuilder,
    axes: &[AxisExprId],
    nodes: &mut NodeHasher<'_>,
) -> Result<(), SemanticFingerprintError> {
    builder.write_len(axes.len());
    for &axis in axes {
        builder.write_digest(nodes.axis(axis)?);
    }
    Ok(())
}

fn write_type_digests(
    builder: &mut CanonicalFingerprintBuilder,
    types: &[TypeRefId],
    nodes: &mut NodeHasher<'_>,
) -> Result<(), SemanticFingerprintError> {
    builder.write_len(types.len());
    for &ty in types {
        builder.write_digest(nodes.ty(ty)?);
    }
    Ok(())
}

fn write_optional_expression(
    builder: &mut CanonicalFingerprintBuilder,
    expression: Option<ExprId>,
    nodes: &mut NodeHasher<'_>,
) -> Result<(), SemanticFingerprintError> {
    builder.write_bool(expression.is_some());
    if let Some(expression) = expression {
        builder.write_digest(nodes.expression(expression)?);
    }
    Ok(())
}

fn write_optional_type(
    builder: &mut CanonicalFingerprintBuilder,
    ty: Option<TypeRefId>,
    nodes: &mut NodeHasher<'_>,
) -> Result<(), SemanticFingerprintError> {
    builder.write_bool(ty.is_some());
    if let Some(ty) = ty {
        builder.write_digest(nodes.ty(ty)?);
    }
    Ok(())
}

fn write_call_arguments(
    builder: &mut CanonicalFingerprintBuilder,
    arguments: &[CallArgument],
    nodes: &mut NodeHasher<'_>,
) -> Result<(), SemanticFingerprintError> {
    builder.write_len(arguments.len());
    for argument in arguments {
        write_call_argument(builder, argument, nodes)?;
    }
    Ok(())
}

fn write_call_argument(
    builder: &mut CanonicalFingerprintBuilder,
    argument: &CallArgument,
    nodes: &mut NodeHasher<'_>,
) -> Result<(), SemanticFingerprintError> {
    builder.write_bool(argument.name.is_some());
    if let Some(name) = &argument.name {
        write_hir_name(builder, name);
    }
    builder.write_digest(nodes.expression(argument.value)?);
    Ok(())
}

fn write_pattern_payload(
    builder: &mut CanonicalFingerprintBuilder,
    payload: &PatternPayload,
    nodes: &mut NodeHasher<'_>,
) -> Result<(), SemanticFingerprintError> {
    match payload {
        PatternPayload::Absent => builder.write_u8(0),
        PatternPayload::Positional(elements) => {
            builder.write_u8(1);
            write_pattern_elements(builder, elements, nodes)?;
        }
        PatternPayload::Named(fields) => {
            builder.write_u8(2);
            write_field_patterns(builder, fields, nodes)?;
        }
    }
    Ok(())
}

fn write_pattern_elements(
    builder: &mut CanonicalFingerprintBuilder,
    elements: &[PatternElement],
    nodes: &mut NodeHasher<'_>,
) -> Result<(), SemanticFingerprintError> {
    builder.write_len(elements.len());
    for element in elements {
        match element {
            PatternElement::Pattern(pattern) => {
                builder.write_u8(0);
                builder.write_digest(nodes.pattern(*pattern)?);
            }
            PatternElement::Rest(_) => builder.write_u8(1),
        }
    }
    Ok(())
}

fn write_field_patterns(
    builder: &mut CanonicalFingerprintBuilder,
    fields: &[FieldPattern],
    nodes: &mut NodeHasher<'_>,
) -> Result<(), SemanticFingerprintError> {
    builder.write_len(fields.len());
    for field in fields {
        match field {
            FieldPattern::Rest(_) => builder.write_u8(0),
            FieldPattern::Field { name, pattern, .. } => {
                builder.write_u8(1);
                write_hir_name(builder, name);
                builder.write_bool(pattern.is_some());
                if let Some(pattern) = pattern {
                    builder.write_digest(nodes.pattern(*pattern)?);
                }
            }
        }
    }
    Ok(())
}

fn write_hir_name(builder: &mut CanonicalFingerprintBuilder, name: &Name) {
    builder.write_bool(name.is_valid());
    if let Some(text) = name.text() {
        builder.write_str(text);
    }
    builder.write_bool(name.is_bang());
}

fn write_path(builder: &mut CanonicalFingerprintBuilder, path: &Path) {
    builder.write_len(path.segments().len());
    for segment in path.segments() {
        write_hir_name(builder, segment);
    }
}

fn write_literal(builder: &mut CanonicalFingerprintBuilder, literal: &Literal) {
    builder.write_u8(match literal.kind() {
        LiteralKind::Integer => 0,
        LiteralKind::Float => 1,
        LiteralKind::String => 2,
        LiteralKind::FormatString => 3,
        LiteralKind::Bool => 4,
        LiteralKind::AxisName => 5,
        LiteralKind::AxisIndex => 6,
        LiteralKind::Invalid => 7,
    });
    builder.write_str(literal.spelling());
}

fn write_mutability(builder: &mut CanonicalFingerprintBuilder, mutability: Mutability) {
    builder.write_u8(match mutability {
        Mutability::Immutable => 0,
        Mutability::Mutable => 1,
    });
}

const fn binary_operator_tag(operator: BinaryOperator) -> u8 {
    match operator {
        BinaryOperator::LogicalOr => 0,
        BinaryOperator::LogicalAnd => 1,
        BinaryOperator::Equal => 2,
        BinaryOperator::NotEqual => 3,
        BinaryOperator::Less => 4,
        BinaryOperator::LessEqual => 5,
        BinaryOperator::Greater => 6,
        BinaryOperator::GreaterEqual => 7,
        BinaryOperator::Range => 8,
        BinaryOperator::RangeInclusive => 9,
        BinaryOperator::Add => 10,
        BinaryOperator::Subtract => 11,
        BinaryOperator::Multiply => 12,
        BinaryOperator::Divide => 13,
        BinaryOperator::FractionalDivide => 14,
        BinaryOperator::Remainder => 15,
    }
}

const fn unary_operator_tag(operator: UnaryOperator) -> u8 {
    match operator {
        UnaryOperator::Not => 0,
        UnaryOperator::Negate => 1,
        UnaryOperator::Dereference => 2,
    }
}

const fn call_syntax_tag(syntax: CallSyntax) -> u8 {
    match syntax {
        CallSyntax::Direct => 0,
        CallSyntax::ExplicitTypeArguments => 1,
        CallSyntax::TrailingArgument => 2,
        CallSyntax::PostfixThen => 3,
        CallSyntax::PostfixApply => 4,
    }
}

const fn tensor_family_tag(family: TensorSchemaFamily) -> u8 {
    match family {
        TensorSchemaFamily::View => 0,
        TensorSchemaFamily::Rearrange => 1,
        TensorSchemaFamily::Inflate => 2,
        TensorSchemaFamily::Reduce => 3,
        TensorSchemaFamily::Contract => 4,
        TensorSchemaFamily::Einop => 5,
    }
}

fn stable_target_error(target: &CanonicalTarget) -> SemanticFingerprintError {
    match target {
        CanonicalTarget::Source(declaration) => {
            SemanticFingerprintError::UnknownStableDeclaration(*declaration)
        }
        CanonicalTarget::External(_) => unreachable!("external targets are stable"),
    }
}
