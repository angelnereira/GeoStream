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
    /// Event kinds produced by the core, waiting for `gs_client_poll_event`.
    ///
    /// Only the **kind** is queued, never a payload: a payload can carry
    /// coordinates and this buffer crosses into the host language, where
    /// redaction is not under our control (docs/10 §8.2).
    events: std::collections::VecDeque<&'static str>,
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
            events: std::collections::VecDeque::new(),
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

/// A platform fix crossing the C ABI. All fields are plain `f64`/`u8` so the
/// layout is stable and needs no allocator on the other side.
///
/// `has_*` flags stand in for the optional fields: C has no `Option<f64>` that
/// crosses an ABI portably.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GsPlatformFix {
    pub lat: f64,
    pub lon: f64,
    pub horizontal_accuracy_m: f64,
    pub altitude_m: f64,
    pub has_altitude: u8,
    pub speed_mps: f64,
    pub has_speed: u8,
    pub heading_deg: f64,
    pub has_heading: u8,
    pub timestamp_ms: u64,
    pub is_mock: u8,
}

/// Feeds a platform fix to the core.
///
/// # Safety
///
/// `handle` must be a valid pointer from `gs_client_create`; `fix` must point to
/// a readable `GsPlatformFix`. Returns the number of outputs produced, or a
/// negative `GsResult` code.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gs_client_location_fix(
    handle: *mut GsHandle,
    fix: *const GsPlatformFix,
) -> i32 {
    if handle.is_null() || fix.is_null() {
        return -(GsResult::NullPointer as i32);
    }
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| -> Result<i32, GsResult> {
        // SAFETY: both pointers were checked non-null; the caller upholds the
        // contract that they are valid for the duration of the call.
        let h = unsafe { handle.as_mut() }.ok_or(GsResult::InvalidHandle)?;
        let f = unsafe { fix.as_ref() }.ok_or(GsResult::NullPointer)?;
        let platform = gs_client_core::PlatformFix {
            lat: f.lat,
            lon: f.lon,
            horizontal_accuracy_m: f.horizontal_accuracy_m,
            altitude_m: (f.has_altitude != 0).then_some(f.altitude_m),
            vertical_accuracy_m: None,
            speed_mps: (f.has_speed != 0).then_some(f.speed_mps),
            speed_accuracy_mps: None,
            heading_deg: (f.has_heading != 0).then_some(f.heading_deg),
            heading_accuracy_deg: None,
            timestamp_ms: f.timestamp_ms,
            is_mock: f.is_mock != 0,
            source: "GPS",
            out_of_band: false,
        };
        let outputs = h
            .core
            .handle(gs_client_core::Input::LocationFix(platform), f.timestamp_ms);
        for output in &outputs {
            h.events.push_back(output_kind(output));
        }
        Ok(outputs.len() as i32)
    }));
    match result {
        Ok(Ok(n)) => n,
        Ok(Err(code)) => -(code as i32),
        Err(_) => -(GsResult::Panic as i32),
    }
}

/// The next timer the core wants, in milliseconds, or `-1` when there is none.
///
/// # Safety
///
/// `handle` must be a valid pointer from `gs_client_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gs_client_next_timer_ms(handle: *mut GsHandle) -> i64 {
    if handle.is_null() {
        return -1;
    }
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| -> Result<i64, GsResult> {
        // SAFETY: checked non-null; the caller upholds the handle contract.
        let h = unsafe { handle.as_mut() }.ok_or(GsResult::InvalidHandle)?;
        Ok(h.core.next_timer_ms().map_or(-1, |ms| ms as i64))
    }));
    match result {
        Ok(Ok(ms)) => ms,
        _ => -1,
    }
}

