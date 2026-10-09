use eglur_driver::{
    CapabilityTrace, CapabilityTraceError, ExecutionCapability, SupportManifest, SupportProfile,
    SupportProfileError, SupportProfileId, SupportSchemaVersion, UnsupportedInventory,
};
use eglur_sema::{
    CompileCapability, DemandDisposition, ModuleAddress, PhaseProduct, ResolveCapability,
    SemanticSource, collect_declarations,
};
use eglur_source::FileId;

fn profile(
    capabilities: impl IntoIterator<Item = CompileCapability>,
) -> Result<SupportProfile, SupportProfileError> {
    SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("semantic-core-v1").unwrap(),
        capabilities,
    )
}

fn source(file: u32, module: &str) -> SemanticSource<'static> {
    SemanticSource::new(
        FileId::new(file),
        ModuleAddress::new([Box::<str>::from(module)]).unwrap(),
        None,
        [],
        [],
        None,
    )
    .unwrap()
}

#[test]
fn support_profile_rejects_unimplemented_and_prerequisite_incomplete_sets() {
    let qualified = CompileCapability::Resolve(ResolveCapability::ResolveQualifiedValues);
    assert!(matches!(
        profile([qualified]),
        Err(SupportProfileError::MissingPrerequisite { .. })
    ));

    let attributes = CompileCapability::Resolve(ResolveCapability::ResolveAttributePaths);
    let collect = CompileCapability::Resolve(ResolveCapability::CollectDeclarations);
    assert!(matches!(
        profile([collect, attributes]),
        Err(SupportProfileError::UnimplementedCapability(capability))
            if capability == attributes
    ));

    assert!(matches!(
        SupportProfile::new(
            SupportSchemaVersion::V1,
            SupportProfileId::new("invalid-execution-v1").unwrap(),
            [],
            [ExecutionCapability::TrustedScalarHooks],
        ),
        Err(SupportProfileError::MissingExecutionPrerequisite { .. })
    ));
}

#[test]
fn support_manifest_projects_selected_execution_capabilities_in_stable_order() {
    let profile = SupportProfile::new(
        SupportSchemaVersion::V1,
        SupportProfileId::new("execution-v1").unwrap(),
        [],
        [
            ExecutionCapability::Branching,
            ExecutionCapability::CoreValues,
            ExecutionCapability::Calls,
            ExecutionCapability::TrustedScalarHooks,
        ],
    )
    .unwrap();
    let manifest = SupportManifest::from_profile(&profile);

    assert_eq!(
        manifest
            .execute()
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<&str>>(),
        [
            "execute.branching",
            "execute.calls",
            "execute.core_values",
            "execute.trusted_scalar_hooks",
        ]
    );
}

#[test]
fn support_manifest_is_a_sorted_exact_projection_with_no_invented_execution_support() {
    let collect = CompileCapability::Resolve(ResolveCapability::CollectDeclarations);
    let external = CompileCapability::Resolve(ResolveCapability::ResolveTrustedExternalNames);
    let qualified = CompileCapability::Resolve(ResolveCapability::ResolveQualifiedValues);
    let profile = profile([qualified, collect, external, collect]).unwrap();
    let manifest = SupportManifest::from_profile(&profile);

    assert_eq!(manifest.schema(), "eglur.support.v1");
    assert_eq!(manifest.profile(), "semantic-core-v1");
    assert_eq!(
        manifest
            .compile()
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<&str>>(),
        [
            "resolve.collect_declarations",
            "resolve.qualified_values",
            "resolve.trusted_external_names",
        ]
    );
    assert!(manifest.execute().is_empty());
}

