use std::fmt::Debug;

use super::*;

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Snapshot {
    tables: Vec<BTreeMap<String, String>>,
    backing: [Vec<String>; 4],
}

fn table<K: Ord + Debug, T: Debug>(facts: &FactTable<K, T>) -> BTreeMap<String, String> {
    facts
        .iter()
        .map(|(key, fact)| (format!("{key:?}"), format!("{fact:?}")))
        .collect()
}

fn local_table(facts: &LocalTypeFacts) -> BTreeMap<String, String> {
    facts
        .entries()
        .map(|(key, fact)| (format!("{key:?}"), format!("{fact:?}")))
        .collect()
}

fn sequence<T: Debug>(values: &[T]) -> Vec<String> {
    values.iter().map(|value| format!("{value:?}")).collect()
}

macro_rules! snapshot {
    ($facts:expr) => {{
        let facts = $facts;
        Snapshot {
            tables: vec![
                table(&facts.expressions),
                local_table(&facts.locals),
                table(&facts.members),
                table(&facts.invocations),
                table(&facts.callable_values),
                table(&facts.reduce_by_commitments),
                table(&facts.regions),
                table(&facts.checked_numeric),
                table(&facts.ownership_operations),
                table(&facts.bodies),
            ],
            backing: [
                sequence(&facts.transfers),
                sequence(&facts.callable_instances),
                sequence(&facts.obligations),
                sequence(&facts.method_dispatches),
            ],
        }
    }};
}

impl Snapshot {
    pub(super) fn body(facts: &BodyTypes) -> Self {
        snapshot!(facts)
    }

    pub(super) fn prefix(facts: &BodyTypingPrefix) -> Self {
        snapshot!(facts)
    }

    pub(super) fn checker(facts: &BodyChecker<'_, '_, '_>) -> Self {
        snapshot!(facts)
    }

    pub(super) fn assert_retained_from(&self, complete: &Self) {
        for (prefix, complete) in self.tables.iter().zip(&complete.tables) {
            for (key, fact) in prefix {
                assert_eq!(Some(fact), complete.get(key), "changed prefix fact {key}");
            }
        }
        for (prefix, complete) in self.backing.iter().zip(&complete.backing) {
            for value in prefix {
                assert!(complete.contains(value), "unbacked prefix record {value}");
            }
        }
        // Callable-instance IDs index this sequence, so set membership alone is insufficient.
        assert!(complete.backing[1].starts_with(&self.backing[1]));
    }
}

#[test]
fn finalized_transfer_ranges_keep_all_edges_and_qualified_sources() {
    let error = crate::ExternalDefId::new("fixture::NumericError").unwrap();
    let module = crate::ModuleAddress::new(["fixture".into()]).unwrap();
    let external = ExternalNameEnvironment::new(
        [module.clone()],
        [crate::ExternalDeclaration::new(
            error.clone(),
            module,
            NameKey::new("NumericError", false).unwrap(),
            DeclarationKind::OpaqueType,
        )
        .unwrap()],
        [error.clone()],
    )
    .unwrap();
    let trusted =
        crate::TrustedSignatureEnvironment::new([crate::TrustedDeclarationSignature::new(
            error.clone(),
            crate::TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([]),
            },
        )])
        .unwrap();
    let items =
        crate::TrustedLanguageItemEnvironment::new([(LanguageItem::NumericError, error)]).unwrap();
    let declarations =
        crate::TrustedDeclarationInputs::new(&trusted, None).with_language_items(&items);
    let text = concat!(
        "fn divide(x: i8, y: i8) -> Res[i8, NumericError] = checked { yield x / y }\n",
        "fn option(x: Opt[i8]) -> Opt[i8] = result { let value = x?\nyield x }\n",
        "fn nested(x: i8) -> i8 = { yield { yield x } }\n",
        "fn plain(x: i8) -> i8 = x\n",
    );
    super::super::super::test_support::with_phase_inputs(
        text,
        &external,
        Some(declarations),
        |inputs| {
            let phase = crate::typecheck_bodies(
                inputs.collection,
                inputs.imports,
                inputs.names,
                Some(&external),
                inputs.sources,
                &AllSupported,
                inputs.declarations,
                inputs.trait_environment,
                inputs.diagnostics,
            )
            .unwrap();
            let PhaseProduct::Performed(bodies) = phase.into_parts().0 else {
                panic!("body phase was not performed");
            };
            if inputs.diagnostics.has_errors() {
                let snapshot =
                    std::mem::replace(inputs.diagnostics, DiagnosticStore::new()).finalize(None);
                panic!("invalid source-range fixture: {snapshot:?}");
            }
            let mut sources = bodies
                .transfers()
                .iter()
                .map(|transfer| transfer.source().clone())
                .collect::<BTreeSet<_>>();
            for source in inputs.sources {
                let module = inputs
                    .collection
                    .modules()
                    .find(|module| module.file() == source.file())
                    .unwrap()
                    .id();
                let foreign = ModuleId::from_index(module.index() + 1).unwrap();
                for (_, body) in source.hir().expect("fixture was lowered").bodies() {
                    for (expression, _) in body.expressions() {
                        sources.insert(ControlTransferSource::Expression(ExpressionKey::new(
                            module, expression,
                        )));
                        sources.insert(ControlTransferSource::Expression(ExpressionKey::new(
                            foreign, expression,
                        )));
                    }
                }
            }
            let mut saw_empty = false;
            let mut saw_multiple = false;
            for source in sources {
                let expected = bodies
                    .transfers()
                    .iter()
                    .filter(|transfer| transfer.source() == &source)
                    .collect::<Vec<_>>();
                let actual = bodies.transfers_from(&source);
                assert_eq!(actual.iter().collect::<Vec<_>>(), expected, "{source:?}");
                saw_empty |= actual.is_empty();
                saw_multiple |= actual.len() > 1;
            }
            assert!(saw_empty, "fixture must exercise missing source keys");
            assert!(saw_multiple, "checked division must keep every fault edge");
        },
    );
}
