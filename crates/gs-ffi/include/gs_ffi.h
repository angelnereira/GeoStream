/* gs_ffi.h — C ABI of the GeoStream client core (docs/26 §3.2, ADR-0031).
 *
 * This is the only surface that uses `unsafe` in the workspace. Every function
 * is `catch_unwind`-guarded: no panic crosses the boundary. Handles are opaque;
 * the caller owns them from `gs_client_create` until `gs_client_destroy`.
 */
#ifndef GS_FFI_H
#define GS_FFI_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Result codes returned across the boundary. */
typedef enum {
    GS_OK = 0,
    GS_INVALID_HANDLE = 1,
    GS_NULL_POINTER = 2,
    GS_TRANSPORT_ERROR = 3,
    GS_PANIC = 99
} GsResult;

/* Opaque handle. Never dereferenced by the host. */
typedef struct GsHandle GsHandle;

/* A platform fix. `has_*` flags stand in for the optional fields: C has no
 * `Option<f64>` that crosses an ABI portably. */
typedef struct {
    double lat;
    double lon;
    double horizontal_accuracy_m;
    double altitude_m;
    uint8_t has_altitude;
    double speed_mps;
    uint8_t has_speed;
    double heading_deg;
    uint8_t has_heading;
    uint64_t timestamp_ms;
    uint8_t is_mock;
} GsPlatformFix;

/* Bumped whenever the handle surface changes. */
int32_t gs_ffi_abi_version(void);

/* Construct a handle. Returns NULL on allocation failure or panic. */
GsHandle *gs_client_create(void);

/* Destroy a handle. The contract is single owner, no concurrent calls. */
GsResult gs_client_destroy(GsHandle *handle);

/* Set the integrator intent (0 off, 1 trip, 2 idle, 3 background, 4 history). */
GsResult gs_client_set_intent(GsHandle *handle, int32_t intent);

/* Feed a platform fix. Returns the number of outputs produced, or a negative
 * GsResult code. */
int32_t gs_client_location_fix(GsHandle *handle, const GsPlatformFix *fix);

/* The next timer the core wants, in milliseconds, or -1 when there is none. */
int64_t gs_client_next_timer_ms(GsHandle *handle);

/* Poll one pending event kind into `buffer` (NUL-terminated). Returns the number
 * of bytes written (excluding the NUL), 0 when nothing is pending, or a
 * negative GsResult code. The payload is only a fixed kind name: a payload can
 * carry coordinates and this buffer crosses into the host language. */
int32_t gs_client_poll_event(GsHandle *handle, uint8_t *buffer, size_t capacity);

/* 1 when the handle is NULL, 0 otherwise. */
int32_t gs_handle_is_null(const GsHandle *handle);

#ifdef __cplusplus
}
#endif

#endif /* GS_FFI_H */