/// Polls one pending event kind into `buffer` (NUL-terminated).
///
/// Returns the number of bytes written (excluding the NUL), `0` when there is
/// nothing pending, or a negative `GsResult` code.
///
/// # Safety
///
/// `handle` must be valid; `buffer` must be writable for `capacity` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gs_client_poll_event(
    handle: *mut GsHandle,
    buffer: *mut u8,
    capacity: usize,
) -> i32 {
    if handle.is_null() || buffer.is_null() || capacity == 0 {
        return -(GsResult::NullPointer as i32);
    }
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| -> Result<i32, GsResult> {
        // SAFETY: checked non-null; the caller upholds the handle contract.
        let h = unsafe { handle.as_mut() }.ok_or(GsResult::InvalidHandle)?;
        let Some(kind) = h.events.pop_front() else {
            // Nothing pending: write a NUL and report 0.
            // SAFETY: buffer is writable for at least one byte (capacity > 0).
            unsafe { buffer.write(0) };
            return Ok(0);
        };
        let bytes = kind.as_bytes();
        let n = bytes.len().min(capacity - 1);
        // SAFETY: we copy at most `capacity - 1` bytes into a buffer the caller
        // promised is writable for `capacity` bytes, then NUL-terminate.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, n);
            buffer.add(n).write(0);
        }
        Ok(n as i32)
    }));
    match result {
        Ok(Ok(n)) => n,
        Ok(Err(code)) => -(code as i32),
        Err(_) => -(GsResult::Panic as i32),
    }
}

/// A stable, payload-free name for an output kind.
fn output_kind(output: &gs_client_core::Output) -> &'static str {
    use gs_client_core::Output;
    match output {
        Output::OpenTransport { .. } => "open_transport",
        Output::SendFrame(_) => "send_frame",
        Output::CloseTransport(_) => "close_transport",
        Output::Http(_, _) => "http",
        Output::ConfigureLocation(_) => "configure_location",
        Output::MonitorRegion(_) => "monitor_region",
        Output::StopMonitoringRegion(_) => "stop_monitoring_region",
        Output::ConfigureBleScan(_) => "configure_ble_scan",
        Output::Persist { .. } => "persist",
        Output::Emit(_) => "emit",
        Output::ScheduleTimer(_) => "schedule_timer",
    }
}

/// Command codes for `gs_client_command`.
pub const GS_CMD_START: i32 = 1;
pub const GS_CMD_STOP: i32 = 2;
pub const GS_CMD_FORCE_RECONNECT: i32 = 3;

/// Sends a lifecycle command to the core.
///
/// # Safety
///
/// `handle` must be a valid pointer from `gs_client_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gs_client_command(handle: *mut GsHandle, command: i32) -> GsResult {
    if handle.is_null() {
        return GsResult::NullPointer;
    }
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| -> Result<i32, GsResult> {
        // SAFETY: checked non-null; the caller upholds the handle contract.
        let h = unsafe { handle.as_mut() }.ok_or(GsResult::InvalidHandle)?;
        let command = match command {
            GS_CMD_START => gs_client_core::Command::Start,
            GS_CMD_STOP => gs_client_core::Command::Stop,
            GS_CMD_FORCE_RECONNECT => gs_client_core::Command::ForceReconnect,
            _ => return Err(GsResult::InvalidHandle),
        };
        let outputs = h.core.handle(gs_client_core::Input::Command(command), 0);
        for output in &outputs {
            h.events.push_back(output_kind(output));
        }
        Ok(outputs.len() as i32)
    }));
    match result {
        Ok(Ok(_)) => GsResult::Ok,
        Ok(Err(code)) => code,
        Err(_) => GsResult::Panic,
    }
}

/// Subscribes (`watch != 0`) or unsubscribes to a session.
///
/// # Safety
///
/// `handle` must be valid; `session_id` must point to `len` readable bytes of
/// UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gs_client_watch(
    handle: *mut GsHandle,
    session_id: *const u8,
    len: usize,
    watch: u8,
) -> GsResult {
    if handle.is_null() || session_id.is_null() || len == 0 {
        return GsResult::NullPointer;
    }
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| -> Result<(), GsResult> {
        // SAFETY: checked non-null; the caller promises `len` readable bytes.
        let bytes = unsafe { std::slice::from_raw_parts(session_id, len) };
        let session_id = std::str::from_utf8(bytes)
            .map_err(|_| GsResult::NullPointer)?
            .to_string();
        // SAFETY: checked non-null; the caller upholds the handle contract.
        let h = unsafe { handle.as_mut() }.ok_or(GsResult::InvalidHandle)?;
        let command = if watch != 0 {
            gs_client_core::Command::Watch { session_id }
        } else {
            gs_client_core::Command::Unwatch { session_id }
        };
        let outputs = h.core.handle(gs_client_core::Input::Command(command), 0);
        for output in &outputs {
            h.events.push_back(output_kind(output));
        }
        Ok(())
    }));
    match result {
        Ok(Ok(())) => GsResult::Ok,
        Ok(Err(code)) => code,
        Err(_) => GsResult::Panic,
    }
}

