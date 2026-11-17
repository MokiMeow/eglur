use eglur_hir::{Mutability, PointerMutability, TypeRefKind};
use eglur_syntax::SyntaxKind;

use super::LoweredTypes;

#[test]
fn generic_arguments_preserve_heterogeneous_children_in_written_order() {
    let spelling = "pkg::Container[Zulu, (Beta, Alpha), Fn(Input, Other) -> Output, dyn iface::View[Key, (Value,)], Tail]";
    let source = LoweredTypes::valid_type(spelling);
    let arguments = source.path(source.alias("Selected"), spelling, "pkg::Container");
    assert_eq!(arguments.len(), 5);
    source.leaf(arguments[0], "Zulu");
    source.leaf(arguments[4], "Tail");

    let TypeRefKind::Tuple(elements) =
        source.kind(arguments[1], SyntaxKind::TupleType, "(Beta, Alpha)")
    else {
        panic!("second argument must remain a tuple");
    };
    assert_eq!(elements.len(), 2);
    source.leaf(elements[0], "Beta");
    source.leaf(elements[1], "Alpha");

    let TypeRefKind::Function { parameters, result } = source.kind(
        arguments[2],
        SyntaxKind::FnType,
        "Fn(Input, Other) -> Output",
    ) else {
        panic!("third argument must remain a function type");
    };
    assert_eq!(parameters.len(), 2);
    source.leaf(parameters[0], "Input");
    source.leaf(parameters[1], "Other");
    source.leaf(*result, "Output");

    let TypeRefKind::Dynamic { trait_ref } = source.kind(
        arguments[3],
        SyntaxKind::DynType,
        "dyn iface::View[Key, (Value,)]",
    ) else {
        panic!("fourth argument must remain dynamic");
    };
    let arguments = source.path(*trait_ref, "iface::View[Key, (Value,)]", "iface::View");
    assert_eq!(arguments.len(), 2);
    source.leaf(arguments[0], "Key");
    let TypeRefKind::Tuple(elements) = source.kind(arguments[1], SyntaxKind::TupleType, "(Value,)")
    else {
        panic!("dynamic trait argument must retain its singleton tuple");
    };
    assert_eq!(elements.len(), 1);
    source.leaf(elements[0], "Value");
}

#[test]
fn reference_and_pointer_layers_keep_their_own_mutability_and_referents() {
    let spelling = "(& *mut Alpha, &mut *const Beta, *const &mut Gamma, *mut &Delta)";
    let source = LoweredTypes::valid_type(spelling);
    let TypeRefKind::Tuple(elements) =
        source.kind(source.alias("Selected"), SyntaxKind::TupleType, spelling)
    else {
        panic!("expected tuple of mixed wrappers");
    };
    assert_eq!(elements.len(), 4);
    for (id, spelling, expected, pointer_spelling, pointer_mutability, name) in [
        (
            elements[0],
            "& *mut Alpha",
            Mutability::Immutable,
            "*mut Alpha",
            PointerMutability::Mutable,
            "Alpha",
        ),
        (
            elements[1],
            "&mut *const Beta",
            Mutability::Mutable,
            "*const Beta",
            PointerMutability::Const,
            "Beta",
        ),
    ] {
        let TypeRefKind::Reference {
            mutability,
            referent,
        } = source.kind(id, SyntaxKind::RefType, spelling)
        else {
            panic!("expected reference wrapper");
        };
        assert_eq!(*mutability, expected);
        let TypeRefKind::Pointer {
            mutability,
            referent,
        } = source.kind(*referent, SyntaxKind::PointerType, pointer_spelling)
        else {
            panic!("reference must retain its pointer child");
        };
        assert_eq!(*mutability, pointer_mutability);
        source.leaf(*referent, name);
    }

    for (id, spelling, expected, reference_spelling, reference_mutability, name) in [
        (
            elements[2],
            "*const &mut Gamma",
            PointerMutability::Const,
            "&mut Gamma",
            Mutability::Mutable,
            "Gamma",
        ),
        (
            elements[3],
            "*mut &Delta",
            PointerMutability::Mutable,
            "&Delta",
            Mutability::Immutable,
            "Delta",
        ),
    ] {
        let TypeRefKind::Pointer {
            mutability,
            referent,
        } = source.kind(id, SyntaxKind::PointerType, spelling)
        else {
            panic!("expected pointer wrapper");
        };
        assert_eq!(*mutability, expected);
        let TypeRefKind::Reference {
            mutability,
            referent,
        } = source.kind(*referent, SyntaxKind::RefType, reference_spelling)
        else {
            panic!("pointer must retain its reference child");
        };
        assert_eq!(*mutability, reference_mutability);
        source.leaf(*referent, name);
    }
}

