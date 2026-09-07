use std::collections::BTreeSet;

use eglur_eval::{ExecutionCapability, UnsupportedFeature};
use eglur_sema::CompileCapability;
use serde_json::json;

use crate::SupportProfile;

/// Machine-readable complement of one selected support profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnsupportedInventory {
    schema: Box<str>,
    profile: Box<str>,
    compile_capabilities: Box<[Box<str>]>,
    execution_capabilities: Box<[Box<str>]>,
    typed_execution_reasons: Box<[Box<str>]>,
}

impl UnsupportedInventory {
    /// Derives unsupported capability families and typed evaluator refusals.
    #[must_use]
    pub fn from_profile(profile: &SupportProfile) -> Self {
        let compile = profile.compile_capabilities().collect::<BTreeSet<_>>();
        let execution = profile.execution_capabilities().collect::<BTreeSet<_>>();
        let compile_capabilities = CompileCapability::ALL
            .into_iter()
            .filter(|capability| !compile.contains(capability))
            .map(|capability| Box::<str>::from(capability.stable_key()))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let execution_capabilities = ExecutionCapability::ALL
            .into_iter()
            .filter(|capability| !execution.contains(capability))
            .map(|capability| Box::<str>::from(capability.stable_key()))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let typed_execution_reasons = UnsupportedFeature::ALL
            .into_iter()
            .map(|feature| Box::<str>::from(feature.stable_key()))
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            schema: "eglur.unsupported.v1".into(),
            profile: profile.id().as_str().into(),
            compile_capabilities,
            execution_capabilities,
            typed_execution_reasons,
        }
    }

    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }

    #[must_use]
    pub fn profile(&self) -> &str {
        &self.profile
    }

    #[must_use]
    pub fn compile_capabilities(&self) -> &[Box<str>] {
        &self.compile_capabilities
    }

    #[must_use]
    pub fn execution_capabilities(&self) -> &[Box<str>] {
        &self.execution_capabilities
    }

    #[must_use]
    pub fn typed_execution_reasons(&self) -> &[Box<str>] {
        &self.typed_execution_reasons
    }

    /// Emits the canonical unsupported-inventory boundary document.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(&json!({
            "kind": "eglur_unsupported_inventory",
            "schema": self.schema(),
            "profile": self.profile(),
            "compile_capabilities": self.compile_capabilities(),
            "execution_capabilities": self.execution_capabilities(),
            "typed_execution_reasons": self.typed_execution_reasons(),
        }))
        .expect("closed unsupported-inventory values are JSON serializable")
    }
}