/// The last measured state, as the host passes it for a render decision.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GsLastMeasured {
    pub lat: f64,
    pub lon: f64,
    pub ts_ms: u64,
    pub speed_mps: f64,
    pub heading_deg: f64,
    pub accuracy_m: f64,
    pub plausibility: f64,
    pub stationary: u8,
    pub stale: u8,
}

/// The position to paint. `kind`: 0 measured, 1 filtered, 2 predicted.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GsRenderPosition {
    pub lat: f64,
    pub lon: f64,
    pub kind: i32,
    pub confidence_radius_m: f64,
}

/// Decides the position to paint (`docs/22` §3.6, ADR-0030).
///
/// A `PREDICTED` position **never leaves the client**: this function only writes
/// it into `out` for the renderer. It is never published, stored or logged.
///
/// # Safety
///
/// `last` must be readable; `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gs_client_render_position(
    last: *const GsLastMeasured,
    now_ms: u64,
    out: *mut GsRenderPosition,
) -> GsResult {
    if last.is_null() || out.is_null() {
        return GsResult::NullPointer;
    }
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| -> Result<(), GsResult> {
        // SAFETY: both pointers were checked non-null; the caller upholds the
        // contract that they are valid for the duration of the call.
        let l = unsafe { last.as_ref() }.ok_or(GsResult::NullPointer)?;
        let measured = gs_client_core::LastMeasured {
            lat: l.lat,
            lon: l.lon,
            ts_ms: l.ts_ms,
            speed_mps: l.speed_mps,
            heading_deg: l.heading_deg,
            accuracy_m: l.accuracy_m,
            plausibility: l.plausibility,
            stationary: l.stationary != 0,
            stale: l.stale != 0,
        };
        let rendered = gs_client_core::render_position(
            now_ms,
            measured,
            gs_client_core::PredictionPolicy::defaults(),
        );
        let kind = match rendered.kind {
            gs_client_core::RenderKind::Measured => 0,
            gs_client_core::RenderKind::Filtered => 1,
            gs_client_core::RenderKind::Predicted => 2,
        };
        // SAFETY: `out` is writable per the contract.
        unsafe {
            out.write(GsRenderPosition {
                lat: rendered.lat,
                lon: rendered.lon,
                kind,
                confidence_radius_m: rendered.confidence_radius_m,
            });
        }
        Ok(())
    }));
    match result {
        Ok(Ok(())) => GsResult::Ok,
        Ok(Err(code)) => code,
        Err(_) => GsResult::Panic,
    }
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
    fn location_fix_is_accepted_and_next_timer_is_sane() {
        let h = gs_client_create();
        assert!(!h.is_null());
        let fix = GsPlatformFix {
            lat: 8.9824,
            lon: -79.5199,
            horizontal_accuracy_m: 8.0,
            altitude_m: 0.0,
            has_altitude: 0,
            speed_mps: 4.0,
            has_speed: 1,
            heading_deg: 90.0,
            has_heading: 1,
            timestamp_ms: 1_000,
            is_mock: 0,
        };
        // SAFETY: h is valid; &fix is a valid pointer for the call.
        let produced = unsafe { gs_client_location_fix(h, &fix) };
        assert!(produced >= 0, "fix must be accepted, got {produced}");
        // SAFETY: h is valid.
        assert_eq!(unsafe { gs_client_next_timer_ms(h) }, -1);
        // SAFETY: h is valid.
        assert_eq!(unsafe { gs_client_destroy(h) }, GsResult::Ok);
    }

    #[test]
    fn events_are_polled_as_kinds_without_payloads() {
        let h = gs_client_create();
        let fix = GsPlatformFix {
            lat: 8.9824,
            lon: -79.5199,
            horizontal_accuracy_m: 8.0,
            altitude_m: 0.0,
            has_altitude: 0,
            speed_mps: 0.0,
            has_speed: 0,
            heading_deg: 0.0,
            has_heading: 0,
            timestamp_ms: 1000,
            is_mock: 0,
        };
        // SAFETY: h valid; &fix valid for the call.
        let _ = unsafe { gs_client_location_fix(h, &fix) };
        let mut buf = [0u8; 64];
        // SAFETY: h valid; buf is writable for 64 bytes.
        let n = unsafe { gs_client_poll_event(h, buf.as_mut_ptr(), buf.len()) };
        assert!(n > 0, "at least one event must be queued, got {n}");
        let kind = std::str::from_utf8(&buf[..n as usize]).unwrap();
        // The kind is a fixed name, never a coordinate.
        assert!(!kind.contains("8.98"), "event leaked a coordinate: {kind}");
        // SAFETY: h valid; buf writable.
        let drained = unsafe { gs_client_poll_event(h, buf.as_mut_ptr(), buf.len()) };
        assert_eq!(drained, 0, "the queue must drain");
        // SAFETY: h valid.
        assert_eq!(unsafe { gs_client_destroy(h) }, GsResult::Ok);
    }

    #[test]
    fn render_position_extrapolates_when_moving() {
        let last = GsLastMeasured {
            lat: 8.9824,
            lon: -79.5199,
            ts_ms: 0,
            speed_mps: 12.0,
            heading_deg: 0.0,
            accuracy_m: 10.0,
            plausibility: 1.0,
            stationary: 0,
            stale: 0,
        };
        let mut out = GsRenderPosition {
            lat: 0.0,
            lon: 0.0,
            kind: -1,
            confidence_radius_m: 0.0,
        };
        // SAFETY: &last and &mut out are valid for the call.
        assert_eq!(
            unsafe { gs_client_render_position(&last, 1000, &mut out) },
            GsResult::Ok
        );
        assert_eq!(out.kind, 2, "a moving entity must be PREDICTED");
        assert!(out.lat > last.lat, "the prediction must advance northward");
    }

    #[test]
    fn render_position_is_measured_when_stationary() {
        let last = GsLastMeasured {
            lat: 8.9824,
            lon: -79.5199,
            ts_ms: 0,
            speed_mps: 12.0,
            heading_deg: 0.0,
            accuracy_m: 10.0,
            plausibility: 1.0,
            stationary: 1,
            stale: 0,
        };
        let mut out = GsRenderPosition {
            lat: 0.0,
            lon: 0.0,
            kind: -1,
            confidence_radius_m: 0.0,
        };
        // SAFETY: valid pointers for the call.
        assert_eq!(
            unsafe { gs_client_render_position(&last, 1000, &mut out) },
            GsResult::Ok
        );
        assert_ne!(out.kind, 2, "a stationary entity must not be predicted");
    }

    #[test]
    fn command_and_watch_are_accepted() {
        let h = gs_client_create();
        // SAFETY: h valid.
        assert_eq!(unsafe { gs_client_command(h, GS_CMD_START) }, GsResult::Ok);
        let session = b"ses_00000000000000000000000000000000";
        // SAFETY: h valid; session is a valid readable slice.
        assert_eq!(
            unsafe { gs_client_watch(h, session.as_ptr(), session.len(), 1) },
            GsResult::Ok
        );
        // SAFETY: h valid; session readable.
        assert_eq!(
            unsafe { gs_client_watch(h, session.as_ptr(), session.len(), 0) },
            GsResult::Ok
        );
        // An unknown command code is rejected.
        // SAFETY: h valid.
        assert_eq!(
            unsafe { gs_client_command(h, 999) },
            GsResult::InvalidHandle
        );
        // SAFETY: h valid.
        assert_eq!(unsafe { gs_client_destroy(h) }, GsResult::Ok);
    }

    #[test]
    fn null_pointers_are_rejected_without_panicking() {
        // SAFETY: intentional null inputs.
        assert!(unsafe { gs_client_location_fix(std::ptr::null_mut(), std::ptr::null()) } < 0);
        let h = gs_client_create();
        // SAFETY: valid handle, null fix.
        assert!(unsafe { gs_client_location_fix(h, std::ptr::null()) } < 0);
        // SAFETY: valid handle.
        assert_eq!(unsafe { gs_client_next_timer_ms(h) }, -1);
        // SAFETY: valid handle.
        assert_eq!(unsafe { gs_client_destroy(h) }, GsResult::Ok);
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