#[test]
fn groups_and_singleton_tuples_remain_distinct_around_reference_children() {
    for (spelling, syntax, reference_spelling) in [
        ("(&mut Inner)", SyntaxKind::TypeExpr, "&mut Inner"),
        ("(&mut Inner,)", SyntaxKind::TupleType, "&mut Inner"),
    ] {
        let source = LoweredTypes::valid_type(spelling);
        let inner = match (
            syntax,
            source.kind(source.alias("Selected"), syntax, spelling),
        ) {
            (SyntaxKind::TypeExpr, TypeRefKind::Group(inner)) => *inner,
            (SyntaxKind::TupleType, TypeRefKind::Tuple(elements)) => {
                assert_eq!(elements.len(), 1);
                elements[0]
            }
            (_, actual) => panic!("wrong parenthesized type: {actual:?}"),
        };
        let TypeRefKind::Reference {
            mutability,
            referent,
        } = source.kind(inner, SyntaxKind::RefType, reference_spelling)
        else {
            panic!("parenthesized type must retain its reference child");
        };
        assert_eq!(*mutability, Mutability::Mutable);
        source.leaf(*referent, "Inner");
    }
}

#[test]
fn nested_function_types_do_not_mix_outer_parameters_with_either_result() {
    let spelling =
        "Fn(Fn(InnerInput) -> InnerResult, Sibling) -> Fn(ResultLeft, ResultRight) -> Final";
    let source = LoweredTypes::valid_type(spelling);
    let TypeRefKind::Function { parameters, result } =
        source.kind(source.alias("Selected"), SyntaxKind::FnType, spelling)
    else {
        panic!("expected outer function type");
    };
    assert_eq!(parameters.len(), 2);
    source.leaf(parameters[1], "Sibling");
    let TypeRefKind::Function {
        parameters: nested,
        result: nested_result,
    } = source.kind(
        parameters[0],
        SyntaxKind::FnType,
        "Fn(InnerInput) -> InnerResult",
    )
    else {
        panic!("first parameter must remain a function");
    };
    assert_eq!(nested.len(), 1);
    source.leaf(nested[0], "InnerInput");
    source.leaf(*nested_result, "InnerResult");

    let TypeRefKind::Function { parameters, result } = source.kind(
        *result,
        SyntaxKind::FnType,
        "Fn(ResultLeft, ResultRight) -> Final",
    ) else {
        panic!("outer result must remain a function");
    };
    assert_eq!(parameters.len(), 2);
    source.leaf(parameters[0], "ResultLeft");
    source.leaf(parameters[1], "ResultRight");
    source.leaf(*result, "Final");
}

#[test]
fn function_parameter_arity_is_not_tuple_arity() {
    for (spelling, parameter_spellings) in [
        ("Fn() -> Result", &[][..]),
        (
            "Fn((TupleLeft, TupleRight)) -> Result",
            &["(TupleLeft, TupleRight)"][..],
        ),
        (
            "Fn(ParamSecond, ParamFirst) -> Result",
            &["ParamSecond", "ParamFirst"][..],
        ),
    ] {
        let source = LoweredTypes::valid_type(spelling);
        let TypeRefKind::Function { parameters, result } =
            source.kind(source.alias("Selected"), SyntaxKind::FnType, spelling)
        else {
            panic!("expected function type");
        };
        assert_eq!(parameters.len(), parameter_spellings.len());
        source.leaf(*result, "Result");
        for (&parameter, &spelling) in parameters.iter().zip(parameter_spellings) {
            if spelling == "(TupleLeft, TupleRight)" {
                let TypeRefKind::Tuple(elements) =
                    source.kind(parameter, SyntaxKind::TupleType, spelling)
                else {
                    panic!("one tuple parameter must not become two parameters");
                };
                assert_eq!(elements.len(), 2);
                source.leaf(elements[0], "TupleLeft");
                source.leaf(elements[1], "TupleRight");
            } else {
                source.leaf(parameter, spelling);
            }
        }
    }
}

#[test]
fn dynamic_type_without_arguments_retains_its_qualified_trait_path() {
    let source = LoweredTypes::valid_type("dyn api::Readable");
    let TypeRefKind::Dynamic { trait_ref } = source.kind(
        source.alias("Selected"),
        SyntaxKind::DynType,
        "dyn api::Readable",
    ) else {
        panic!("expected dynamic type");
    };
    source.leaf(*trait_ref, "api::Readable");
}
