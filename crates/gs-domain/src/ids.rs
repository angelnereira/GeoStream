//! Strongly typed, prefixed entity identifiers based on UUIDv7.
//! Normative reference: docs/05_DOMAIN_MODEL.md §2 and ADR-0011.
#![forbid(unsafe_code)]

use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use uuid::{Uuid, Variant, Version};

/// Error returned when parsing an entity identifier fails.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum IdParseError {
    #[error("identifier cannot be empty")]
    Empty,

    #[error("invalid prefix: expected '{expected}', found '{found}'")]
    InvalidPrefix {
        expected: &'static str,
        found: String,
    },

    #[error("invalid separator: expected '_' after prefix")]
    InvalidSeparator,

    #[error("invalid hex length: expected 32 hex characters, found {0}")]
    InvalidLength(usize),

    #[error("invalid character in hex payload: must be lowercase hex [0-9a-f]")]
    InvalidCharacters,

    #[error("invalid UUID payload: {0}")]
    InvalidUuid(String),
}

/// Error returned when validating an external subject identifier.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum ExternalSubjectIdError {
    #[error("external_subject_id cannot be empty")]
    Empty,

    #[error("external_subject_id exceeds maximum length of 128 characters (found {0})")]
    TooLong(usize),

    #[error("external_subject_id contains invalid character '{0}': only [A-Za-z0-9._:-] allowed")]
    InvalidCharacter(char),
}

/// Error returned when validating an installation identifier.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum InstallationIdError {
    #[error("installation_id cannot be empty")]
    Empty,

    #[error("installation_id must be in canonical 36-char hyphenated format")]
    InvalidFormat,

    #[error("installation_id must be lowercase")]
    NotLowercase,

    #[error("installation_id must be a valid UUIDv4: {0}")]
    InvalidUuid(String),

    #[error("installation_id must have UUID version 4")]
    NotVersion4,

    #[error("installation_id must have RFC 4122 variant")]
    NotRfc4122Variant,
}

macro_rules! define_prefixed_id {
    ($type_name:ident, $prefix:literal, $doc_name:literal) => {
        #[doc = concat!("Strongly-typed identifier for `", $doc_name, "` with prefix `", $prefix, "_`.")]
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $type_name(Uuid);

        impl $type_name {
            /// The prefix string for this entity type without the underscore.
            pub const PREFIX: &'static str = $prefix;

            /// Generates a new unique identifier using UUIDv7.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Creates an identifier directly from an existing [`Uuid`].
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Returns the underlying [`Uuid`].
            pub const fn into_uuid(self) -> Uuid {
                self.0
            }

            /// Returns a reference to the underlying [`Uuid`].
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            /// Returns the 16 bytes of the underlying UUID.
            pub const fn as_bytes(&self) -> &[u8; 16] {
                self.0.as_bytes()
            }
        }

        impl Default for $type_name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $type_name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}_{}", Self::PREFIX, self.0.simple())
            }
        }

        impl fmt::Debug for $type_name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($type_name), self)
            }
        }

        impl FromStr for $type_name {
            type Err = IdParseError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if s.is_empty() {
                    return Err(IdParseError::Empty);
                }

                let (prefix, hex_part) = match s.split_once('_') {
                    Some(parts) => parts,
                    None => {
                        // Check if separator was wrong (e.g. '-' instead of '_')
                        if let Some((prefix, _)) = s.split_once('-') {
                            if prefix == Self::PREFIX {
                                return Err(IdParseError::InvalidSeparator);
                            }
                        }
                        return Err(IdParseError::InvalidPrefix {
                            expected: Self::PREFIX,
                            found: s.to_string(),
                        });
                    }
                };

                if prefix != Self::PREFIX {
                    return Err(IdParseError::InvalidPrefix {
                        expected: Self::PREFIX,
                        found: prefix.to_string(),
                    });
                }

                if hex_part.len() != 32 {
                    return Err(IdParseError::InvalidLength(hex_part.len()));
                }

                for b in hex_part.bytes() {
                    if !b.is_ascii_digit() && !(b'a'..=b'f').contains(&b) {
                        return Err(IdParseError::InvalidCharacters);
                    }
                }

                let uuid = Uuid::parse_str(hex_part)
                    .map_err(|e| IdParseError::InvalidUuid(e.to_string()))?;

                Ok(Self(uuid))
            }
        }

        impl serde::Serialize for $type_name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.collect_str(self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $type_name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let s = String::deserialize(deserializer)?;
                Self::from_str(&s).map_err(serde::de::Error::custom)
            }
        }
    };
}

