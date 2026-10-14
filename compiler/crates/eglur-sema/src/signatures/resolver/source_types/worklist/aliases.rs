use super::*;

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(super) fn prepare_alias_plan(
        &mut self,
        request: AliasRequest,
    ) -> Result<AliasPlan, SignatureFlow> {
        let alias = request.alias;
        let declaration = self
            .collection
            .declaration(alias)
            .expect("alias belongs to collection");
        let module = self
            .collection
            .module(alias.module())
            .expect("alias module belongs to collection");
        let file = module.file();
        let hir = self.source_hir(file)?;
        let item_id = declaration
            .item()
            .ok_or(SignatureResolutionError::MissingDeclarationItem(alias))?;
        let ItemKind::TypeAlias(item) = hir.item(item_id).kind() else {
            return Err(SignatureFlow::Internal(
                SignatureResolutionError::WrongDeclarationItem(alias),
            ));
        };
        let value = item.value;
        let custody = match request.custody {
            AliasCustody::Original => {
                self.original_source_custody(TypeOccurrenceId::new(file, value))?
            }
            AliasCustody::Inherited(custody) => custody,
        };
        self.claim_source_custody(&custody)?;
        #[cfg(test)]
        super::custody_tests::observe(self, TypeOccurrenceId::new(file, value), &custody)?;
        if let Some(state) = self.alias_states.get(&alias).cloned() {
            let result = match state {
                AliasState::Resolving => {
                    let (source, origin) = if let Some((file, origin)) = request.cycle_site {
                        (self.sources[&file], origin)
                    } else {
                        let declaration = self
                            .collection
                            .declaration(alias)
                            .expect("alias belongs to collection");
                        let module = self
                            .collection
                            .module(alias.module())
                            .expect("alias module belongs to collection");
                        (self.sources[&module.file()], declaration.origin())
                    };
                    Resolution::Poisoned(self.source_error(
                        source,
                        origin,
                        "TYPE_ALIAS_CYCLE",
                        "type alias expansion contains a cycle",
                        BTreeMap::new(),
                    )?)
                }
                AliasState::Resolved {
                    generic_parameters,
                    value,
                } => Resolution::Known(AliasTemplate {
                    generic_parameters,
                    value,
                }),
                AliasState::Poisoned(poison) => Resolution::Poisoned(poison),
                AliasState::Unperformed => Resolution::Unperformed,
            };
            return Ok(AliasPlan::Complete(result));
        }
        self.alias_states.insert(alias, AliasState::Resolving);
        let context = self.source_context(alias, false)?;
        let site = Rc::new(SourceSite {
            owner: alias,
            file,
            context,
            custody,
        });
        Ok(AliasPlan::Body {
            request: TypeRequest {
                site,
                type_ref: value,
                position: TypePosition::Ordinary,
            },
            generic_parameters: self.direct_generic_ids(alias),
        })
    }

    pub(super) fn finish_alias_plan(
        &mut self,
        frame: AliasFrame,
        result: Resolution<TypeId>,
    ) -> Result<Resolution<AliasTemplate>, SignatureFlow> {
        self.claim_source_custody(&frame.site.custody)?;
        let generic_parameters = frame.generic_parameters;
        let state = match &result {
            Resolution::Known(value) => AliasState::Resolved {
                generic_parameters: generic_parameters.clone(),
                value: *value,
            },
            Resolution::Poisoned(poison) => AliasState::Poisoned(poison.clone()),
            Resolution::Unperformed => AliasState::Unperformed,
        };
        self.alias_states.insert(frame.alias, state);
        Ok(result.map(|value| AliasTemplate {
            generic_parameters,
            value,
        }))
    }
}
