//! Use cases: sessions, members, invitations, sharing, tokens and the authorization cache.
#![forbid(unsafe_code)]

pub mod admin;
pub mod auth;
pub mod idempotency;
pub mod invitations;
pub mod members;
pub mod sessions;
pub mod tokens;
pub mod migrate {
    pub use gs_store::PgPool;
    pub use gs_store::StoreError;
    pub use gs_store::migrate::*;
}

pub use admin::{
    AdminError, CreatedApiKey, DEFAULT_APPLICATION_DEVICE_SCOPES, GeneratedSigningKeyFiles,
};
pub use auth::{
    ApplicationPrincipal, AuthError, VerifiedPrincipal, require_scope, verify_application_key,
    verify_application_principal, verify_device_token_principal,
};
pub use gs_bus::{Bus, BusError, BusMessage, InMemoryBus};
pub use gs_ephemeral::{EphemeralError, EphemeralStore, InMemoryEphemeralStore};
pub use idempotency::{
    IDEMPOTENCY_KEY_MAX_LEN, IdempotencyContext, IdempotencyDecision, IdempotencyError,
    StoredResponse, begin as idempotency_begin, canonical_json, discard as idempotency_discard,
    finish as idempotency_finish, request_fingerprint, run_idempotent,
};
pub use invitations::{
    CreateInvitationCaller, CreateInvitationRequest, InvitationCreatedResource,
    InvitationListResource, InvitationResource, InvitationUseCaseError, ListInvitationsCaller,
    RedeemInvitationParams, RedeemInvitationRequest, RedeemInvitationResponse,
    RevokeInvitationCaller, create_invitation, list_invitations, redeem_invitation,
    revoke_invitation,
};
pub use members::{
    AddMemberRequest, MemberList, MemberResource, MemberUseCaseError, SetSharingCaller,
    SetSharingContext, SharingRequest, UpdateCaller, UpdateMemberRequest, add_member, get_member,
    get_member_for_device_me, leave_session, list_members, revoke_member, set_sharing,
    update_member,
};
pub use sessions::{
    CreateSessionRequest, SessionFilters, SessionPage, SessionPolicy, SessionResource,
    SessionUseCaseError, UpdateSessionRequest, close_session, create_session, expire_due_sessions,
    get_session, list_sessions_for_application, list_sessions_for_subject, update_session,
    verify_device_active_member, verify_device_manage_capability,
};
pub use tokens::{
    MintedToken, REQUIRED_SCOPE, TokenError, TokenRequest, TokenResponse, TokenTtlBounds,
    mint_device_token,
};

// `gs-service` is the only crate of the HTTP layer that may depend on these crates
// (docs/04 §4.1), so the layers above name their types through these re-exports instead of
// adding dependencies the dependency checker would reject.
pub use gs_auth;
pub use gs_bus;
pub use gs_domain;
pub use gs_ephemeral;
pub use gs_store;