// 14 entity IDs defined in docs/05_DOMAIN_MODEL.md §2:
define_prefixed_id!(TenantId, "ten", "Tenant");
define_prefixed_id!(ApplicationId, "app", "Application");
define_prefixed_id!(ApiKeyId, "key", "ApiKey");
define_prefixed_id!(SubjectId, "sub", "Subject");
define_prefixed_id!(DeviceId, "dev", "Device");
define_prefixed_id!(SessionId, "ses", "Session");
define_prefixed_id!(MemberId, "mem", "Membership");
define_prefixed_id!(InvitationId, "inv", "Invitation");
define_prefixed_id!(WebhookEndpointId, "whk", "WebhookEndpoint");
define_prefixed_id!(EventId, "evt", "Webhook Event");
define_prefixed_id!(GeofenceId, "geo", "Geofence");
define_prefixed_id!(ConnectionId, "con", "Stream Connection");
define_prefixed_id!(RequestId, "req", "Request Correlation");
define_prefixed_id!(AuditId, "aud", "Audit Log Record");
define_prefixed_id!(TagId, "tag", "Tag");
define_prefixed_id!(TripId, "trp", "Trip");
define_prefixed_id!(PlaceId, "plc", "Place");
define_prefixed_id!(ExportId, "exp", "Export");
define_prefixed_id!(LpwanIntegrationId, "lpw", "LPWAN Integration");
define_prefixed_id!(AccountId, "acc", "Console Account");

/// Integrator-defined external subject identifier (`external_subject_id`).
/// Regex: `^[A-Za-z0-9._:\-]{1,128}$`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExternalSubjectId(String);

impl ExternalSubjectId {
    /// Maximum allowed length in characters.
    pub const MAX_LEN: usize = 128;

    /// Validates and constructs a new [`ExternalSubjectId`].
    pub fn new(s: impl Into<String>) -> Result<Self, ExternalSubjectIdError> {
        let val = s.into();
        Self::validate(&val)?;
        Ok(Self(val))
    }

    fn validate(val: &str) -> Result<(), ExternalSubjectIdError> {
        if val.is_empty() {
            return Err(ExternalSubjectIdError::Empty);
        }
        if val.len() > Self::MAX_LEN {
            return Err(ExternalSubjectIdError::TooLong(val.len()));
        }
        for c in val.chars() {
            if !c.is_ascii_alphanumeric() && c != '.' && c != '_' && c != ':' && c != '-' {
                return Err(ExternalSubjectIdError::InvalidCharacter(c));
            }
        }
        Ok(())
    }

    /// Returns a string slice of the external subject ID.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes into inner [`String`].
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for ExternalSubjectId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for ExternalSubjectId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for ExternalSubjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for ExternalSubjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ExternalSubjectId({:?})", self.0)
    }
}

impl FromStr for ExternalSubjectId {
    type Err = ExternalSubjectIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl serde::Serialize for ExternalSubjectId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for ExternalSubjectId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

/// Installation identifier (`installation_id`), stored as UUIDv4 in canonical hyphenated lowercase form.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstallationId(Uuid);

impl InstallationId {
    /// Generates a new random UUIDv4 installation ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates an installation ID from an existing [`Uuid`].
    pub fn from_uuid(uuid: Uuid) -> Result<Self, InstallationIdError> {
        if uuid.get_version() != Some(Version::Random) {
            return Err(InstallationIdError::NotVersion4);
        }
        if uuid.get_variant() != Variant::RFC4122 {
            return Err(InstallationIdError::NotRfc4122Variant);
        }
        Ok(Self(uuid))
    }

