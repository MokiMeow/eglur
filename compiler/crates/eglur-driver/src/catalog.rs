use std::collections::BTreeMap;
use std::fmt;

use eglur_eval::{RuntimeCatalog, RuntimeHook};
use eglur_sema::{
    DeclarationKind, EffectCategory, EffectSummary, EffectSummaryError, ExternalDeclaration,
    ExternalDefId, ExternalEnvironmentError, ExternalNameEnvironment, ExternalSemanticOwner,
    IntrinsicNominal, KnownEffectSummary, LanguageItem, ModuleAddress, NameKey, PrimitiveType,
    ReferenceMutability, StateScope, TensorSourceConstructor, TrustedCallableParameter,
    TrustedCallableRequirement, TrustedConstructorPayload, TrustedDeclarationSignature,
    TrustedEffectEnvironment, TrustedEffectError, TrustedEffectSummary, TrustedGenericParameter,
    TrustedImplMethod, TrustedInhabitanceEnvironment, TrustedInhabitanceError,
    TrustedInhabitanceFact, TrustedLanguageItemEnvironment, TrustedLanguageItemError,
    TrustedNominalInhabitance, TrustedSignatureEnvironment, TrustedSignatureError,
    TrustedSignatureKind, TrustedTraitAdapter, TrustedTraitExpr, TrustedTraitMethod,
    TrustedTypeExpr,
};

const NUMERIC_ERROR: &str = "std::prelude::NumericError";
const TEXT_ERROR: &str = "std::io::TextError";
const DISPLAY: &str = "std::prelude::Display";
const DISPLAY_TO_STR: &str = "std::prelude::Display.to_str";
const SOME: &str = "std::opt::Some";
const NONE: &str = "std::opt::None";
const OK: &str = "std::res::Ok";
const ERR: &str = "std::res::Err";
const TO_STR: &str = "std::prelude::to_str";
const WRAPPING_ADD: &str = "std::prelude::wrapping_add";
const TEST_EMIT: &str = "std::test::emit!";
const TEST_ABORT: &str = "std::test::abort!";
const IO_PRINT: &str = "std::io::print!";
const TENSOR_ZEROS: &str = "std::tensor::zeros";
const TENSOR: &str = "std::tensor::Tensor";
const TENSOR_ONES: &str = "std::tensor::ones";
const TENSOR_NAN: &str = "std::tensor::nan";
const TENSOR_RAND: &str = "std::tensor::rand!";

/// One validated, partial standard-library slice trusted by the compiler driver.
pub struct TrustedCatalog {
    names: ExternalNameEnvironment,
    signatures: TrustedSignatureEnvironment,
    inhabitance: TrustedInhabitanceEnvironment,
    effects: TrustedEffectEnvironment,
    language_items: TrustedLanguageItemEnvironment,
    runtime_hooks: BTreeMap<ExternalDefId, RuntimeHook>,
}

