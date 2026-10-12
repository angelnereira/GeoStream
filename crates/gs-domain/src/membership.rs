//! Membership domain model, capability rules, sharing state machine, and presence (docs/05 §5–8).
#![forbid(unsafe_code)]

use std::fmt;
use std::str::FromStr;
use time::OffsetDateTime;

use crate::ids::{DeviceId, InvitationId, MemberId, SessionId, SubjectId, TenantId};
use crate::session::{Actor, ClosedReason, SessionMode};

/// Role of a member in a session (docs/05 §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MemberRole {
    #[serde(rename = "OWNER", alias = "MEMBER_ROLE_OWNER")]
    Owner,
    #[serde(rename = "MEMBER", alias = "MEMBER_ROLE_MEMBER")]
    Member,
}

impl fmt::Display for MemberRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Owner => write!(f, "OWNER"),
            Self::Member => write!(f, "MEMBER"),
        }
    }
}

impl FromStr for MemberRole {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "OWNER" | "MEMBER_ROLE_OWNER" => Ok(Self::Owner),
            "MEMBER" | "MEMBER_ROLE_MEMBER" => Ok(Self::Member),
            _ => Err(()),
        }
    }
}

/// Lifecycle status of a membership (docs/05 §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MemberStatusKind {
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "LEFT")]
    Left,
    #[serde(rename = "REVOKED")]
    Revoked,
}

impl fmt::Display for MemberStatusKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "ACTIVE"),
            Self::Left => write!(f, "LEFT"),
            Self::Revoked => write!(f, "REVOKED"),
        }
    }
}

/// How a member joined the session (docs/05 §5.1).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum JoinedVia {
    Creator,
    DirectAdd,
    Invitation(InvitationId),
}

/// Permissions granted to a member in a session (docs/05 §5.1, session.proto).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub struct Capabilities {
    pub publish: bool,
    pub read: bool,
    pub invite: bool,
    pub manage: bool,
}

impl Capabilities {
    /// Full permissions: owner always has all four (INV-MEM-1).
    pub const fn all() -> Self {
        Self {
            publish: true,
            read: true,
            invite: true,
            manage: true,
        }
    }

    pub const fn none() -> Self {
        Self {
            publish: false,
            read: false,
            invite: false,
            manage: false,
        }
    }

    pub const fn read_only() -> Self {
        Self {
            publish: false,
            read: true,
            invite: false,
            manage: false,
        }
    }

    pub const fn publish_read() -> Self {
        Self {
            publish: true,
            read: true,
            invite: false,
            manage: false,
        }
    }

    /// Evaluates default capabilities by session mode (docs/05 §4.2, FR-SES-006).
    pub fn default_for_mode(mode: SessionMode, is_owner: bool) -> Self {
        if is_owner {
            return Self::all();
        }
        match mode {
            SessionMode::OneToOne => Self::publish_read(),
            SessionMode::OneToMany => Self::read_only(),
            SessionMode::ManyToMany => Self::publish_read(),
        }
    }

    /// Checks if `self` is a subset of `other` (`self ⊆ other`, INV-MEM-2).
    pub fn is_subset_of(&self, other: &Self) -> bool {
        (!self.publish || other.publish)
            && (!self.read || other.read)
            && (!self.invite || other.invite)
            && (!self.manage || other.manage)
    }
}

/// Connectivity status of publisher device (session.proto).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Connectivity {
    #[default]
    #[serde(rename = "UNSPECIFIED", alias = "CONNECTIVITY_UNSPECIFIED")]
    Unspecified,
    #[serde(rename = "ONLINE", alias = "CONNECTIVITY_ONLINE")]
    Online,
    #[serde(rename = "OFFLINE", alias = "CONNECTIVITY_OFFLINE")]
    Offline,
}

/// Outcome of a sharing state transition (docs/05 §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharingTransition {
    Started(DeviceId),
    Transferred {
        previous_device: DeviceId,
        new_device: DeviceId,
    },
    Stopped,
    Unchanged,
}

/// Outcome when an owner leaves or is revoked (FR-SES-007, docs/05 §5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerDepartureOutcome {
    Transferred {
        new_owner_subject_id: SubjectId,
        new_owner_member_id: MemberId,
    },
    SessionClosed(ClosedReason),
    NoOwnerSessionOpen,
}

