use super::*;
use eglur_sema::{ArgumentPassing, ExpressionKey, InvocationTarget, ReferenceMutability};

#[test]
fn method_invocations_retain_passing_plans_without_retyping_the_receiver_expression() {
    for (expression, passing) in [
        (
            "counter.read()",
            ArgumentPassing::BorrowPlace(ReferenceMutability::Shared),
        ),
        (
            "((counter.read))()",
            ArgumentPassing::BorrowPlace(ReferenceMutability::Shared),
        ),
        (
            "(Counter { value: 7 }).read()",
            ArgumentPassing::BorrowTemporary(ReferenceMutability::Shared),
        ),
        ("shared.read()", ArgumentPassing::Value),
        ("writable.read()", ArgumentPassing::Value),
        ("counter.copy()", ArgumentPassing::Value),
        (
            "counter.set!(7)",
            ArgumentPassing::BorrowPlace(ReferenceMutability::Mutable),
        ),
        ("writable.set!(7)", ArgumentPassing::Value),
    ] {
        let source = format!(
            "struct Counter {{\n\
               field mut value: i64\n\
               fn read(self: &Self) -> i64 = self.value\n\
               fn copy(self: Self) -> i64 = self.value\n\
               fn set!(self: &mut Self, value: i64) -> Unit {{ self.value = value\nyield () }}\n\
             }}\n\
             fn inspect!() -> Unit {{\n\
               let mut counter = Counter {{ value: 7 }}\n\
               let shared = &counter\n\
               let writable = &mut counter\n\
               _ = {expression}\n\
               yield ()\n\
             }}\n"
        );
        let mut session = session(&source);
        let catalog = TrustedCatalog::first_slice().unwrap();
        let analysis = session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        assert_eq!(analysis.trace().missing().count(), 0, "{expression}");
        let resolution = analysis.resolution().unwrap();
        let (key, invocation) = resolution
            .body_types()
            .invocations()
            .find(|(_, invocation)| matches!(invocation.target(), InvocationTarget::Callable(_)))
            .expect("method invocation");
        let receiver = invocation.arguments()[0];
        assert_eq!(receiver.passing(), passing, "{expression}");
        assert!(
            invocation.arguments()[1..]
                .iter()
                .all(|argument| argument.passing() == ArgumentPassing::Value)
        );
        assert_eq!(invocation.evaluation_order().next(), Some(&receiver));

        let FactState::Known(typed) = resolution
            .body_types()
            .expression(ExpressionKey::new(key.module(), receiver.expression()))
        else {
            panic!("receiver expression must retain its own type");
        };
        match passing {
            ArgumentPassing::BorrowPlace(_) => assert!(typed.place().is_some()),
            ArgumentPassing::BorrowTemporary(_) => assert!(typed.place().is_none()),
            ArgumentPassing::Value => {}
        }
        if passing != ArgumentPassing::Value {
            assert!(!matches!(
                resolution.signatures().types().kind(typed.ty()).unwrap(),
                TypeKind::Reference { .. }
            ));
        }
        assert!(resolution.fingerprints().is_some());
        assert!(
            session.seal(None).unwrap().checked_program().is_some(),
            "{source}"
        );
    }
}

#[test]
fn receiver_adjustments_do_not_authorize_nonreceiver_or_invalid_mutable_borrows() {
    for (statement, expected) in [
        ("immutable.set!(7)", "METHOD_NOT_FOUND"),
        ("(Counter { value: 0 }).set!(7)", "METHOD_NOT_FOUND"),
        ("shared.set!(7)", "METHOD_NOT_FOUND"),
        ("take!(counter)", "CALL_ARG_TYPE_MISMATCH"),
        ("_ = &(Counter { value: 0 })", "REFERENCE_TARGET_INVALID"),
    ] {
        let source = format!(
            "struct Counter {{\n\
               field mut value: i64\n\
               fn set!(self: &mut Self, value: i64) -> Unit {{ self.value = value\nyield () }}\n\
             }}\n\
             fn take!(value: &mut Counter) -> Unit = ()\n\
             fn inspect!() -> Unit {{\n\
               let mut counter = Counter {{ value: 0 }}\n\
               let immutable = Counter {{ value: 0 }}\n\
               let shared = &counter\n\
               {statement}\n\
               yield ()\n\
             }}\n"
        );
        let mut session = session(&source);
        let catalog = TrustedCatalog::first_slice().unwrap();
        session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        let sealed = session.seal(None).unwrap();
        assert!(sealed.checked_program().is_none());
        assert_eq!(
            diagnostic_codes(sealed.diagnostics()),
            [expected],
            "{statement}"
        );
    }
}