impl TrustedCatalog {
    /// Builds the exact catalog required by the first executable compiler slice.
    pub fn first_slice() -> Result<Self, TrustedCatalogError> {
        let prelude = module(["std", "prelude"]);
        let opt = module(["std", "opt"]);
        let res = module(["std", "res"]);
        let io = module(["std", "io"]);
        let test = module(["std", "test"]);
        let tensor = module(["std", "tensor"]);

        let numeric_error = external_id(NUMERIC_ERROR);
        let text_error = external_id(TEXT_ERROR);
        let display = external_id(DISPLAY);
        let display_to_str = external_id(DISPLAY_TO_STR);
        let some = external_id(SOME);
        let none = external_id(NONE);
        let ok = external_id(OK);
        let err = external_id(ERR);
        let to_str = external_id(TO_STR);
        let wrapping_add = external_id(WRAPPING_ADD);
        let emit = external_id(TEST_EMIT);
        let abort = external_id(TEST_ABORT);
        let print = external_id(IO_PRINT);
        let zeros = external_id(TENSOR_ZEROS);
        let ones = external_id(TENSOR_ONES);
        let nan = external_id(TENSOR_NAN);
        let rand = external_id(TENSOR_RAND);

        let declarations = vec![
            declaration(
                external_id(TENSOR),
                tensor.clone(),
                "Tensor",
                false,
                DeclarationKind::OpaqueType,
            )?,
            declaration(
                numeric_error.clone(),
                prelude.clone(),
                "NumericError",
                false,
                DeclarationKind::OpaqueType,
            )?,
            declaration(
                text_error.clone(),
                io.clone(),
                "TextError",
                false,
                DeclarationKind::Enum,
            )?,
            declaration(
                display.clone(),
                prelude.clone(),
                "Display",
                false,
                DeclarationKind::Trait,
            )?,
            declaration(
                some.clone(),
                opt.clone(),
                "Some",
                false,
                DeclarationKind::IntrinsicConstructor,
            )?,
            declaration(
                none.clone(),
                opt.clone(),
                "None",
                false,
                DeclarationKind::IntrinsicConstructor,
            )?,
            declaration(
                ok.clone(),
                res.clone(),
                "Ok",
                false,
                DeclarationKind::IntrinsicConstructor,
            )?,
            declaration(
                err.clone(),
                res.clone(),
                "Err",
                false,
                DeclarationKind::IntrinsicConstructor,
            )?,
            declaration(
                to_str.clone(),
                prelude.clone(),
                "to_str",
                false,
                DeclarationKind::Function,
            )?,
            declaration(
                wrapping_add.clone(),
                prelude.clone(),
                "wrapping_add",
                false,
                DeclarationKind::Function,
            )?,
            declaration(
                emit.clone(),
                test.clone(),
                "emit",
                true,
                DeclarationKind::Function,
            )?,
            declaration(
                abort.clone(),
                test.clone(),
                "abort",
                true,
                DeclarationKind::Function,
            )?,
            declaration(
                print.clone(),
                io.clone(),
                "print",
                true,
                DeclarationKind::Function,
            )?,
            tensor_source_declaration(
                zeros,
                tensor.clone(),
                "zeros",
                false,
                TensorSourceConstructor::Zeros,
            )?,
            tensor_source_declaration(
                ones,
                tensor.clone(),
                "ones",
                false,
                TensorSourceConstructor::Ones,
            )?,
            tensor_source_declaration(
                nan,
                tensor.clone(),
                "nan",
                false,
                TensorSourceConstructor::Nan,
            )?,
            tensor_source_declaration(
                rand,
                tensor.clone(),
                "rand",
                true,
                TensorSourceConstructor::Rand,
            )?,
        ];
        let names = ExternalNameEnvironment::partial_standard_library(
            [prelude, opt, res, io, test, tensor],
            declarations,
            [
                numeric_error.clone(),
                display.clone(),
                some.clone(),
                none.clone(),
                ok.clone(),
                err.clone(),
                to_str.clone(),
                wrapping_add.clone(),
            ],
        )?;

        let mut signatures = vec![
            TrustedDeclarationSignature::new(
                numeric_error.clone(),
                TrustedSignatureKind::Nominal {
                    generic_parameters: Box::new([]),
                },
            ),
            TrustedDeclarationSignature::new(
                text_error.clone(),
                TrustedSignatureKind::Nominal {
                    generic_parameters: Box::new([]),
                },
            ),
            TrustedDeclarationSignature::new(
                display.clone(),
                TrustedSignatureKind::Trait {
                    closed: false,
                    generic_parameters: Box::new([]),
                    associated_types: Box::new([]),
                },
            )
            .with_trait_methods([TrustedTraitMethod {
                declaration: display_to_str.clone(),
                name: name("to_str", false),
                has_default: false,
            }]),
            TrustedDeclarationSignature::new(
                display_to_str.clone(),
                TrustedSignatureKind::Callable {
                    bang: false,
                    generic_parameters: Box::new([]),
                    parameters: Box::new([parameter(
                        "self",
                        TrustedTypeExpr::Reference {
                            mutability: ReferenceMutability::Shared,
                            referent: Box::new(TrustedTypeExpr::SelfType(display.clone())),
                        },
                    )]),
                    result: TrustedTypeExpr::Primitive(PrimitiveType::Str),
                    requirements: Box::new([]),
                },
            ),
            positional_constructor(
                some.clone(),
                ["T"],
                [TrustedTypeExpr::GenericParameter(0)],
                intrinsic(
                    IntrinsicNominal::Opt,
                    [TrustedTypeExpr::GenericParameter(0)],
                ),
            ),
            unit_constructor(
                none.clone(),
                ["T"],
                intrinsic(
                    IntrinsicNominal::Opt,
                    [TrustedTypeExpr::GenericParameter(0)],
                ),
            ),
            positional_constructor(
                ok.clone(),
                ["T", "E"],
                [TrustedTypeExpr::GenericParameter(0)],
                intrinsic(
                    IntrinsicNominal::Res,
                    [
                        TrustedTypeExpr::GenericParameter(0),
                        TrustedTypeExpr::GenericParameter(1),
                    ],
                ),
            ),
            positional_constructor(
                err.clone(),
                ["T", "E"],
                [TrustedTypeExpr::GenericParameter(1)],
                intrinsic(
                    IntrinsicNominal::Res,
                    [
                        TrustedTypeExpr::GenericParameter(0),
                        TrustedTypeExpr::GenericParameter(1),
                    ],
                ),
            ),
            TrustedDeclarationSignature::new(
                to_str.clone(),
                TrustedSignatureKind::Callable {
                    bang: false,
                    generic_parameters: Box::new([generic(
                        "T",
                        [TrustedTraitExpr {
                            declaration: display.clone(),
                            arguments: Box::new([]),
                        }],
                    )]),
                    parameters: Box::new([parameter(
                        "value",
                        TrustedTypeExpr::GenericParameter(0),
                    )]),
                    result: TrustedTypeExpr::Primitive(PrimitiveType::Str),
                    requirements: Box::new([]),
                },
            )
            .with_trait_adapter(TrustedTraitAdapter {
                method: display_to_str.clone(),
                argument: 0,
            }),
            TrustedDeclarationSignature::new(
                wrapping_add.clone(),
                TrustedSignatureKind::Callable {
                    bang: false,
                    generic_parameters: Box::new([generic("T", [])]),
                    parameters: Box::new([
                        parameter("lhs", TrustedTypeExpr::GenericParameter(0)),
                        parameter("rhs", TrustedTypeExpr::GenericParameter(0)),
                    ]),
                    result: TrustedTypeExpr::GenericParameter(0),
                    requirements: Box::new([TrustedCallableRequirement::FixedWidthInteger(
                        TrustedTypeExpr::GenericParameter(0),
                    )]),
                },
            ),
            TrustedDeclarationSignature::new(
                emit.clone(),
                TrustedSignatureKind::Callable {
                    bang: true,
                    generic_parameters: Box::new([]),
                    parameters: Box::new([parameter(
                        "s",
                        TrustedTypeExpr::Primitive(PrimitiveType::Str),
                    )]),
                    result: TrustedTypeExpr::Primitive(PrimitiveType::Unit),
                    requirements: Box::new([]),
                },
            ),
            TrustedDeclarationSignature::new(
                abort.clone(),
                TrustedSignatureKind::Callable {
                    bang: true,
                    generic_parameters: Box::new([]),
                    parameters: Box::new([
                        parameter("code", TrustedTypeExpr::Primitive(PrimitiveType::Str)),
                        parameter("message", TrustedTypeExpr::Primitive(PrimitiveType::Str)),
                    ]),
                    result: TrustedTypeExpr::Never,
                    requirements: Box::new([]),
                },
            ),
            TrustedDeclarationSignature::new(
                print.clone(),
                TrustedSignatureKind::Callable {
                    bang: true,
                    generic_parameters: Box::new([]),
                    parameters: Box::new([parameter(
                        "s",
                        TrustedTypeExpr::Primitive(PrimitiveType::Str),
                    )]),
                    result: intrinsic(
                        IntrinsicNominal::Res,
                        [
                            TrustedTypeExpr::Primitive(PrimitiveType::Unit),
                            nominal(text_error, []),
                        ],
                    ),
                    requirements: Box::new([]),
                },
            ),
        ];
        let display_members = display_impls(&display, &display_to_str);
        signatures.extend(display_members.signatures);
        signatures.push(TrustedDeclarationSignature::new(
            external_id(TENSOR),
            TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([TrustedGenericParameter {
                    name: "T".into(),
                    bounds: Box::new([]),
                }]),
            },
        ));
        let signatures = TrustedSignatureEnvironment::new(signatures)?;
        let inhabitance = TrustedInhabitanceEnvironment::new([
            TrustedInhabitanceFact::new(external_id(TENSOR), TrustedNominalInhabitance::Inhabited),
            TrustedInhabitanceFact::new(
                external_id(NUMERIC_ERROR),
                TrustedNominalInhabitance::Unknown,
            ),
            TrustedInhabitanceFact::new(
                external_id(TEXT_ERROR),
                TrustedNominalInhabitance::Inhabited,
            ),
        ])?;

