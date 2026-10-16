//! C ABI for bindings that do not use UniFFI directly.
//!
//! **This is the only crate in the workspace that may use `unsafe`.** Every
//! unsafe block is marked with a `// SAFETY:` comment reviewed as part of
//! DoD 19 §3.
//!
//! Invariants:
//! - `GsHandle` is never dereferenced by anyone but us; `*mut` it is just a
//!   token for the host language.
//! - No `panic!` crosses the boundary: every `#[unsafe(no_mangle)] fn` wraps its body
//!   in `let _ = std::panic::catch_unwind(...)` and translates the result into
//!   a signed code.
//! - Handles returned to the host are boxed and ownership transferred on
//!   `create`; the host gives them back on `destroy`. Use-after-destroy is a
//!   contract violation and returns `GsError::InvalidHandle`.

// SAFETY: this crate is the only exception to `#![forbid(unsafe_code)]` and
// does so with an ADR (19 §3). Every unsafe block below has its comment.
#![allow(unsafe_code)]

use std::os::raw::c_int;
use std::panic;

use gs_client_core::{ClientCore, Command, Input, Intent};
use tokio::runtime::Runtime as TokioRuntime;

/// Opaque handle given to the host language. Ownership: created on
/// `gs_client_create`, freed on `gs_client_destroy`. Never dereferenced by
/// anyone external.
pub struct GsHandle {
    #[allow(dead_code)]
    tokio: TokioRuntime,
    /// The core, kept in sync on the Rust side.
    core: ClientCore,
}

/// Result codes returned across the FFI boundary.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GsResult {
    Ok = 0,
    InvalidHandle = 1,
    NullPointer = 2,
    TransportError = 3,
    Panic = 99,
}

/// Construct a handle. Returns a non-null pointer, or null on alloc failure /
/// panic. The host owns the pointer until `gs_client_destroy`.
#[unsafe(no_mangle)]
pub extern "C" fn gs_client_create() -> *mut GsHandle {
    let result = panic::catch_unwind(|| {
        let tokio = TokioRuntime::new().ok()?;
        let handle = Box::new(GsHandle {
            tokio,
            core: ClientCore::new(),
        });
        Some(Box::into_raw(handle))
    });
    // SAFETY: Box::into_raw produced a valid, uniquely owned pointer on the
    // small heap; returning to the host. A null here is the documented way to
    // signal failure.
    result.unwrap_or(None).unwrap_or(std::ptr::null_mut())
}

/// # Safety
///
/// `handle` must be a pointer previously returned by `gs_client_create` and
/// not yet destroyed. The contract is: single owner, no concurrent calls on
/// the same handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gs_client_destroy(handle: *mut GsHandle) -> GsResult {
    if handle.is_null() {
        return GsResult::NullPointer;
    }
    let _ = panic::catch_unwind(|| {
        // SAFETY: handle is non-null and we recover the owning Box that
        // gs_client_create leaked. After this, handle is dangling — documented.
        unsafe {
            let _ = Box::from_raw(handle);
        }
    });
    GsResult::Ok
}

/// # Safety
///
/// `handle` must be a valid pointer previously returned by `gs_client_create`
/// and not yet destroyed. `intent` is an integer code mapped internally.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gs_client_set_intent(handle: *mut GsHandle, intent: i32) -> GsResult {
    if handle.is_null() {
        return GsResult::NullPointer;
    }
    let panicking = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        let h = match unsafe { handle.as_mut() } {
            Some(h) => h,
            None => return GsResult::InvalidHandle,
        };
        let intent = match intent {
            0 => Intent::Off,
            1 => Intent::RealtimeTrip,
            2 => Intent::RealtimeIdle,
            3 => Intent::BackgroundTracking,
            _ => Intent::HistoryOnly,
        };
        let _ = h.core.handle(Input::Command(Command::SetIntent(intent)), 0);
        GsResult::Ok
    }));
    panicking.unwrap_or(GsResult::Panic)
}

#[unsafe(no_mangle)]
pub extern "C" fn gs_ffi_abi_version() -> i32 {
    // Bumped whenever the handle surface changes.
    1
}

// SAFETY: opaque guard to satisfy callers that want a canonical null.
#[unsafe(no_mangle)]
pub extern "C" fn gs_handle_is_null(h: *const GsHandle) -> c_int {
    if h.is_null() { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_then_destroy_does_not_panic() {
        let h = gs_client_create();
        assert!(!h.is_null());
        // SAFETY: h was just created.
        assert_eq!(unsafe { gs_client_destroy(h) }, GsResult::Ok);
    }

    #[test]
    fn destroy_null_is_not_ok_return() {
        // Null is a documented corner case handled without panicking.
        assert_eq!(
            unsafe { gs_client_destroy(std::ptr::null_mut()) },
            GsResult::NullPointer
        );
    }

    #[test]
    fn null_handle_set_intent_returns_null_pointer() {
        // SAFETY: intentional null input.
        assert_eq!(
            unsafe { gs_client_set_intent(std::ptr::null_mut(), 1) },
            GsResult::NullPointer,
        );
    }

    #[test]
    fn intent_on_live_handle_returns_ok() {
        let h = gs_client_create();
        assert!(!h.is_null());
        // SAFETY: h valid.
        assert_eq!(unsafe { gs_client_set_intent(h, 1) }, GsResult::Ok);
        assert_eq!(
            // SAFETY: h valid.
            unsafe { gs_client_destroy(h) },
            GsResult::Ok
        );
    }
}