/// View of member status exposed in stream or REST (INV-PRS-1).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MemberStatusView {
    pub session_id: SessionId,
    pub member_id: MemberId,
    pub external_subject_id: Option<String>,
    pub role: MemberRole,
    pub capabilities: Capabilities,
    pub sharing: bool,
    pub connectivity: Connectivity,
    pub last_location_at: Option<OffsetDateTime>,
}

/// Domain errors for membership operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MembershipError {
    #[error("owner capabilities cannot be reduced (INV-MEM-1)")]
    OwnerCapabilitiesImmutable,
    #[error("cannot grant capabilities exceeding own capabilities (INV-MEM-2)")]
    CannotEscalateCapabilities,
    #[error("member cannot modify own capabilities (INV-MEM-3)")]
    CannotModifyOwnCapabilities,
    #[error("permission denied: caller lacks manage or permission")]
    PermissionDenied,
    #[error("member is not active")]
    NotActiveMember,
    #[error("only the subject itself can activate location sharing (INV-SHR-2)")]
    OnlySelfCanActivateSharing,
    #[error("member lacks publish capability to share location")]
    MissingPublishCapability,
    #[error("subject is already an active member of this session (INV-MEM-6)")]
    AlreadyActiveMember,
    #[error("revoked member cannot rejoin via invitation (FR-MEM-007)")]
    RevokedMemberCannotRedeemInvitation,
    #[error("session is full (reached max_members) (INV-MEM-7)")]
    SessionFull,
    #[error("publishing attempted without active sharing")]
    NotSharing,
    #[error("another device of the subject is already the active publisher")]
    NotPublisher,
    #[error("member was revoked from this session")]
    MemberRevoked,
}

/// Session membership entity (docs/05 §5).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Membership {
    pub id: MemberId,
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub subject_id: SubjectId,
    pub role: MemberRole,
    pub capabilities: Capabilities,
    pub status: MemberStatusKind,
    pub sharing: bool,
    pub publisher_device_id: Option<DeviceId>,
    pub sharing_changed_at: Option<OffsetDateTime>,
    pub joined_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
    pub joined_via: JoinedVia,
    pub version: u64,
}

impl Membership {
    /// Constructs a new owner membership. Starts with sharing=false (FR-SHR-001) and all capabilities (INV-MEM-1).
    pub fn new_owner(
        id: MemberId,
        session_id: SessionId,
        tenant_id: TenantId,
        subject_id: SubjectId,
        joined_at: OffsetDateTime,
    ) -> Self {
        Self {
            id,
            session_id,
            tenant_id,
            subject_id,
            role: MemberRole::Owner,
            capabilities: Capabilities::all(), // INV-MEM-1
            status: MemberStatusKind::Active,
            sharing: false, // FR-SHR-001
            publisher_device_id: None,
            sharing_changed_at: None,
            joined_at,
            ended_at: None,
            joined_via: JoinedVia::Creator,
            version: 1,
        }
    }

    /// Constructs a new regular membership. Starts with sharing=false (FR-SHR-001).
    pub fn new_member(
        id: MemberId,
        session_id: SessionId,
        tenant_id: TenantId,
        subject_id: SubjectId,
        capabilities: Capabilities,
        joined_via: JoinedVia,
        joined_at: OffsetDateTime,
    ) -> Self {
        Self {
            id,
            session_id,
            tenant_id,
            subject_id,
            role: MemberRole::Member,
            capabilities,
            status: MemberStatusKind::Active,
            sharing: false, // FR-SHR-001
            publisher_device_id: None,
            sharing_changed_at: None,
            joined_at,
            ended_at: None,
            joined_via,
            version: 1,
        }
    }

    /// Activates sharing (docs/05 §7, FR-SHR-002, INV-SHR-1..3).
    pub fn start_sharing(
        &mut self,
        caller_subject_id: SubjectId,
        device_id: DeviceId,
        now: OffsetDateTime,
    ) -> Result<SharingTransition, MembershipError> {
        if self.status != MemberStatusKind::Active {
            return Err(MembershipError::NotActiveMember);
        }

        // INV-SHR-2: only the subject itself can activate sharing
        if caller_subject_id != self.subject_id {
            return Err(MembershipError::OnlySelfCanActivateSharing);
        }

        // Must have publish capability
        if !self.capabilities.publish {
            return Err(MembershipError::MissingPublishCapability);
        }

        // INV-SHR-3: idempotent if already sharing from the same device
        if self.sharing && self.publisher_device_id == Some(device_id) {
            return Ok(SharingTransition::Unchanged);
        }

        let transition = if self.sharing {
            let prev = self.publisher_device_id.unwrap_or(device_id);
            SharingTransition::Transferred {
                previous_device: prev,
                new_device: device_id,
            }
        } else {
            SharingTransition::Started(device_id)
        };

        // INV-SHR-1: sharing=true => publisher_device_id present, active, and from same subject
        self.sharing = true;
        self.publisher_device_id = Some(device_id);
        self.sharing_changed_at = Some(now);
        self.version += 1;

        Ok(transition)
    }