#[test]
fn capability_trace_deduplicates_locations_and_rejects_profile_drift() {
    let collect = CompileCapability::Resolve(ResolveCapability::CollectDeclarations);
    let supported = profile([collect]).unwrap();
    let sources = [source(1, "right"), source(0, "left")];
    let analysis = collect_declarations(&sources, &supported).unwrap();
    assert!(matches!(analysis.product(), PhaseProduct::Performed(_)));
    let mut demands = analysis.demands().to_vec();
    demands.reverse();
    demands.push(demands[0]);

    let trace = CapabilityTrace::from_demands(&supported, demands.clone()).unwrap();
    assert!(!trace.has_missing());
    assert_eq!(trace.entries().len(), 1);
    assert_eq!(trace.satisfied().count(), 1);
    assert_eq!(trace.entries()[0].locations().len(), 2);
    assert_eq!(trace.entries()[0].locations()[0].file(), FileId::new(0));
    assert_eq!(trace.entries()[0].locations()[1].file(), FileId::new(1));

    let unsupported = profile([]).unwrap();
    assert!(matches!(
        CapabilityTrace::from_demands(&unsupported, demands),
        Err(CapabilityTraceError::SupportMismatch { capability, .. })
            if capability == collect
    ));

    let missing_analysis = collect_declarations(&sources, &unsupported).unwrap();
    let missing =
        CapabilityTrace::from_demands(&unsupported, missing_analysis.demands().iter().copied())
            .unwrap();
    assert!(missing.has_missing());
    assert_eq!(missing.missing().count(), 1);
    assert_eq!(
        missing.entries()[0].disposition(),
        DemandDisposition::Missing
    );
}

#[test]
fn complete_ownership_flow_cannot_be_enabled_before_it_is_implemented() {
    let full =
        CompileCapability::Typecheck(eglur_sema::TypecheckCapability::CheckCompleteOwnershipFlow);
    let core = SupportProfile::semantic_core_v1();
    assert!(matches!(
        profile(core.compile_capabilities().chain([full])),
        Err(SupportProfileError::UnimplementedCapability(capability)) if capability == full
    ));
}

#[test]
fn trusted_inherent_resolution_cannot_be_enabled_before_it_is_implemented() {
    let trusted = CompileCapability::Typecheck(
        eglur_sema::TypecheckCapability::ResolveTrustedInherentMethods,
    );
    let core = SupportProfile::semantic_core_v1();
    assert!(matches!(
        profile(core.compile_capabilities().chain([trusted])),
        Err(SupportProfileError::UnimplementedCapability(capability)) if capability == trusted
    ));
}

#[test]
fn semantic_core_profile_and_unsupported_inventory_partition_closed_vocabularies() {
    let profile = SupportProfile::semantic_core_v1();
    let manifest = SupportManifest::from_profile(&profile);
    let inventory = UnsupportedInventory::from_profile(&profile);

    assert_eq!(manifest.schema(), "eglur.support.v1");
    assert_eq!(manifest.profile(), "semantic-core-v1");
    assert_eq!(inventory.schema(), "eglur.unsupported.v1");
    assert_eq!(inventory.profile(), manifest.profile());

    let supported_compile = manifest
        .compile()
        .iter()
        .map(AsRef::<str>::as_ref)
        .collect::<std::collections::BTreeSet<_>>();
    let unsupported_compile = inventory
        .compile_capabilities()
        .iter()
        .map(AsRef::<str>::as_ref)
        .collect::<std::collections::BTreeSet<_>>();
    let all_compile = CompileCapability::ALL
        .into_iter()
        .map(CompileCapability::stable_key)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(supported_compile.is_disjoint(&unsupported_compile));
    assert_eq!(
        supported_compile
            .union(&unsupported_compile)
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        all_compile
    );

    let supported_execution = manifest
        .execute()
        .iter()
        .map(AsRef::<str>::as_ref)
        .collect::<std::collections::BTreeSet<_>>();
    let unsupported_execution = inventory
        .execution_capabilities()
        .iter()
        .map(AsRef::<str>::as_ref)
        .collect::<std::collections::BTreeSet<_>>();
    let all_execution = ExecutionCapability::ALL
        .into_iter()
        .map(ExecutionCapability::stable_key)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(supported_execution.is_disjoint(&unsupported_execution));
    assert_eq!(
        supported_execution
            .union(&unsupported_execution)
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        all_execution
    );

    let payload: serde_json::Value = serde_json::from_str(&inventory.to_json()).unwrap();
    assert_eq!(payload["kind"], "eglur_unsupported_inventory");
    assert_eq!(payload["profile"], manifest.profile());
    assert_eq!(
        payload["typed_execution_reasons"].as_array().unwrap().len(),
        eglur_driver::UnsupportedFeature::ALL.len()
    );
}
