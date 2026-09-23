use super::*;

pub(super) struct InvalidTraitAdapterDispatch;

pub(super) struct TraitAdapterDispatch {
    pub(super) selected: SelectedCall,
    pub(super) borrow_argument: Option<usize>,
}

pub(super) struct SelectedCall {
    pub(super) target: CanonicalTarget,
    pub(super) method: Option<CanonicalTarget>,
    pub(super) witness: Option<WitnessId>,
}

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

    pub(super) fn callable_signature(
        &self,
        target: &CanonicalTarget,
    ) -> Option<&'a CallableSignature> {
        let signatures = self.program.resolution().signatures();
        let FactState::Known(DeclarationSignature::Callable(signature)) =
            signatures.declaration(target)
        else {
            return None;
        };
        signatures.callable(*signature)
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

    pub(super) fn selected_call(
        &self,
        dispatch: &CheckedDispatch,
        parent: &Witnesses,
        store: &mut WitnessStore,
    ) -> Option<SelectedCall> {
        let evidence = self.trait_evidence()?;
        let traits = self.program.resolution().trait_environment();
        match dispatch {
            CheckedDispatch::Direct(target) => Some(SelectedCall {
                target: target.clone(),
                method: None,
                witness: None,
            }),
            CheckedDispatch::ImplTemplate {
                callable,
                method,
                proof,
            } => {
                let root = store.resolve_proof(evidence, *proof, parent)?;
                let value = store.value(root)?;
                let selected = traits.checked_impl_method(&value.implementation, method)?;
                (selected == callable).then(|| SelectedCall {
                    target: callable.clone(),
                    method: Some(method.clone()),
                    witness: Some(root),
                })
            }
            CheckedDispatch::Bound { witness, method } => {
                let root = *parent.get(witness)?;
                let value = store.value(root)?;
                let target = traits
                    .checked_impl_method(&value.implementation, method)?
                    .clone();
                Some(SelectedCall {
                    target,
                    method: Some(method.clone()),
                    witness: Some(root),
                })
            }
        }
    }

    pub(super) fn trait_adapter_dispatch(
        &self,
        callable: &CanonicalTarget,
        module: ModuleId,
        arguments: &[InvocationArgument],
        environment: &EvidenceEnvironment,
        witnesses: &Witnesses,
        store: &mut WitnessStore,
    ) -> Result<Option<TraitAdapterDispatch>, InvalidTraitAdapterDispatch> {
        let resolution = self.program.resolution();
        let Some(adapter) = resolution.signatures().trait_adapter(callable) else {
            return Ok(None);
        };
        let argument = arguments
            .get(adapter.argument())
            .ok_or(InvalidTraitAdapterDispatch)?;
        let FactState::Known(typed) = resolution
            .body_types()
            .expression(ExpressionKey::new(module, argument.expression()))
        else {
            return Err(InvalidTraitAdapterDispatch);
        };
        let owner = resolution
            .signatures()
            .trait_method_owner(adapter.method())
            .ok_or(InvalidTraitAdapterDispatch)?;
        let evidence = self.trait_evidence().ok_or(InvalidTraitAdapterDispatch)?;
        let mut implementations = environment
            .entries()
            .iter()
            .filter(|entry| {
                entry.subject() == typed.ty() && entry.trait_ref().declaration() == owner
            })
            .map(|entry| {
                let value = store
                    .resolve_proof(evidence, entry.proof(), witnesses)
                    .ok_or(InvalidTraitAdapterDispatch)?;
                Ok((
                    store
                        .value(value)
                        .ok_or(InvalidTraitAdapterDispatch)?
                        .implementation
                        .clone(),
                    value,
                ))
            })
            .collect::<Result<Vec<_>, InvalidTraitAdapterDispatch>>()?;
        implementations.sort();
        implementations.dedup();
        let [(implementation, root)] = implementations.as_slice() else {
            return Err(InvalidTraitAdapterDispatch);
        };
        let target = resolution
            .trait_environment()
            .checked_impl_method(implementation, adapter.method())
            .cloned()
            .ok_or(InvalidTraitAdapterDispatch)?;
        let parameter = self
            .callable_signature(adapter.method())
            .and_then(|signature| signature.parameters().get(adapter.argument()))
            .ok_or(InvalidTraitAdapterDispatch)?;
        let borrow_argument = match resolution.signatures().types().kind(parameter.ty()) {
            Ok(TypeKind::Reference { .. }) => Some(adapter.argument()),
            Ok(_) => None,
            Err(_) => return Err(InvalidTraitAdapterDispatch),
        };
        Ok(Some(TraitAdapterDispatch {
            selected: SelectedCall {
                target,
                method: Some(adapter.method().clone()),
                witness: Some(*root),
            },
            borrow_argument,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct WitnessId(usize);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct WitnessValue {
    pub(super) implementation: CanonicalTarget,
    pub(super) substitution: SubstId,
    pub(super) implementation_bounds: Box<[WitnessId]>,
    pub(super) trait_bounds: Box<[WitnessId]>,
}

pub(super) type Witnesses = BTreeMap<WitnessSlotId, WitnessId>;

#[derive(Default)]
pub(super) struct WitnessStore {
    values: Vec<WitnessValue>,
    ids: BTreeMap<WitnessValue, WitnessId>,
}

impl WitnessStore {
    pub(super) fn value(&self, id: WitnessId) -> Option<&WitnessValue> {
        self.values.get(id.0)
    }

    pub(super) fn resolve_proof(
        &mut self,
        evidence: &TraitEvidence,
        proof: eglur_sema::EvidenceProofId,
        parent: &Witnesses,
    ) -> Option<WitnessId> {
        self.resolve_inner(
            evidence,
            proof,
            parent,
            &mut BTreeMap::new(),
            &mut BTreeSet::new(),
        )
    }

    fn resolve_inner(
        &mut self,
        evidence: &TraitEvidence,
        proof: eglur_sema::EvidenceProofId,
        parent: &Witnesses,
        cache: &mut BTreeMap<eglur_sema::EvidenceProofId, WitnessId>,
        active: &mut BTreeSet<eglur_sema::EvidenceProofId>,
    ) -> Option<WitnessId> {
        if let Some(&id) = cache.get(&proof) {
            return Some(id);
        }
        if !active.insert(proof) {
            return None;
        }
        let id = match evidence.proof(proof)? {
            EvidenceSource::Bound(slot) => *parent.get(slot)?,
            EvidenceSource::Implementation {
                implementation,
                substitution,
                implementation_bounds,
                trait_bounds,
            } => {
                let mut resolve = |bounds: &[eglur_sema::EvidenceProofId]| {
                    bounds
                        .iter()
                        .map(|&child| self.resolve_inner(evidence, child, parent, cache, active))
                        .collect::<Option<Vec<_>>>()
                        .map(Vec::into_boxed_slice)
                };
                let implementation_bounds = resolve(implementation_bounds)?;
                let trait_bounds = resolve(trait_bounds)?;
                self.intern(WitnessValue {
                    implementation: implementation.clone(),
                    substitution: *substitution,
                    implementation_bounds,
                    trait_bounds,
                })
            }
        };
        active.remove(&proof);
        cache.insert(proof, id);
        Some(id)
    }

    fn intern(&mut self, value: WitnessValue) -> WitnessId {
        if let Some(&id) = self.ids.get(&value) {
            return id;
        }
        let id = WitnessId(self.values.len());
        self.values.push(value.clone());
        self.ids.insert(value, id);
        id
    }

    pub(super) fn child_witnesses(
        &mut self,
        evidence: &TraitEvidence,
        environment: &EvidenceEnvironment,
        parent: &Witnesses,
    ) -> Option<Witnesses> {
        let mut child = Witnesses::new();
        let mut cache = BTreeMap::new();
        let mut active = BTreeSet::new();
        for entry in environment.entries() {
            let value =
                self.resolve_inner(evidence, entry.proof(), parent, &mut cache, &mut active)?;
            if let Some(destination) = entry.destination()
                && child.insert(destination, value).is_some()
            {
                return None;
            }
        }
        Some(child)
    }

    pub(super) fn method_frame(
        &mut self,
        evidence: &TraitEvidence,
        selected: &SelectedCall,
        environment: &EvidenceEnvironment,
        parent: &Witnesses,
    ) -> Option<Witnesses> {
        let (CanonicalTarget::Source(target), Some(method), Some(root)) =
            (&selected.target, &selected.method, selected.witness)
        else {
            return None;
        };
        let implementation = self.value(root)?.implementation.clone();
        let route = evidence.method_route(&implementation, method)?;
        if route.target() != *target {
            return None;
        }
        let root_value = self.value(root)?.clone();
        let mut frame = Witnesses::new();
        for binding in route.bindings() {
            let value = match binding.source() {
                eglur_sema::MethodFrameSource::Root => root,
                eglur_sema::MethodFrameSource::ImplementationBound(index) => {
                    *root_value.implementation_bounds.get(*index)?
                }
                eglur_sema::MethodFrameSource::TraitBound(index) => {
                    *root_value.trait_bounds.get(*index)?
                }
                eglur_sema::MethodFrameSource::MethodBound {
                    parameter,
                    bound_index,
                } => {
                    let entry = environment.entries().iter().find(|entry| {
                        entry.parameter() == parameter && entry.bound_index() == *bound_index
                    })?;
                    self.resolve_proof(evidence, entry.proof(), parent)?
                }
            };
            match frame.insert(binding.destination(), value) {
                Some(existing) if existing != value => return None,
                _ => {}
            }
        }
        Some(frame)
    }
}