        let mut effects = vec![
            TrustedEffectSummary::new(external_id(TENSOR_ZEROS), EffectSummary::pure()),
            TrustedEffectSummary::new(external_id(TENSOR_ONES), EffectSummary::pure()),
            TrustedEffectSummary::new(external_id(TENSOR_NAN), EffectSummary::pure()),
            TrustedEffectSummary::new(
                external_id(TENSOR_RAND),
                EffectSummary::Known(KnownEffectSummary::new(
                    [EffectCategory::Nondet],
                    StateScope::None,
                )?),
            ),
            TrustedEffectSummary::new(display_to_str, EffectSummary::pure()),
            TrustedEffectSummary::new(to_str.clone(), EffectSummary::pure()),
            TrustedEffectSummary::new(wrapping_add.clone(), EffectSummary::pure()),
            TrustedEffectSummary::new(
                emit.clone(),
                EffectSummary::Known(KnownEffectSummary::new(
                    [EffectCategory::State],
                    StateScope::Nonlocal,
                )?),
            ),
            TrustedEffectSummary::new(abort.clone(), EffectSummary::Unknown),
            TrustedEffectSummary::new(
                print,
                EffectSummary::Known(KnownEffectSummary::new(
                    [EffectCategory::Io],
                    StateScope::None,
                )?),
            ),
        ];
        effects.extend(display_members.effects);
        let effects = TrustedEffectEnvironment::new(effects)?;
        let mut runtime_hooks = vec![
            (some.clone(), RuntimeHook::OptSome),
            (none.clone(), RuntimeHook::OptNone),
            (ok.clone(), RuntimeHook::ResOk),
            (err.clone(), RuntimeHook::ResErr),
            (wrapping_add, RuntimeHook::WrappingAdd),
            (emit, RuntimeHook::TestEmit),
            (abort, RuntimeHook::TestAbort),
        ];
        runtime_hooks.extend(display_members.runtime_hooks);

        let language_items = TrustedLanguageItemEnvironment::new([
            (LanguageItem::NumericError, numeric_error),
            (LanguageItem::Tensor, external_id(TENSOR)),
            (LanguageItem::OptSome, some.clone()),
            (LanguageItem::OptNone, none.clone()),
            (LanguageItem::ResOk, ok.clone()),
            (LanguageItem::ResErr, err.clone()),
        ])?;

        Self::new_with_language_items(
            names,
            signatures,
            inhabitance,
            effects,
            language_items,
            runtime_hooks,
        )
    }

    #[cfg(test)]
    fn new(
        names: ExternalNameEnvironment,
        signatures: TrustedSignatureEnvironment,
        inhabitance: TrustedInhabitanceEnvironment,
        effects: TrustedEffectEnvironment,
        runtime_hooks: impl IntoIterator<Item = (ExternalDefId, RuntimeHook)>,
    ) -> Result<Self, TrustedCatalogError> {
        Self::new_with_language_items(
            names,
            signatures,
            inhabitance,
            effects,
            TrustedLanguageItemEnvironment::new([])?,
            runtime_hooks,
        )
    }

    fn new_with_language_items(
        names: ExternalNameEnvironment,
        signatures: TrustedSignatureEnvironment,
        inhabitance: TrustedInhabitanceEnvironment,
        effects: TrustedEffectEnvironment,
        language_items: TrustedLanguageItemEnvironment,
        runtime_hooks: impl IntoIterator<Item = (ExternalDefId, RuntimeHook)>,
    ) -> Result<Self, TrustedCatalogError> {
        let mut normalized_hooks = BTreeMap::new();
        for (declaration, hook) in runtime_hooks {
            if normalized_hooks.insert(declaration.clone(), hook).is_some() {
                return Err(TrustedCatalogError::DuplicateRuntimeHook(declaration));
            }
        }
        validate_name_signatures(&names, &signatures)?;
        validate_inhabitance(&signatures, &inhabitance)?;
        validate_effects(&names, &signatures, &effects)?;
        validate_trait_adapter_effects(&signatures, &effects)?;
        validate_runtime_hooks(&signatures, &normalized_hooks)?;
        Ok(Self {
            names,
            signatures,
            inhabitance,
            effects,
            language_items,
            runtime_hooks: normalized_hooks,
        })
    }

    #[must_use]
    pub const fn names(&self) -> &ExternalNameEnvironment {
        &self.names
    }

    #[must_use]
    pub const fn signatures(&self) -> &TrustedSignatureEnvironment {
        &self.signatures
    }

    #[must_use]
    pub const fn inhabitance(&self) -> &TrustedInhabitanceEnvironment {
        &self.inhabitance
    }

    #[must_use]
    pub const fn effects(&self) -> &TrustedEffectEnvironment {
        &self.effects
    }

    #[must_use]
    pub const fn language_items(&self) -> &TrustedLanguageItemEnvironment {
        &self.language_items
    }

    #[must_use]
    pub fn runtime_hook(&self, declaration: &ExternalDefId) -> Option<RuntimeHook> {
        self.runtime_hooks.get(declaration).copied()
    }

    pub fn runtime_hooks(&self) -> impl ExactSizeIterator<Item = (&ExternalDefId, RuntimeHook)> {
        self.runtime_hooks.iter().map(|(id, &hook)| (id, hook))
    }
}

