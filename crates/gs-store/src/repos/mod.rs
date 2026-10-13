//! Repository implementations for GeoStream storage.
#![forbid(unsafe_code)]

pub mod api_key_repo;
pub mod app_repo;
pub mod audit_repo;
pub mod device_repo;
pub mod idempotency_repo;
pub mod subject_repo;
pub mod tenant_repo;

pub use api_key_repo::ApiKeyRepository;
pub use app_repo::ApplicationRepository;
pub use audit_repo::AuditRepository;
pub use device_repo::{DeviceRepository, UpsertDeviceParams};
pub use idempotency_repo::{
    BeginOutcome, IdempotencyRecord, IdempotencyRepository, IdempotencyState,
};
pub use subject_repo::SubjectRepository;
pub use tenant_repo::TenantRepository;
