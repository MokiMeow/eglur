use super::*;
use eglur_hir::Visibility;
use eglur_sema::{
    InherentMemberForm, TrustedInherentMember, TrustedInherentOwner, TrustedInherentOwnerHead,
};

const TRI: &str = "std::prelude::Tri";
const DOMAIN_ERROR: &str = "std::prelude::DomainError";
const SEQ: &str = "std::seq::Seq";

pub(super) struct PrimitiveCatalogContribution {
    pub(super) modules: Vec<ModuleAddress>,
    pub(super) declarations: Vec<ExternalDeclaration>,
    pub(super) prelude: Vec<ExternalDefId>,
    pub(super) signatures: Vec<TrustedDeclarationSignature>,
    pub(super) owners: Vec<TrustedInherentOwner>,
    pub(super) effects: Vec<TrustedEffectSummary>,
    pub(super) inhabitance: Vec<TrustedInhabitanceFact>,
}

pub(super) fn primitive_catalog() -> Result<PrimitiveCatalogContribution, TrustedCatalogError> {
    let prelude = module(["std", "prelude"]);
    let seq = module(["std", "seq"]);
    let tri_id = external_id(TRI);
    let domain_error_id = external_id(DOMAIN_ERROR);
    let seq_id = external_id(SEQ);

    let mut contribution = PrimitiveCatalogContribution {
        modules: vec![
            prelude.clone(),
            module(["std", "logic"]),
            module(["std", "str"]),
            seq.clone(),
        ],
        declarations: vec![
            declaration(
                tri_id.clone(),
                prelude.clone(),
                "Tri",
                false,
                DeclarationKind::Enum,
            )?,
            declaration(
                domain_error_id.clone(),
                prelude,
                "DomainError",
                false,
                DeclarationKind::OpaqueType,
            )?,
            declaration(
                seq_id.clone(),
                seq,
                "Seq",
                false,
                DeclarationKind::OpaqueType,
            )?,
        ],
        prelude: vec![tri_id.clone(), domain_error_id.clone(), seq_id.clone()],
        signatures: vec![
            TrustedDeclarationSignature::new(
                tri_id,
                TrustedSignatureKind::Enum {
                    generic_parameters: Box::new([]),
                    variants: ["True", "False", "Unknown"]
                        .into_iter()
                        .map(|variant| TrustedEnumVariant {
                            id: external_id(&format!("{TRI}::{variant}")),
                            name: name(variant, false),
                            payload: TrustedEnumPayload::Absent,
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                },
            ),
            TrustedDeclarationSignature::new(
                domain_error_id.clone(),
                TrustedSignatureKind::Nominal {
                    generic_parameters: Box::new([]),
                },
            ),
            TrustedDeclarationSignature::new(
                seq_id.clone(),
                TrustedSignatureKind::Nominal {
                    generic_parameters: Box::new([generic("T", [])]),
                },
            ),
        ],
        owners: Vec::new(),
        effects: Vec::new(),
        inhabitance: vec![
            TrustedInhabitanceFact::new(domain_error_id, TrustedNominalInhabitance::Unknown),
            TrustedInhabitanceFact::new(seq_id, TrustedNominalInhabitance::Inhabited),
        ],
    };

    for primitive in PrimitiveType::ALL {
        let members = match primitive {
            PrimitiveType::Bool => bool_members(),
            PrimitiveType::Str => str_members(),
            PrimitiveType::Nat => domain_members(primitive, "to_i64", PrimitiveType::I64),
            PrimitiveType::PositiveNat => domain_members(primitive, "to_nat", PrimitiveType::Nat),
            PrimitiveType::Unit
            | PrimitiveType::I8
            | PrimitiveType::I16
            | PrimitiveType::I32
            | PrimitiveType::I64
            | PrimitiveType::U8
            | PrimitiveType::U16
            | PrimitiveType::U32
            | PrimitiveType::U64
            | PrimitiveType::F16
            | PrimitiveType::Bf16
            | PrimitiveType::F32
            | PrimitiveType::F64
            | PrimitiveType::Complex64
            | PrimitiveType::Complex128 => Vec::new(),
        };
        let mut inventory = Vec::new();
        for (signature, member) in members {
            contribution.effects.push(TrustedEffectSummary::new(
                member.callable.clone(),
                EffectSummary::pure(),
            ));
            contribution.signatures.push(signature);
            inventory.push(member);
        }
        contribution.owners.push(TrustedInherentOwner {
            head: TrustedInherentOwnerHead::Primitive(primitive),
            members: inventory.into_boxed_slice(),
        });
    }

    Ok(contribution)
}

fn bool_members() -> Vec<(TrustedDeclarationSignature, TrustedInherentMember)> {
    let receiver = parameter("self", TrustedTypeExpr::Primitive(PrimitiveType::Bool));
    let tri = nominal(external_id(TRI), []);
    [
        ("to_tri", vec![receiver.clone()]),
        ("and", vec![receiver.clone(), parameter("rhs", tri.clone())]),
        ("or", vec![receiver, parameter("rhs", tri.clone())]),
    ]
    .into_iter()
    .map(|(method, parameters)| {
        primitive_member(
            PrimitiveType::Bool,
            "std::logic",
            method,
            InherentMemberForm::ValueReceiver,
            parameters,
            tri.clone(),
        )
    })
    .collect()
}

fn str_members() -> Vec<(TrustedDeclarationSignature, TrustedInherentMember)> {
    let str_type = TrustedTypeExpr::Primitive(PrimitiveType::Str);
    let shared_str = shared(str_type.clone());
    let bool_type = TrustedTypeExpr::Primitive(PrimitiveType::Bool);
    let seq_str = nominal(external_id(SEQ), [str_type.clone()]);
    let opt_str = intrinsic(IntrinsicNominal::Opt, [str_type.clone()]);

    [
        (
            "len_bytes",
            vec![],
            TrustedTypeExpr::Primitive(PrimitiveType::Nat),
        ),
        ("is_empty", vec![], bool_type.clone()),
        (
            "contains",
            vec![parameter("needle", shared_str.clone())],
            bool_type.clone(),
        ),
        (
            "starts_with",
            vec![parameter("prefix", shared_str.clone())],
            bool_type.clone(),
        ),
        (
            "ends_with",
            vec![parameter("suffix", shared_str.clone())],
            bool_type,
        ),
        ("trim", vec![], str_type.clone()),
        ("trim_start", vec![], str_type.clone()),
        ("trim_end", vec![], str_type.clone()),
        (
            "replace",
            vec![
                parameter("from", shared_str.clone()),
                parameter("to", shared_str.clone()),
            ],
            str_type.clone(),
        ),
        ("lines", vec![], seq_str.clone()),
        (
            "split",
            vec![parameter("sep", shared_str.clone())],
            seq_str.clone(),
        ),
        (
            "split_nonempty",
            vec![parameter("sep", shared_str.clone())],
            seq_str,
        ),
        (
            "strip_prefix",
            vec![parameter("prefix", shared_str.clone())],
            opt_str.clone(),
        ),
        (
            "strip_suffix",
            vec![parameter("suffix", shared_str.clone())],
            opt_str,
        ),
        ("to_lower_ascii", vec![], str_type.clone()),
        ("to_upper_ascii", vec![], str_type),
    ]
    .into_iter()
    .map(|(method, arguments, result)| {
        let mut parameters = vec![parameter("self", shared_str.clone())];
        parameters.extend(arguments);

        primitive_member(
            PrimitiveType::Str,
            "std::str",
            method,
            InherentMemberForm::SharedReceiver,
            parameters,
            result,
        )
    })
    .collect()
}

fn domain_members(
    primitive: PrimitiveType,
    conversion: &str,
    result: PrimitiveType,
) -> Vec<(TrustedDeclarationSignature, TrustedInherentMember)> {
    let owner = TrustedTypeExpr::Primitive(primitive);
    vec![
        primitive_member(
            primitive,
            "std::prelude",
            "from_i64",
            InherentMemberForm::Associated,
            vec![parameter(
                "n",
                TrustedTypeExpr::Primitive(PrimitiveType::I64),
            )],
            intrinsic(
                IntrinsicNominal::Res,
                [owner.clone(), nominal(external_id(DOMAIN_ERROR), [])],
            ),
        ),
        primitive_member(
            primitive,
            "std::prelude",
            conversion,
            InherentMemberForm::ValueReceiver,
            vec![parameter("self", owner)],
            TrustedTypeExpr::Primitive(result),
        ),
    ]
}

fn primitive_member(
    primitive: PrimitiveType,
    module: &str,
    method: &str,
    form: InherentMemberForm,
    parameters: Vec<TrustedCallableParameter>,
    result: TrustedTypeExpr,
) -> (TrustedDeclarationSignature, TrustedInherentMember) {
    let callable = external_id(&format!("{module}::{}.{method}", primitive.spelling()));
    (
        TrustedDeclarationSignature::new(
            callable.clone(),
            TrustedSignatureKind::Callable {
                bang: false,
                generic_parameters: Box::new([]),
                parameters: parameters.into_boxed_slice(),
                result,
                requirements: Box::new([]),
            },
        ),
        TrustedInherentMember {
            callable,
            name: name(method, false),
            owner_pattern: TrustedTypeExpr::Primitive(primitive),
            owner_parameter_count: 0,
            form,
            visibility: Visibility::Public,
            overload: false,
        },
    )
}

fn shared(referent: TrustedTypeExpr) -> TrustedTypeExpr {
    TrustedTypeExpr::Reference {
        mutability: ReferenceMutability::Shared,
        referent: Box::new(referent),
    }
}