impl RuntimeCatalog for TrustedCatalog {
    fn runtime_hook(&self, declaration: &ExternalDefId) -> Option<RuntimeHook> {
        TrustedCatalog::runtime_hook(self, declaration)
    }
}

fn validate_name_signatures(
    names: &ExternalNameEnvironment,
    signatures: &TrustedSignatureEnvironment,
) -> Result<(), TrustedCatalogError> {
    for declaration in names.declarations() {
        match declaration.semantic_owner() {
            ExternalSemanticOwner::General => {
                let signature = signatures.declaration(declaration.id()).ok_or_else(|| {
                    TrustedCatalogError::MissingSignature(declaration.id().clone())
                })?;
                if !matching_kinds(declaration.kind(), signature.kind()) {
                    return Err(TrustedCatalogError::DeclarationKindMismatch(
                        declaration.id().clone(),
                    ));
                }
            }
            ExternalSemanticOwner::TensorSource(_) => {
                if signatures.declaration(declaration.id()).is_some() {
                    return Err(TrustedCatalogError::UnexpectedGeneralSignature(
                        declaration.id().clone(),
                    ));
                }
            }
        }
    }
    for signature in signatures.declarations() {
        let hidden_callable = signatures.is_hidden_callable(signature.id());
        if matches!(signature.kind(), TrustedSignatureKind::Impl { .. }) || hidden_callable {
            if names.declaration(signature.id()).is_some() {
                return Err(TrustedCatalogError::DeclarationKindMismatch(
                    signature.id().clone(),
                ));
            }
        } else {
            match names.declaration(signature.id()) {
                None => return Err(TrustedCatalogError::OrphanSignature(signature.id().clone())),
                Some(declaration)
                    if !matches!(declaration.semantic_owner(), ExternalSemanticOwner::General) =>
                {
                    return Err(TrustedCatalogError::UnexpectedGeneralSignature(
                        signature.id().clone(),
                    ));
                }
                Some(_) => {}
            }
        }
    }
    Ok(())
}

fn matching_kinds(kind: DeclarationKind, signature: &TrustedSignatureKind) -> bool {
    matches!(
        (kind, signature),
        (
            DeclarationKind::Struct | DeclarationKind::Enum | DeclarationKind::OpaqueType,
            TrustedSignatureKind::Nominal { .. }
        ) | (DeclarationKind::Trait, TrustedSignatureKind::Trait { .. })
            | (
                DeclarationKind::Function | DeclarationKind::ExternFunction,
                TrustedSignatureKind::Callable { .. }
            )
            | (
                DeclarationKind::IntrinsicConstructor,
                TrustedSignatureKind::Constructor { .. }
            )
    )
}

fn validate_inhabitance(
    signatures: &TrustedSignatureEnvironment,
    inhabitance: &TrustedInhabitanceEnvironment,
) -> Result<(), TrustedCatalogError> {
    for signature in signatures.declarations() {
        if matches!(signature.kind(), TrustedSignatureKind::Nominal { .. })
            && inhabitance.rule(signature.id()).is_none()
        {
            return Err(TrustedCatalogError::MissingInhabitance(
                signature.id().clone(),
            ));
        }
    }
    for fact in inhabitance.facts() {
        let Some(signature) = signatures.declaration(fact.declaration()) else {
            return Err(TrustedCatalogError::OrphanInhabitance(
                fact.declaration().clone(),
            ));
        };
        if !matches!(signature.kind(), TrustedSignatureKind::Nominal { .. }) {
            return Err(TrustedCatalogError::InhabitanceOnNonNominal(
                fact.declaration().clone(),
            ));
        }
    }
    Ok(())
}

fn validate_effects(
    names: &ExternalNameEnvironment,
    signatures: &TrustedSignatureEnvironment,
    effects: &TrustedEffectEnvironment,
) -> Result<(), TrustedCatalogError> {
    for signature in signatures.declarations() {
        if matches!(signature.kind(), TrustedSignatureKind::Callable { .. })
            && effects.summary(signature.id()).is_none()
        {
            return Err(TrustedCatalogError::MissingEffect(signature.id().clone()));
        }
    }
    for effect in effects.summaries() {
        if names
            .declaration(effect.declaration())
            .is_some_and(|declaration| {
                matches!(
                    declaration.semantic_owner(),
                    ExternalSemanticOwner::TensorSource(_)
                )
            })
        {
            continue;
        }
        let Some(signature) = signatures.declaration(effect.declaration()) else {
            return Err(TrustedCatalogError::OrphanEffect(
                effect.declaration().clone(),
            ));
        };
        if !matches!(signature.kind(), TrustedSignatureKind::Callable { .. }) {
            return Err(TrustedCatalogError::EffectOnNonCallable(
                effect.declaration().clone(),
            ));
        }
    }
    Ok(())
}

fn validate_trait_adapter_effects(
    signatures: &TrustedSignatureEnvironment,
    effects: &TrustedEffectEnvironment,
) -> Result<(), TrustedCatalogError> {
    for adapter in signatures
        .declarations()
        .filter(|declaration| declaration.trait_adapter().is_some())
    {
        let relation = adapter
            .trait_adapter()
            .expect("filtered trusted declaration has an adapter relation");
        let method = signatures
            .declaration(&relation.method)
            .expect("trusted adapter validation resolved its method");
        let TrustedSignatureKind::Callable { bang, .. } = method.kind() else {
            unreachable!("trusted adapter validation requires a callable method")
        };
        let expected = if *bang {
            EffectSummary::Unknown
        } else {
            EffectSummary::pure()
        };
        let actual = effects
            .summary(adapter.id())
            .expect("trusted callable effect completeness was validated");
        if actual != &expected {
            return Err(TrustedCatalogError::TraitAdapterEffectMismatch {
                adapter: adapter.id().clone(),
                method: relation.method.clone(),
            });
        }
    }
    Ok(())
}