    /// Returns the underlying [`Uuid`].
    pub const fn into_uuid(self) -> Uuid {
        self.0
    }

    /// Returns a reference to the underlying [`Uuid`].
    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for InstallationId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for InstallationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

impl fmt::Debug for InstallationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InstallationId({})", self)
    }
}

impl FromStr for InstallationId {
    type Err = InstallationIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(InstallationIdError::Empty);
        }
        if s.len() != 36 {
            return Err(InstallationIdError::InvalidFormat);
        }
        if s.chars().any(|c| c.is_ascii_uppercase()) {
            return Err(InstallationIdError::NotLowercase);
        }

        let uuid =
            Uuid::parse_str(s).map_err(|e| InstallationIdError::InvalidUuid(e.to_string()))?;

        Self::from_uuid(uuid)
    }
}

impl serde::Serialize for InstallationId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for InstallationId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id_methods_and_errors() {
        let tenant_id = TenantId::new();
        let u = *tenant_id.as_uuid();
        assert_eq!(tenant_id.as_bytes().len(), 16);
        assert_eq!(TenantId::from_uuid(u), tenant_id);
        assert_eq!(tenant_id.into_uuid(), u);
        assert_eq!(TenantId::default().as_bytes().len(), 16);

        // IdParseError checks
        assert_eq!(TenantId::from_str(""), Err(IdParseError::Empty));
        assert!(matches!(
            TenantId::from_str("wrong_018f2bb8c49e7b168661df277f329971"),
            Err(IdParseError::InvalidPrefix { .. })
        ));
        assert_eq!(
            TenantId::from_str("ten-018f2bb8c49e7b168661df277f329971"),
            Err(IdParseError::InvalidSeparator)
        );
        assert_eq!(
            TenantId::from_str("ten_too_short"),
            Err(IdParseError::InvalidLength(9))
        );
        assert_eq!(
            TenantId::from_str("ten_018f2bb8c49e7b168661df277f32997Z"),
            Err(IdParseError::InvalidCharacters)
        );

        // ExternalSubjectId errors
        assert_eq!(
            ExternalSubjectId::new(""),
            Err(ExternalSubjectIdError::Empty)
        );
        assert_eq!(
            ExternalSubjectId::new("a".repeat(129)),
            Err(ExternalSubjectIdError::TooLong(129))
        );
        assert_eq!(
            ExternalSubjectId::new("user@example"),
            Err(ExternalSubjectIdError::InvalidCharacter('@'))
        );
        let ext = ExternalSubjectId::new("valid-user.123:foo_bar").unwrap();
        assert_eq!(ext.as_str(), "valid-user.123:foo_bar");
        assert_eq!(ext.to_string(), "valid-user.123:foo_bar");

        // InstallationId errors
        assert_eq!(
            InstallationId::from_str(""),
            Err(InstallationIdError::Empty)
        );
        assert_eq!(
            InstallationId::from_str("abc"),
            Err(InstallationIdError::InvalidFormat)
        );
        assert_eq!(
            InstallationId::from_str("550E8400-E29B-41D4-A716-446655440000"),
            Err(InstallationIdError::NotLowercase)
        );
        assert!(matches!(
            InstallationId::from_str("zzzzzzzz-zzzz-zzzz-zzzz-zzzzzzzzzzzz"),
            Err(InstallationIdError::InvalidUuid(_))
        ));

        let valid_inst = InstallationId::new();
        assert_eq!(valid_inst.to_string().len(), 36);
        assert_eq!(*valid_inst.as_uuid(), valid_inst.into_uuid());
    }
}
