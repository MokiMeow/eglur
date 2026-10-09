use super::*;
use eglur_source::FileId;

struct Support(bool);

impl CompileSupport for Support {
    fn supports(&self, _: CompileCapability) -> bool {
        self.0
    }
}

fn input(owner: &str) -> UnavailableCompileInput {
    UnavailableCompileInput::TrustedInherentInventory {
        owner: ExternalDefId::new(owner).unwrap(),
    }
}

fn location(file: u32) -> SemanticLocation {
    SemanticLocation::source(FileId::new(file))
}

#[test]
fn implemented_operation_can_refuse_missing_input_without_a_missing_capability() {
    let support = Support(true);
    let mut recorder = DemandRecorder::new(&support);
    let unavailable = input("catalog::Reader");
    assert!(recorder.require(unavailable.capability(), location(0)));
    recorder.refuse_input(unavailable.clone(), location(1));
    let (_, demands) = recorder
        .finish(PhaseProduct::<()>::Unperformed)
        .into_parts();
    let trace = CapabilityTrace::from_demands(&support, demands).unwrap();

    assert!(!trace.has_missing());
    assert!(trace.has_unsupported());
    assert_eq!(trace.missing().count(), 0);
    let entry = trace.unsupported().next().unwrap();
    assert_eq!(entry.disposition(), DemandDisposition::Satisfied);
    assert_eq!(entry.locations(), [location(0), location(1)]);
    assert_eq!(entry.input_refusals()[0].input(), &unavailable);
    assert_eq!(entry.input_refusals()[0].locations(), [location(1)]);
}

#[test]
fn value_member_inventory_refusal_keeps_implemented_member_lookup_selected() {
    let support = Support(true);
    let mut recorder = DemandRecorder::new(&support);
    let unavailable = UnavailableCompileInput::TrustedValueMemberInventory {
        owner: ExternalDefId::new("catalog::Cell").unwrap(),
    };
    let capability = CompileCapability::Typecheck(TypecheckCapability::ResolveMembers);
    assert!(recorder.require(capability, location(0)));
    recorder.refuse_input(unavailable.clone(), location(1));
    recorder.refuse_input(unavailable.clone(), location(1));
    let (_, demands) = recorder
        .finish(PhaseProduct::<()>::Unperformed)
        .into_parts();
    let trace = CapabilityTrace::from_demands(&support, demands).unwrap();

    assert!(!trace.has_missing());
    let entry = trace.unsupported().next().unwrap();
    assert_eq!(entry.capability(), capability);
    assert_eq!(entry.input_refusals()[0].input(), &unavailable);
    assert_eq!(entry.input_refusals()[0].locations(), [location(1)]);
}

#[test]
fn input_identity_and_locations_survive_duplicates_and_permutations() {
    let support = Support(true);
    let mut recorder = DemandRecorder::new(&support);
    recorder.require(input("catalog::Reader").capability(), location(4));
    recorder.refuse_input(input("catalog::Reader"), location(1));
    recorder.refuse_input(input("catalog::Writer"), location(2));
    recorder.refuse_input(input("catalog::Reader"), location(3));
    recorder.refuse_input(input("catalog::Reader"), location(1));
    let (_, mut demands) = recorder
        .finish(PhaseProduct::<()>::Unperformed)
        .into_parts();
    let expected = CapabilityTrace::from_demands(&support, demands.iter().cloned()).unwrap();
    demands.reverse();
    assert_eq!(
        CapabilityTrace::from_demands(&support, demands).unwrap(),
        expected
    );

    let refusals = expected.entries()[0].input_refusals();
    assert_eq!(refusals.len(), 2);
    assert_eq!(refusals[0].input(), &input("catalog::Reader"));
    assert_eq!(refusals[0].locations(), [location(1), location(3)]);
    assert_eq!(refusals[1].input(), &input("catalog::Writer"));
    assert_eq!(refusals[1].locations(), [location(2)]);
}

#[test]
fn unavailable_input_does_not_relax_profile_drift_validation() {
    for selected in [false, true] {
        let support = Support(selected);
        let mut recorder = DemandRecorder::new(&support);
        recorder.refuse_input(input("catalog::Reader"), location(0));
        let (_, demands) = recorder
            .finish(PhaseProduct::<()>::Unperformed)
            .into_parts();
        let trace = CapabilityTrace::from_demands(&support, demands.iter().cloned()).unwrap();
        assert_eq!(trace.has_missing(), !selected);
        assert!(trace.has_unsupported());
        assert_eq!(trace.entries()[0].input_refusals().len(), 1);
        assert!(matches!(
            CapabilityTrace::from_demands(&Support(!selected), demands),
            Err(CapabilityTraceError::SupportMismatch { .. })
        ));
    }
}

#[test]
fn available_input_and_empty_trace_do_not_invent_refusal() {
    let support = Support(true);
    let empty = CapabilityTrace::from_demands(&support, []).unwrap();
    assert!(!empty.has_unsupported());
    let mut recorder = DemandRecorder::new(&support);
    recorder.require(input("catalog::Reader").capability(), location(0));
    let (_, demands) = recorder.finish(PhaseProduct::Performed(())).into_parts();
    let trace = CapabilityTrace::from_demands(&support, demands).unwrap();
    assert!(!trace.has_unsupported());
    assert!(trace.entries()[0].input_refusals().is_empty());
}