fn validate_runtime_hooks(
    signatures: &TrustedSignatureEnvironment,
    hooks: &BTreeMap<ExternalDefId, RuntimeHook>,
) -> Result<(), TrustedCatalogError> {
    for declaration in hooks.keys() {
        let Some(signature) = signatures.declaration(declaration) else {
            return Err(TrustedCatalogError::OrphanRuntimeHook(declaration.clone()));
        };
        if !matches!(
            signature.kind(),
            TrustedSignatureKind::Callable { .. } | TrustedSignatureKind::Constructor { .. }
        ) {
            return Err(TrustedCatalogError::RuntimeHookTargetKind(
                declaration.clone(),
            ));
        }
    }
    Ok(())
}

struct DisplayCatalogMembers {
    signatures: Vec<TrustedDeclarationSignature>,
    effects: Vec<TrustedEffectSummary>,
    runtime_hooks: Vec<(ExternalDefId, RuntimeHook)>,
}

fn display_impls(display: &ExternalDefId, trait_method: &ExternalDefId) -> DisplayCatalogMembers {
    [
        PrimitiveType::Bool,
        PrimitiveType::I8,
        PrimitiveType::I16,
        PrimitiveType::I32,
        PrimitiveType::I64,
        PrimitiveType::U8,
        PrimitiveType::U16,
        PrimitiveType::U32,
        PrimitiveType::U64,
        PrimitiveType::Nat,
        PrimitiveType::PositiveNat,
        PrimitiveType::F32,
        PrimitiveType::F64,
        PrimitiveType::Str,
    ]
    .into_iter()
    .fold(
        DisplayCatalogMembers {
            signatures: Vec::new(),
            effects: Vec::new(),
            runtime_hooks: Vec::new(),
        },
        |mut members, primitive| {
            let implementation = external_id(&format!(
                "std::prelude::impl::Display::{}",
                primitive.spelling()
            ));
            let callable = external_id(&format!(
                "std::prelude::impl::Display::{}.to_str",
                primitive.spelling()
            ));
            members.signatures.push(
                TrustedDeclarationSignature::new(
                    implementation,
                    TrustedSignatureKind::Impl {
                        negative: false,
                        generic_parameters: Box::new([]),
                        trait_ref: TrustedTraitExpr {
                            declaration: display.clone(),
                            arguments: Box::new([]),
                        },
                        target: TrustedTypeExpr::Primitive(primitive),
                        associated_bindings: Box::new([]),
                    },
                )
                .with_impl_methods([TrustedImplMethod {
                    trait_method: trait_method.clone(),
                    callable: callable.clone(),
                }]),
            );
            members.signatures.push(TrustedDeclarationSignature::new(
                callable.clone(),
                TrustedSignatureKind::Callable {
                    bang: false,
                    generic_parameters: Box::new([]),
                    parameters: Box::new([parameter(
                        "self",
                        TrustedTypeExpr::Reference {
                            mutability: ReferenceMutability::Shared,
                            referent: Box::new(TrustedTypeExpr::Primitive(primitive)),
                        },
                    )]),
                    result: TrustedTypeExpr::Primitive(PrimitiveType::Str),
                    requirements: Box::new([]),
                },
            ));
            members.effects.push(TrustedEffectSummary::new(
                callable.clone(),
                EffectSummary::pure(),
            ));
            members.runtime_hooks.push((callable, RuntimeHook::ToStr));
            members
        },
    )
}

fn positional_constructor<const G: usize, const P: usize>(
    id: ExternalDefId,
    generic_names: [&str; G],
    payload: [TrustedTypeExpr; P],
    result: TrustedTypeExpr,
) -> TrustedDeclarationSignature {
    TrustedDeclarationSignature::new(
        id,
        TrustedSignatureKind::Constructor {
            generic_parameters: generic_names
                .into_iter()
                .map(|name| generic(name, []))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            payload: TrustedConstructorPayload::Positional(Box::new(payload)),
            result,
        },
    )
}

fn unit_constructor<const G: usize>(
    id: ExternalDefId,
    generic_names: [&str; G],
    result: TrustedTypeExpr,
) -> TrustedDeclarationSignature {
    TrustedDeclarationSignature::new(
        id,
        TrustedSignatureKind::Constructor {
            generic_parameters: generic_names
                .into_iter()
                .map(|name| generic(name, []))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            payload: TrustedConstructorPayload::Unit,
            result,
        },
    )
}

fn generic<const N: usize>(name: &str, bounds: [TrustedTraitExpr; N]) -> TrustedGenericParameter {
    TrustedGenericParameter {
        name: name.into(),
        bounds: Box::new(bounds),
    }
}

fn parameter(name: &str, ty: TrustedTypeExpr) -> TrustedCallableParameter {
    TrustedCallableParameter {
        name: Some(name.into()),
        ty,
    }
}

fn name(value: &str, bang: bool) -> NameKey {
    NameKey::new(value, bang).expect("static catalog names are valid")
}

fn intrinsic<const N: usize>(
    head: IntrinsicNominal,
    arguments: [TrustedTypeExpr; N],
) -> TrustedTypeExpr {
    TrustedTypeExpr::Intrinsic {
        head,
        arguments: Box::new(arguments),
    }
}

fn nominal<const N: usize>(id: ExternalDefId, arguments: [TrustedTypeExpr; N]) -> TrustedTypeExpr {
    TrustedTypeExpr::Nominal {
        head: id,
        arguments: Box::new(arguments),
    }
}

fn declaration(
    id: ExternalDefId,
    module: ModuleAddress,
    name: &str,
    bang: bool,
    kind: DeclarationKind,
) -> Result<ExternalDeclaration, TrustedCatalogError> {
    ExternalDeclaration::new(
        id,
        module,
        NameKey::new(name, bang).expect("static catalog names are valid"),
        kind,
    )
    .map_err(Into::into)
}

