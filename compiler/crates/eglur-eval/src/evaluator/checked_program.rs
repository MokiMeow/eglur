use super::*;

pub(super) struct InvalidTraitAdapterDispatch;

#[derive(Clone, Copy)]
pub(super) struct ProgramView<'a> {
    pub(super) program: &'a CheckedProgram,
}

impl<'a> ProgramView<'a> {
    pub(super) const fn new(program: &'a CheckedProgram) -> Self {
        Self { program }
    }

    fn source_for_module(&self, module: ModuleId) -> Option<&'a eglur_sema::ProgramSource> {
        let file = self
            .program
            .resolution()
            .declarations()
            .module(module)?
            .file();
        self.program
            .sources()
            .iter()
            .find(|source| source.file() == file)
    }

    pub(super) fn callable(
        &self,
        declaration: DefId,
    ) -> Option<(
        &'a eglur_sema::ProgramSource,
        &'a CallableSignature,
        FunctionBody,
    )> {
        let resolution = self.program.resolution();
        let declaration_fact = resolution.declarations().declaration(declaration)?;
        let item = declaration_fact.item()?;
        let source = self.source_for_module(declaration.module())?;
        let ItemKind::Function(function) = source.hir().item(item).kind() else {
            return None;
        };
        let FactState::Known(DeclarationSignature::Callable(signature)) =
            resolution.signatures().source_declaration(declaration)
        else {
            return None;
        };
        Some((
            source,
            resolution.signatures().callable(*signature)?,
            function.body_form,
        ))
    }

    pub(super) fn body(&self, module: ModuleId, body: eglur_hir::BodyId) -> Option<&'a Body> {
        Some(self.source_for_module(module)?.hir().body(body))
    }

    pub(super) fn trait_evidence(&self) -> Option<&'a TraitEvidence> {
        match self.program.resolution().trait_evidence() {
            PhaseProduct::Performed(evidence) => Some(evidence),
            PhaseProduct::Unperformed => None,
        }
    }

    pub(super) fn primitive_type(&self, ty: TypeId) -> Option<PrimitiveType> {
        match self
            .program
            .resolution()
            .signatures()
            .types()
            .kind(ty)
            .ok()?
        {
            TypeKind::Primitive(primitive) => Some(*primitive),
            _ => None,
        }
    }

    pub(super) fn invocation(
        &self,
        module: ModuleId,
        expression: ExprId,
    ) -> Option<(&'a InvocationFact, &'a eglur_sema::CheckedInvocation)> {
        let key = ExpressionKey::new(module, expression);
        let FactState::Known(invocation) = self.program.resolution().body_types().invocation(key)
        else {
            return None;
        };
        let FactState::Known(checked) = self.trait_evidence()?.invocation(key) else {
            return None;
        };
        Some((invocation, checked))
    }

    pub(super) fn trait_adapter_target(
        &self,
        callable: &CanonicalTarget,
        module: ModuleId,
        arguments: &[ExprId],
        environment: &EvidenceEnvironment,
        witnesses: &Witnesses,
    ) -> Result<Option<CanonicalTarget>, InvalidTraitAdapterDispatch> {
        let resolution = self.program.resolution();
        let Some(adapter) = resolution.signatures().trait_adapter(callable) else {
            return Ok(None);
        };
        let argument = arguments
            .get(adapter.argument())
            .ok_or(InvalidTraitAdapterDispatch)?;
        let FactState::Known(typed) = resolution
            .body_types()
            .expression(ExpressionKey::new(module, *argument))
        else {
            return Err(InvalidTraitAdapterDispatch);
        };
        let owner = resolution
            .signatures()
            .trait_method_owner(adapter.method())
            .ok_or(InvalidTraitAdapterDispatch)?;
        let mut implementations = environment
            .entries()
            .iter()
            .filter(|entry| {
                entry.subject() == typed.ty() && entry.trait_ref().declaration() == owner
            })
            .filter_map(|entry| match entry.source() {
                EvidenceSource::Bound(witness) => witnesses
                    .get(witness)
                    .map(|value| value.implementation.clone()),
                EvidenceSource::Implementation { implementation, .. } => {
                    Some(implementation.clone())
                }
            })
            .collect::<Vec<_>>();
        implementations.sort();
        implementations.dedup();
        let [implementation] = implementations.as_slice() else {
            return Err(InvalidTraitAdapterDispatch);
        };
        resolution
            .trait_environment()
            .checked_impl_method(implementation, adapter.method())
            .cloned()
            .map(Some)
            .ok_or(InvalidTraitAdapterDispatch)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct WitnessValue {
    pub(super) implementation: CanonicalTarget,
    pub(super) substitution: SubstId,
}

pub(super) type Witnesses = BTreeMap<WitnessSlotId, WitnessValue>;

pub(super) fn child_witnesses(
    environment: &EvidenceEnvironment,
    parent: &Witnesses,
) -> Option<Witnesses> {
    let mut child = Witnesses::new();
    for entry in environment.entries() {
        let value = match entry.source() {
            EvidenceSource::Bound(witness) => parent.get(witness)?.clone(),
            EvidenceSource::Implementation {
                implementation,
                substitution,
            } => WitnessValue {
                implementation: implementation.clone(),
                substitution: *substitution,
            },
        };
        if let Some(destination) = entry.destination()
            && child.insert(destination, value).is_some()
        {
            return None;
        }
    }
    Some(child)
}
