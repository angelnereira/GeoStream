/* abi_smoke.c — a C consumer of the GeoStream C ABI.
 *
 * It links against the `gs-ffi` cdylib and exercises the documented surface:
 * create, set intent, feed a fix, ask for the next timer, destroy. If this
 * compiles and links, the ABI is usable from C without a Rust shim, which is
 * the whole point of `gs-ffi` (ADR-0031).
 *
 * Build (see .github/workflows/ci.yml):
 *   cargo build -p gs-ffi --release
 *   cc abi_smoke.c -I../../include -L../../../target/release -lgs_ffi -o abi_smoke
 *   LD_LIBRARY_PATH=../../../target/release ./abi_smoke
 */
#include "gs_ffi.h"
#include <stdio.h>
#include <string.h>

int main(void) {
    if (gs_ffi_abi_version() < 1) {
        fprintf(stderr, "unexpected ABI version\n");
        return 1;
    }

    GsHandle *handle = gs_client_create();
    if (handle == NULL) {
        fprintf(stderr, "gs_client_create returned NULL\n");
        return 1;
    }
    if (gs_handle_is_null(handle) != 0) {
        fprintf(stderr, "a live handle reported as NULL\n");
        return 1;
    }

    if (gs_client_set_intent(handle, 1) != GS_OK) {
        fprintf(stderr, "set_intent failed\n");
        return 1;
    }

    if (gs_client_command(handle, GS_CMD_START) != GS_OK) {
        fprintf(stderr, "start command failed\n");
        return 1;
    }

    const char *session = "ses_00000000000000000000000000000000";
    if (gs_client_watch(handle, (const uint8_t *)session, strlen(session), 1) != GS_OK) {
        fprintf(stderr, "watch failed\n");
        return 1;
    }

    GsPlatformFix fix = {0};
    fix.lat = 8.9824;
    fix.lon = -79.5199;
    fix.horizontal_accuracy_m = 8.0;
    fix.has_altitude = 0;
    fix.has_speed = 0;
    fix.has_heading = 0;
    fix.timestamp_ms = 1000;
    fix.is_mock = 0;

    int32_t produced = gs_client_location_fix(handle, &fix);
    if (produced < 0) {
        fprintf(stderr, "location_fix rejected the fix: %d\n", produced);
        return 1;
    }

    if (gs_client_next_timer_ms(handle) != -1) {
        fprintf(stderr, "no timer was expected yet\n");
        return 1;
    }

    /* At least one output event is queued by the fix; poll it as a kind name. */
    uint8_t buffer[64];
    int32_t n = gs_client_poll_event(handle, buffer, sizeof(buffer));
    if (n <= 0) {
        fprintf(stderr, "expected a queued event, got %d\n", n);
        return 1;
    }
    if (strstr((char *)buffer, "8.98") != NULL) {
        fprintf(stderr, "event leaked a coordinate: %s\n", buffer);
        return 1;
    }

    /* Feed a couple more fixes and drain the whole queue in a loop: the host
     * event loop is "poll until empty", so the ABI must support it. */
    fix.timestamp_ms = 2000;
    if (gs_client_location_fix(handle, &fix) < 0) {
        fprintf(stderr, "second fix rejected\n");
        return 1;
    }
    fix.timestamp_ms = 3000;
    if (gs_client_location_fix(handle, &fix) < 0) {
        fprintf(stderr, "third fix rejected\n");
        return 1;
    }
    int drained = 0;
    for (;;) {
        int32_t got = gs_client_poll_event(handle, buffer, sizeof(buffer));
        if (got < 0) {
            fprintf(stderr, "poll failed: %d\n", got);
            return 1;
        }
        if (got == 0) {
            break;
        }
        drained++;
        if (drained > 1000) {
            fprintf(stderr, "event loop did not terminate\n");
            return 1;
        }
    }
    if (drained == 0) {
        fprintf(stderr, "expected queued events after three fixes\n");
        return 1;
    }

    if (gs_client_location_fix(NULL, &fix) >= 0) {
        fprintf(stderr, "a NULL handle must be rejected\n");
        return 1;
    }

    /* Visual prediction is a pure function of the last measurement. */
    GsLastMeasured last = {0};
    last.lat = 8.9824;
    last.lon = -79.5199;
    last.ts_ms = 0;
    last.speed_mps = 12.0;
    last.heading_deg = 0.0;
    last.accuracy_m = 10.0;
    last.plausibility = 1.0;
    GsRenderPosition rendered = {0};
    if (gs_client_render_position(&last, 1000, &rendered) != GS_OK) {
        fprintf(stderr, "render_position failed\n");
        return 1;
    }
    if (rendered.kind != 2) {
        fprintf(stderr, "a moving entity must be PREDICTED, got %d\n", rendered.kind);
        return 1;
    }

    if (gs_client_destroy(handle) != GS_OK) {
        fprintf(stderr, "destroy failed\n");
        return 1;
    }

    printf("gs-ffi C ABI smoke: OK\n");
    return 0;
}