fn tensor_source_declaration(
    id: ExternalDefId,
    module: ModuleAddress,
    name: &str,
    bang: bool,
    constructor: TensorSourceConstructor,
) -> Result<ExternalDeclaration, TrustedCatalogError> {
    ExternalDeclaration::tensor_source(
        id,
        module,
        NameKey::new(name, bang).expect("static catalog names are valid"),
        constructor,
    )
    .map_err(Into::into)
}

fn module<const N: usize>(components: [&str; N]) -> ModuleAddress {
    ModuleAddress::new(components.into_iter().map(Box::<str>::from))
        .expect("static catalog module paths are valid")
}

fn external_id(value: &str) -> ExternalDefId {
    ExternalDefId::new(value).expect("static catalog identities are valid")
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrustedCatalogError {
    ExternalEnvironment(ExternalEnvironmentError),
    Signature(TrustedSignatureError),
    Inhabitance(TrustedInhabitanceError),
    Effect(TrustedEffectError),
    EffectSummary(EffectSummaryError),
    LanguageItem(TrustedLanguageItemError),
    MissingSignature(ExternalDefId),
    OrphanSignature(ExternalDefId),
    UnexpectedGeneralSignature(ExternalDefId),
    DeclarationKindMismatch(ExternalDefId),
    MissingInhabitance(ExternalDefId),
    OrphanInhabitance(ExternalDefId),
    InhabitanceOnNonNominal(ExternalDefId),
    MissingEffect(ExternalDefId),
    OrphanEffect(ExternalDefId),
    EffectOnNonCallable(ExternalDefId),
    TraitAdapterEffectMismatch {
        adapter: ExternalDefId,
        method: ExternalDefId,
    },
    DuplicateRuntimeHook(ExternalDefId),
    OrphanRuntimeHook(ExternalDefId),
    RuntimeHookTargetKind(ExternalDefId),
}

impl fmt::Display for TrustedCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExternalEnvironment(error) => error.fmt(formatter),
            Self::Signature(error) => error.fmt(formatter),
            Self::Inhabitance(error) => error.fmt(formatter),
            Self::Effect(error) => error.fmt(formatter),
            Self::EffectSummary(error) => error.fmt(formatter),
            Self::LanguageItem(error) => error.fmt(formatter),
            Self::MissingSignature(id) => write!(
                formatter,
                "trusted declaration {} has no signature",
                id.as_str()
            ),
            Self::OrphanSignature(id) => write!(
                formatter,
                "trusted signature {} has no name declaration",
                id.as_str()
            ),
            Self::UnexpectedGeneralSignature(id) => write!(
                formatter,
                "trusted declaration {} has a dedicated semantic owner but also a general signature",
                id.as_str()
            ),
            Self::DeclarationKindMismatch(id) => write!(
                formatter,
                "trusted declaration {} disagrees with its signature kind",
                id.as_str()
            ),
            Self::MissingInhabitance(id) => write!(
                formatter,
                "trusted nominal {} has no inhabitance fact",
                id.as_str()
            ),
            Self::OrphanInhabitance(id) => write!(
                formatter,
                "trusted inhabitance fact {} has no signature",
                id.as_str()
            ),
            Self::InhabitanceOnNonNominal(id) => write!(
                formatter,
                "trusted inhabitance fact {} targets a non-nominal declaration",
                id.as_str()
            ),
            Self::MissingEffect(id) => write!(
                formatter,
                "trusted callable {} has no effect summary",
                id.as_str()
            ),
            Self::OrphanEffect(id) => {
                write!(formatter, "trusted effect {} has no signature", id.as_str())
            }
            Self::EffectOnNonCallable(id) => write!(
                formatter,
                "trusted effect {} targets a non-callable declaration",
                id.as_str()
            ),
            Self::TraitAdapterEffectMismatch { adapter, method } => write!(
                formatter,
                "trusted trait adapter {} has an effect summary inconsistent with method {}",
                adapter.as_str(),
                method.as_str()
            ),
            Self::DuplicateRuntimeHook(id) => write!(
                formatter,
                "trusted declaration {} has duplicate runtime hooks",
                id.as_str()
            ),
            Self::OrphanRuntimeHook(id) => write!(
                formatter,
                "trusted runtime hook {} has no signature",
                id.as_str()
            ),
            Self::RuntimeHookTargetKind(id) => write!(
                formatter,
                "trusted runtime hook {} targets an unsupported declaration kind",
                id.as_str()
            ),
        }
    }
}

impl std::error::Error for TrustedCatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ExternalEnvironment(error) => Some(error),
            Self::Signature(error) => Some(error),
            Self::Inhabitance(error) => Some(error),
            Self::Effect(error) => Some(error),
            Self::EffectSummary(error) => Some(error),
            Self::LanguageItem(error) => Some(error),
            Self::MissingSignature(_)
            | Self::OrphanSignature(_)
            | Self::UnexpectedGeneralSignature(_)
            | Self::DeclarationKindMismatch(_)
            | Self::MissingInhabitance(_)
            | Self::OrphanInhabitance(_)
            | Self::InhabitanceOnNonNominal(_)
            | Self::MissingEffect(_)
            | Self::OrphanEffect(_)
            | Self::EffectOnNonCallable(_)
            | Self::TraitAdapterEffectMismatch { .. }
            | Self::DuplicateRuntimeHook(_)
            | Self::OrphanRuntimeHook(_)
            | Self::RuntimeHookTargetKind(_) => None,
        }
    }
}

impl From<ExternalEnvironmentError> for TrustedCatalogError {
    fn from(error: ExternalEnvironmentError) -> Self {
        Self::ExternalEnvironment(error)
    }
}

impl From<TrustedSignatureError> for TrustedCatalogError {
    fn from(error: TrustedSignatureError) -> Self {
        Self::Signature(error)
    }
}

impl From<TrustedInhabitanceError> for TrustedCatalogError {
    fn from(error: TrustedInhabitanceError) -> Self {
        Self::Inhabitance(error)
    }
}

