//! Repository implementations for GeoStream storage.
#![forbid(unsafe_code)]

pub mod api_key_repo;
pub mod app_repo;
pub mod audit_repo;
pub mod device_repo;
pub mod history_repo;
pub mod idempotency_repo;
pub mod invitation_repo;
pub mod membership_repo;
pub mod session_repo;
pub mod subject_repo;
pub mod tag_repo;
pub mod tenant_repo;

pub use api_key_repo::ApiKeyRepository;
pub use app_repo::ApplicationRepository;
pub use audit_repo::AuditRepository;
pub use device_repo::{DeviceRepository, UpsertDeviceParams};
pub use history_repo::{CellRow, HistoryRepository, LocationPointRow, TracePoint};
pub use idempotency_repo::{
    BeginOutcome, IdempotencyRecord, IdempotencyRepository, IdempotencyState,
};
pub use invitation_repo::{
    CreateInvitationOutcome, CreateInvitationParams, InvitationPage, InvitationRepository,
    InvitationRow, RedeemOutcome,
};
pub use membership_repo::{
    AddMemberOutcome, LeaveOutcome, MemberPage, MemberWithSubject, MembershipRepository,
    RevokeOutcome, SetSharingOutcome, SetSharingParams, TransferOwnershipOutcome,
    UpdateCapabilitiesOutcome, UpdateCapabilitiesParams,
};
pub use session_repo::{SessionFilters, SessionPage, SessionRepository, SessionUpdate};
pub use subject_repo::SubjectRepository;
pub use tag_repo::{Tag, TagRepository};
pub use tenant_repo::TenantRepository;