    /// Stops sharing (docs/05 §7.3).
    pub fn stop_sharing(&mut self, now: OffsetDateTime) -> SharingTransition {
        if !self.sharing {
            return SharingTransition::Unchanged;
        }

        self.sharing = false;
        self.publisher_device_id = None;
        self.sharing_changed_at = Some(now);
        self.version += 1;

        SharingTransition::Stopped
    }

    /// Updates member capabilities with strict invariant checks (INV-MEM-1..4).
    ///
    /// Returns `Ok(true)` if sharing was automatically stopped due to losing `publish` (INV-MEM-4).
    pub fn update_capabilities(
        &mut self,
        caller_is_application: bool,
        caller_member_id: Option<MemberId>,
        caller_caps: Option<&Capabilities>,
        new_caps: Capabilities,
        now: OffsetDateTime,
    ) -> Result<bool, MembershipError> {
        if self.status != MemberStatusKind::Active {
            return Err(MembershipError::NotActiveMember);
        }

        // INV-MEM-1: owner capabilities cannot be reduced
        if self.role == MemberRole::Owner {
            return Err(MembershipError::OwnerCapabilitiesImmutable);
        }

        if !caller_is_application {
            // INV-MEM-3: member cannot modify their own capabilities
            if caller_member_id == Some(self.id) {
                return Err(MembershipError::CannotModifyOwnCapabilities);
            }

            // Caller must have manage
            let caller_c = caller_caps.ok_or(MembershipError::PermissionDenied)?;
            if !caller_c.manage {
                return Err(MembershipError::PermissionDenied);
            }

            // INV-MEM-2: new capabilities must be a subset of caller capabilities
            if !new_caps.is_subset_of(caller_c) {
                return Err(MembershipError::CannotEscalateCapabilities);
            }
        }

        let had_publish = self.capabilities.publish;
        self.capabilities = new_caps;
        self.version += 1;

        // INV-MEM-4: withdrawing publish when sharing=true stops sharing immediately
        if had_publish && !new_caps.publish && self.sharing {
            self.stop_sharing(now);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Member leaves the session voluntarily.
    pub fn leave(&mut self, now: OffsetDateTime) -> Result<(), MembershipError> {
        if self.status != MemberStatusKind::Active {
            return Err(MembershipError::NotActiveMember);
        }
        self.sharing = false;
        self.publisher_device_id = None;
        self.status = MemberStatusKind::Left;
        self.ended_at = Some(now);
        self.version += 1;
        Ok(())
    }

    /// Member is revoked from the session.
    pub fn revoke(&mut self, now: OffsetDateTime) -> Result<(), MembershipError> {
        if self.status != MemberStatusKind::Active {
            return Err(MembershipError::NotActiveMember);
        }
        self.sharing = false;
        self.publisher_device_id = None;
        self.status = MemberStatusKind::Revoked;
        self.ended_at = Some(now);
        self.version += 1;
        Ok(())
    }

    /// Constructs the public MemberStatusView enforcing INV-PRS-1.
    ///
    /// INV-PRS-1 (FR-PRS-004): If `sharing=false`, `connectivity` MUST always be
    /// `Connectivity::Unspecified` and `last_location_at` MUST be `None`.
    pub fn build_status_view(
        &self,
        external_subject_id: Option<String>,
        device_online: bool,
        snapshot_time: Option<OffsetDateTime>,
    ) -> MemberStatusView {
        let (connectivity, last_location_at) = if self.sharing {
            let conn = if device_online {
                Connectivity::Online
            } else {
                Connectivity::Offline
            };
            (conn, snapshot_time)
        } else {
            // INV-PRS-1: never reveal presence or coordinates if member is not sharing
            (Connectivity::Unspecified, None)
        };

        MemberStatusView {
            session_id: self.session_id,
            member_id: self.id,
            external_subject_id,
            role: self.role,
            capabilities: self.capabilities,
            sharing: self.sharing,
            connectivity,
            last_location_at,
        }
    }
}

/// Evaluates owner departure according to FR-SES-007 (docs/05 §5.4).
pub fn handle_owner_departure(
    departing_owner_id: SubjectId,
    active_members: &mut [Membership],
    session_created_by: &Actor,
) -> OwnerDepartureOutcome {
    // 1. Filter eligible candidates: active, different subject, has manage
    let eligible = active_members
        .iter_mut()
        .filter(|m| {
            m.status == MemberStatusKind::Active
                && m.subject_id != departing_owner_id
                && m.capabilities.manage
        })
        .min_by(|a, b| {
            // Oldest by joined_at, tie-breaker by member ID
            a.joined_at.cmp(&b.joined_at).then_with(|| a.id.cmp(&b.id))
        });

    if let Some(successor) = eligible {
        successor.role = MemberRole::Owner;
        successor.capabilities = Capabilities::all(); // INV-MEM-1
        successor.version += 1;
        OwnerDepartureOutcome::Transferred {
            new_owner_subject_id: successor.subject_id,
            new_owner_member_id: successor.id,
        }
    } else {
        match session_created_by {
            Actor::Device(_) => OwnerDepartureOutcome::SessionClosed(ClosedReason::OwnerLeft),
            Actor::Application(_) => OwnerDepartureOutcome::NoOwnerSessionOpen,
        }
    }
}

/// Evaluates readmission eligibility according to FR-MEM-007 (docs/05 §5.5).
pub fn validate_readmission(
    last_status: Option<MemberStatusKind>,
    is_direct_add: bool,
) -> Result<(), MembershipError> {
    match last_status {
        Some(MemberStatusKind::Active) => Err(MembershipError::AlreadyActiveMember),
        Some(MemberStatusKind::Revoked) => {
            if is_direct_add {
                Ok(())
            } else {
                Err(MembershipError::RevokedMemberCannotRedeemInvitation)
            }
        }
        Some(MemberStatusKind::Left) | None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enums_display_and_parse() {
        assert_eq!(MemberRole::Owner.to_string(), "OWNER");
        assert_eq!(MemberRole::Member.to_string(), "MEMBER");
        assert_eq!(MemberRole::from_str("OWNER").unwrap(), MemberRole::Owner);
        assert_eq!(MemberRole::from_str("MEMBER").unwrap(), MemberRole::Member);
        assert!(MemberRole::from_str("INVALID").is_err());

        assert_eq!(MemberStatusKind::Active.to_string(), "ACTIVE");
        assert_eq!(MemberStatusKind::Left.to_string(), "LEFT");
        assert_eq!(MemberStatusKind::Revoked.to_string(), "REVOKED");

        assert_eq!(Connectivity::default(), Connectivity::Unspecified);
    }

    #[test]
    fn test_membership_lifecycle_and_error_paths() {
        let now = OffsetDateTime::now_utc();
        let mut member = Membership::new_member(
            MemberId::new(),
            SessionId::new(),
            TenantId::new(),
            SubjectId::new(),
            Capabilities::read_only(),
            JoinedVia::Invitation(InvitationId::new()),
            now,
        );

        // start_sharing fails without publish capability
        let dev = DeviceId::new();
        assert_eq!(
            member.start_sharing(member.subject_id, dev, now),
            Err(MembershipError::MissingPublishCapability)
        );

        // start_sharing fails if caller != subject_id
        member.capabilities = Capabilities::all();
        let other_sub = SubjectId::new();
        assert_eq!(
            member.start_sharing(other_sub, dev, now),
            Err(MembershipError::OnlySelfCanActivateSharing)
        );

        // Leave session
        assert!(member.leave(now).is_ok());
        assert_eq!(member.status, MemberStatusKind::Left);

        // Leave or revoke on inactive returns NotActiveMember
        assert_eq!(member.leave(now), Err(MembershipError::NotActiveMember));
        assert_eq!(member.revoke(now), Err(MembershipError::NotActiveMember));
        assert_eq!(
            member.start_sharing(member.subject_id, dev, now),
            Err(MembershipError::NotActiveMember)
        );

        // Application caller in update_capabilities
        let mut member2 = Membership::new_member(
            MemberId::new(),
            SessionId::new(),
            TenantId::new(),
            SubjectId::new(),
            Capabilities::read_only(),
            JoinedVia::Creator,
            now,
        );
        let res = member2.update_capabilities(true, None, None, Capabilities::all(), now);
        assert!(res.is_ok());
        assert_eq!(member2.capabilities, Capabilities::all());
    }
}