impl From<TrustedEffectError> for TrustedCatalogError {
    fn from(error: TrustedEffectError) -> Self {
        Self::Effect(error)
    }
}

impl From<EffectSummaryError> for TrustedCatalogError {
    fn from(error: EffectSummaryError) -> Self {
        Self::EffectSummary(error)
    }
}

impl From<TrustedLanguageItemError> for TrustedCatalogError {
    fn from(error: TrustedLanguageItemError) -> Self {
        Self::LanguageItem(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RuntimeHook, TrustedCatalog, TrustedCatalogError, declaration, external_id, module,
        tensor_source_declaration, validate_trait_adapter_effects,
    };
    use eglur_sema::{
        DeclarationKind, EffectSummary, ExternalNameEnvironment, NameKey, PrimitiveType,
        ReferenceMutability, TensorSourceConstructor, TrustedCallableParameter,
        TrustedDeclarationSignature, TrustedEffectEnvironment, TrustedEffectSummary,
        TrustedGenericParameter, TrustedInhabitanceEnvironment, TrustedInhabitanceFact,
        TrustedNominalInhabitance, TrustedSignatureEnvironment, TrustedSignatureKind,
        TrustedTraitAdapter, TrustedTraitExpr, TrustedTraitMethod, TrustedTypeExpr,
    };

    fn callable(id: eglur_sema::ExternalDefId) -> TrustedDeclarationSignature {
        TrustedDeclarationSignature::new(
            id,
            TrustedSignatureKind::Callable {
                bang: false,
                generic_parameters: Box::new([]),
                parameters: Box::<[TrustedCallableParameter]>::default(),
                result: TrustedTypeExpr::Never,
                requirements: Box::new([]),
            },
        )
    }

    fn nominal(id: eglur_sema::ExternalDefId) -> TrustedDeclarationSignature {
        TrustedDeclarationSignature::new(
            id,
            TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([]),
            },
        )
    }

    #[test]
    fn aggregate_rejects_trait_adapter_effect_summary_drift() {
        let trait_id = external_id("std::prelude::Display");
        let method_id = external_id("std::prelude::Display.to_str");
        let adapter_id = external_id("std::prelude::to_str");
        let signatures = TrustedSignatureEnvironment::new([
            TrustedDeclarationSignature::new(
                trait_id.clone(),
                TrustedSignatureKind::Trait {
                    closed: false,
                    generic_parameters: Box::new([]),
                    associated_types: Box::new([]),
                },
            )
            .with_trait_methods([TrustedTraitMethod {
                declaration: method_id.clone(),
                name: NameKey::new("to_str", false).unwrap(),
                has_default: false,
            }]),
            TrustedDeclarationSignature::new(
                method_id.clone(),
                TrustedSignatureKind::Callable {
                    bang: false,
                    generic_parameters: Box::new([]),
                    parameters: Box::new([TrustedCallableParameter {
                        name: Some("self".into()),
                        ty: TrustedTypeExpr::Reference {
                            mutability: ReferenceMutability::Shared,
                            referent: Box::new(TrustedTypeExpr::SelfType(trait_id.clone())),
                        },
                    }]),
                    result: TrustedTypeExpr::Primitive(PrimitiveType::Str),
                    requirements: Box::new([]),
                },
            ),
            TrustedDeclarationSignature::new(
                adapter_id.clone(),
                TrustedSignatureKind::Callable {
                    bang: false,
                    generic_parameters: Box::new([TrustedGenericParameter {
                        name: "T".into(),
                        bounds: Box::new([TrustedTraitExpr {
                            declaration: trait_id,
                            arguments: Box::new([]),
                        }]),
                    }]),
                    parameters: Box::new([TrustedCallableParameter {
                        name: Some("value".into()),
                        ty: TrustedTypeExpr::GenericParameter(0),
                    }]),
                    result: TrustedTypeExpr::Primitive(PrimitiveType::Str),
                    requirements: Box::new([]),
                },
            )
            .with_trait_adapter(TrustedTraitAdapter {
                method: method_id.clone(),
                argument: 0,
            }),
        ])
        .unwrap();
        let effects = TrustedEffectEnvironment::new([
            TrustedEffectSummary::new(method_id.clone(), EffectSummary::pure()),
            TrustedEffectSummary::new(adapter_id.clone(), EffectSummary::Unknown),
        ])
        .unwrap();

        assert_eq!(
            validate_trait_adapter_effects(&signatures, &effects),
            Err(TrustedCatalogError::TraitAdapterEffectMismatch {
                adapter: adapter_id,
                method: method_id,
            })
        );
    }

    #[test]
    fn aggregate_rejects_cross_table_drift_and_missing_prerequisites() {
        let id = external_id("std::prelude::sample");
        let prelude = module(["std", "prelude"]);
        let names = ExternalNameEnvironment::partial_standard_library(
            [prelude.clone()],
            [declaration(
                id.clone(),
                prelude.clone(),
                "sample",
                false,
                DeclarationKind::Function,
            )
            .unwrap()],
            [],
        )
        .unwrap();
        assert!(matches!(
            TrustedCatalog::new(
                names,
                TrustedSignatureEnvironment::new([]).unwrap(),
                TrustedInhabitanceEnvironment::new([]).unwrap(),
                TrustedEffectEnvironment::new([]).unwrap(),
                [],
            ),
            Err(TrustedCatalogError::MissingSignature(found)) if found == id
        ));

        let names = ExternalNameEnvironment::partial_standard_library(
            [prelude.clone()],
            [declaration(
                id.clone(),
                prelude.clone(),
                "sample",
                false,
                DeclarationKind::Trait,
            )
            .unwrap()],
            [],
        )
        .unwrap();
        assert!(matches!(
            TrustedCatalog::new(
                names,
                TrustedSignatureEnvironment::new([callable(id.clone())]).unwrap(),
                TrustedInhabitanceEnvironment::new([]).unwrap(),
                TrustedEffectEnvironment::new([TrustedEffectSummary::new(
                    id.clone(),
                    EffectSummary::pure(),
                )])
                .unwrap(),
                [],
            ),
            Err(TrustedCatalogError::DeclarationKindMismatch(found)) if found == id
        ));

        let names = ExternalNameEnvironment::partial_standard_library(
            [prelude.clone()],
            [declaration(
                id.clone(),
                prelude,
                "sample",
                false,
                DeclarationKind::Function,
            )
            .unwrap()],
            [],
        )
        .unwrap();
        assert!(matches!(
            TrustedCatalog::new(
                names,
                TrustedSignatureEnvironment::new([callable(id.clone())]).unwrap(),
                TrustedInhabitanceEnvironment::new([]).unwrap(),
                TrustedEffectEnvironment::new([]).unwrap(),
                [],
            ),
            Err(TrustedCatalogError::MissingEffect(found)) if found == id
        ));
    }

