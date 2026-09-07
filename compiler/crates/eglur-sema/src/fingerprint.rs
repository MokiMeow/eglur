use std::fmt;

use sha2::{Digest, Sha256};

mod identity;
mod semantic;

pub use identity::{
    StableDeclarationError, StableDeclarationKey, StableDeclarationKeys, StableTargetKey,
};
pub use semantic::{
    ConformanceComponentFact, ConformanceEnvironmentFact, ModuleSemanticFingerprints,
    SemanticFingerprintError, SemanticFingerprints,
};

const FINGERPRINT_MAGIC: &[u8] = b"eglur-semantic-fingerprint";
const FINGERPRINT_SCHEMA_VERSION: u32 = 1;

/// Closed families of compiler-internal deterministic fingerprints.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FingerprintDomain {
    Source,
    AnalysisInput,
    Interface,
    Body,
    Effect,
    Catalog,
    DeclarationDiscriminator,
    Conformance,
}

impl FingerprintDomain {
    const fn tag(self) -> u8 {
        match self {
            Self::Source => 0,
            Self::AnalysisInput => 1,
            Self::Interface => 2,
            Self::Body => 3,
            Self::Effect => 4,
            Self::Catalog => 5,
            Self::DeclarationDiscriminator => 6,
            Self::Conformance => 7,
        }
    }
}

/// One SHA-256 result over a versioned canonical semantic encoding.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FingerprintDigest([u8; 32]);

impl FingerprintDigest {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for FingerprintDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "FingerprintDigest({self})")
    }
}

impl fmt::Display for FingerprintDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Typed writer for the canonical fingerprint wire image.
///
/// This encoding is compiler-internal. Explicit tags and lengths keep adjacent
/// values unambiguous without exposing Rust layouts or serializer behavior.
pub struct CanonicalFingerprintBuilder {
    hasher: Sha256,
}

impl CanonicalFingerprintBuilder {
    #[must_use]
    pub fn new(domain: FingerprintDomain) -> Self {
        let mut builder = Self {
            hasher: Sha256::new(),
        };
        builder.write_bytes(FINGERPRINT_MAGIC);
        builder.write_u32(FINGERPRINT_SCHEMA_VERSION);
        builder.write_u8(domain.tag());
        builder
    }

    pub fn write_u8(&mut self, value: u8) {
        self.hasher.update([value]);
    }

    pub fn write_bool(&mut self, value: bool) {
        self.write_u8(u8::from(value));
    }

    pub fn write_u32(&mut self, value: u32) {
        self.hasher.update(value.to_le_bytes());
    }

    pub fn write_u64(&mut self, value: u64) {
        self.hasher.update(value.to_le_bytes());
    }

    pub fn write_len(&mut self, value: usize) {
        self.write_u64(
            u64::try_from(value).expect("canonical fingerprint length must fit unsigned 64-bit"),
        );
    }

    pub fn write_bytes(&mut self, value: &[u8]) {
        self.write_len(value.len());
        self.hasher.update(value);
    }

    pub fn write_str(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
    }

    pub fn write_digest(&mut self, value: FingerprintDigest) {
        self.hasher.update(value.0);
    }

    #[must_use]
    pub fn finish(self) -> FingerprintDigest {
        FingerprintDigest(self.hasher.finalize().into())
    }
}

macro_rules! fingerprint_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(FingerprintDigest);

        impl $name {
            #[must_use]
            pub const fn new(digest: FingerprintDigest) -> Self {
                Self(digest)
            }

            #[must_use]
            pub const fn digest(self) -> FingerprintDigest {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

fingerprint_type!(InterfaceFingerprint);
fingerprint_type!(BodyFingerprint);
fingerprint_type!(EffectFingerprint);
fingerprint_type!(ConformanceFingerprint);
fingerprint_type!(ConformanceComponentFingerprint);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_encoding_separates_domains_and_value_boundaries() {
        let mut split_left = CanonicalFingerprintBuilder::new(FingerprintDomain::Interface);
        split_left.write_str("ab");
        split_left.write_str("c");

        let mut split_right = CanonicalFingerprintBuilder::new(FingerprintDomain::Interface);
        split_right.write_str("a");
        split_right.write_str("bc");

        let mut other_domain = CanonicalFingerprintBuilder::new(FingerprintDomain::Body);
        other_domain.write_str("ab");
        other_domain.write_str("c");

        assert_ne!(split_left.finish(), split_right.finish());
        assert_ne!(
            CanonicalFingerprintBuilder::new(FingerprintDomain::Interface).finish(),
            CanonicalFingerprintBuilder::new(FingerprintDomain::Body).finish()
        );
        assert_ne!(
            other_domain.finish(),
            CanonicalFingerprintBuilder::new(FingerprintDomain::Body).finish()
        );
    }

    #[test]
    fn digest_hex_projection_is_fixed_width_and_round_trip_free() {
        let digest = CanonicalFingerprintBuilder::new(FingerprintDomain::Catalog).finish();
        assert_eq!(digest.to_string().len(), 64);
        assert_eq!(digest.as_bytes().len(), 32);
    }
}
