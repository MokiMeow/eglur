use crate::SupportProfile;

/// Versioned machine-boundary projection of one canonical support profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupportManifest {
    schema: Box<str>,
    profile: Box<str>,
    compile: Box<[Box<str>]>,
    execute: Box<[Box<str>]>,
}

impl SupportManifest {
    #[must_use]
    pub fn from_profile(profile: &SupportProfile) -> Self {
        let mut compile = profile
            .compile_capabilities()
            .map(|capability| Box::<str>::from(capability.stable_key()))
            .collect::<Vec<_>>();
        compile.sort();
        let mut execute = profile
            .execution_capabilities()
            .map(|capability| Box::<str>::from(capability.stable_key()))
            .collect::<Vec<_>>();
        execute.sort();
        Self {
            schema: profile.schema().stable_key().into(),
            profile: profile.id().as_str().into(),
            compile: compile.into_boxed_slice(),
            execute: execute.into_boxed_slice(),
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
    pub fn compile(&self) -> &[Box<str>] {
        &self.compile
    }

    #[must_use]
    pub fn execute(&self) -> &[Box<str>] {
        &self.execute
    }
}