    #[test]
    fn aggregate_keeps_tensor_source_semantics_out_of_general_signatures() {
        let id = external_id("std::tensor::ones");
        let tensor = module(["std", "tensor"]);
        let names = || {
            ExternalNameEnvironment::partial_standard_library(
                [tensor.clone()],
                [tensor_source_declaration(
                    id.clone(),
                    tensor.clone(),
                    "ones",
                    false,
                    TensorSourceConstructor::Ones,
                )
                .unwrap()],
                [],
            )
            .unwrap()
        };

        assert!(
            TrustedCatalog::new(
                names(),
                TrustedSignatureEnvironment::new([]).unwrap(),
                TrustedInhabitanceEnvironment::new([]).unwrap(),
                TrustedEffectEnvironment::new([]).unwrap(),
                [],
            )
            .is_ok()
        );
        assert!(matches!(
            TrustedCatalog::new(
                names(),
                TrustedSignatureEnvironment::new([callable(id.clone())]).unwrap(),
                TrustedInhabitanceEnvironment::new([]).unwrap(),
                TrustedEffectEnvironment::new([]).unwrap(),
                [],
            ),
            Err(TrustedCatalogError::UnexpectedGeneralSignature(found)) if found == id
        ));
    }

    #[test]
    fn aggregate_rejects_orphan_and_duplicate_runtime_records() {
        let id = external_id("std::prelude::orphan");
        let empty_names = || ExternalNameEnvironment::partial_standard_library([], [], []).unwrap();
        let empty_signatures = || TrustedSignatureEnvironment::new([]).unwrap();

        assert!(matches!(
            TrustedCatalog::new(
                empty_names(),
                empty_signatures(),
                TrustedInhabitanceEnvironment::new([]).unwrap(),
                TrustedEffectEnvironment::new([TrustedEffectSummary::new(
                    id.clone(),
                    EffectSummary::pure(),
                )])
                .unwrap(),
                [],
            ),
            Err(TrustedCatalogError::OrphanEffect(found)) if found == id
        ));
        assert!(matches!(
            TrustedCatalog::new(
                empty_names(),
                empty_signatures(),
                TrustedInhabitanceEnvironment::new([]).unwrap(),
                TrustedEffectEnvironment::new([]).unwrap(),
                [(id.clone(), RuntimeHook::ToStr)],
            ),
            Err(TrustedCatalogError::OrphanRuntimeHook(found)) if found == id
        ));
        assert!(matches!(
            TrustedCatalog::new(
                empty_names(),
                empty_signatures(),
                TrustedInhabitanceEnvironment::new([]).unwrap(),
                TrustedEffectEnvironment::new([]).unwrap(),
                [
                    (id.clone(), RuntimeHook::ToStr),
                    (id.clone(), RuntimeHook::WrappingAdd),
                ],
            ),
            Err(TrustedCatalogError::DuplicateRuntimeHook(found)) if found == id
        ));
    }

    #[test]
    fn aggregate_rejects_inhabitance_cross_table_drift() {
        let id = external_id("std::prelude::Value");
        let prelude = module(["std", "prelude"]);
        let names = ExternalNameEnvironment::partial_standard_library(
            [prelude.clone()],
            [declaration(
                id.clone(),
                prelude.clone(),
                "Value",
                false,
                DeclarationKind::OpaqueType,
            )
            .unwrap()],
            [],
        )
        .unwrap();
        assert!(matches!(
            TrustedCatalog::new(
                names,
                TrustedSignatureEnvironment::new([nominal(id.clone())]).unwrap(),
                TrustedInhabitanceEnvironment::new([]).unwrap(),
                TrustedEffectEnvironment::new([]).unwrap(),
                [],
            ),
            Err(TrustedCatalogError::MissingInhabitance(found)) if found == id
        ));

        let fact = TrustedInhabitanceFact::new(id.clone(), TrustedNominalInhabitance::Unknown);
        assert!(matches!(
            TrustedCatalog::new(
                ExternalNameEnvironment::partial_standard_library([], [], []).unwrap(),
                TrustedSignatureEnvironment::new([]).unwrap(),
                TrustedInhabitanceEnvironment::new([fact]).unwrap(),
                TrustedEffectEnvironment::new([]).unwrap(),
                [],
            ),
            Err(TrustedCatalogError::OrphanInhabitance(found)) if found == id
        ));

        let names = ExternalNameEnvironment::partial_standard_library(
            [prelude.clone()],
            [declaration(
                id.clone(),
                prelude,
                "value",
                false,
                DeclarationKind::Function,
            )
            .unwrap()],
            [],
        )
        .unwrap();
        assert!(matches!(
            TrustedCatalog::new(
                names,
                TrustedSignatureEnvironment::new([callable(id.clone())]).unwrap(),
                TrustedInhabitanceEnvironment::new([TrustedInhabitanceFact::new(
                    id.clone(),
                    TrustedNominalInhabitance::Inhabited,
                )])
                .unwrap(),
                TrustedEffectEnvironment::new([TrustedEffectSummary::new(
                    id.clone(),
                    EffectSummary::pure(),
                )])
                .unwrap(),
                [],
            ),
            Err(TrustedCatalogError::InhabitanceOnNonNominal(found)) if found == id
        ));
    }
}
