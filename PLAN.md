# Dahua Pager — Implementation Plan

## 1. Goal

Build a Rust-based, standalone Dahua NVR event pager for the existing ESP32-S3 handheld hardware from [crd-esp32](https://github.com/mtojek/crd-esp32). Develop and test primarily on macOS using a faithful device simulator, then deploy the same application logic and UI renderer to the ESP32-S3.

The device connects to the NVR, receives detection events, checks whether detected people or vehicles intersect user-selected cells in a camera-specific grid, and displays, records, and optionally sounds an alarm. Multiple cameras may alarm concurrently; the UI shows one alarm at a time and allows browsing the others.

## 2. Hardware and constraints

- Target: ESP32-S3 handheld from `crd-esp32`; use that repository as the authoritative pinout and peripheral reference, rather than assuming a generic ESP32 board.
- Display: ILI9488, **480 × 320**, landscape, SPI.
- Touch: XPT2046.
- Inputs: four physical buttons, joystick, touchscreen. **Two buttons below the screen = PREV / NEXT; two buttons on the right = SILENCE / CLOSE.** Confirm exact GPIO-to-position mapping against the hardware before implementation.
- Sound: MAX98357A I2S amplifier and speaker; audible alarm pattern.
- Storage: microSD card on the device; regular local files on macOS.
- Wi-Fi: ESP32-S3 wireless networking.
- PSRAM: 8 MB available on the existing hardware; benchmark actual memory and image-decoding behavior.
- Keep UI responsive during NVR disconnects, JPEG decode, SD writes, and audio playback.

## 3. Core system states

Three **global** modes, distinct from individual event lifecycle states:

| Mode | Process incoming events | Record new events/snapshots | Sound new alarms |
| --- | --- | --- | --- |
| `ARMED` | Yes | Yes | Yes |
| `SILENCED` | Yes | Yes | No |
| `DISARMED` | No | No | No |

- `SILENCE` immediately stops current audio and switches `ARMED → SILENCED`. New events in `SILENCED` still appear in the queue and history but never restart the siren.
- `DISARMED` ignores incoming detections and does not create new records. Existing history remains available. The device may maintain an NVR connection for readiness, but it must not process detections while disarmed.
- Switching back to `ARMED` enables sound for **future** new alarms, not historical events.
- An audible alarm automatically stops after **5 minutes**, but its entry stays visible until manually closed. **Decision to confirm:** whether the 5-minute audio timeout also changes the global mode to `SILENCED` or only silences the current audio episode. Initial implementation should keep the mode `ARMED` and stop only the current audio episode; future distinct events may sound again.
- `CLOSE` dismisses only the currently displayed alarm from the active queue; it never deletes its persisted history record. An alarm stays on screen until manually closed even if the NVR event has ended.
- Schedule controls global arming/disarming; manually silenced mode must have a documented precedence rule. Proposed: `SILENCED` persists until manual rearm or the next scheduled transition into an armed period.

## 4. NVR event ingestion

- Connect to Dahua `cgi-bin/eventManager.cgi` using authenticated HTTP and consume the event stream incrementally, with reconnection, bounded backoff, and heartbeat/timeout handling.
- Support **6 cameras** in the initial configuration, identified by NVR channel and friendly label.
- Discover or configure available event types and object classifications (person, vehicle); do not hard-code unsupported Dahua capabilities.
- Capture representative **real** NVR event payloads early. Confirm event start/stop semantics, channel IDs, object classification, object coordinates/bounding boxes, and event identifiers. The endpoint alone does **not** guarantee positional metadata.
- If the event stream lacks reliable object coordinates, implement an explicit alternative integration or restrict spatial filtering to NVR-defined IVS zones; do not pretend that event-only notifications can locate an object on a 32 × 20 grid.
- Separate transport parsing from normalized events: camera ID, event type, object type, timestamp, phase, optional event/object ID, optional normalized bounding box or polygon, and raw metadata for diagnostics.
- Preserve the order in which events are received from the API for alarm ordering, with a monotonically increasing local sequence number.
- Network interruption must not freeze UI. Expose disconnected/reconnecting status.
- Never store NVR credentials in logs or event history.

## 5. Zone configuration

- Each camera has its own selected set of cells on a configurable grid; initial constants: **32 columns × 20 rows**.
- Grid dimensions are configurable in code (compile-time/default constants), not necessarily editable through the device UI.
- On-device zone editor shows a camera snapshot with an overlaid grid and allows selecting/deselecting cells using touch. Add zoom/pan or a magnified selection mode because each cell is very small on a 480 × 320 screen.
- Normalize object geometry to the camera frame and test intersection with selected cells. Define the matching rule explicitly (proposed: bounding-box intersection with any enabled cell; optionally configurable minimum overlap later).
- Preserve aspect ratio of snapshots; correctly transform between image coordinates, screen coordinates, and grid coordinates. Letterboxed margins are not selectable cells.
- The camera preview may be a still JPEG snapshot; live video is not required.
- Zone changes persist atomically to JSON.

## 6. Alarm queue, event lifecycle and history

- Multiple cameras can produce overlapping alarms. The **first received qualifying event** is the first active alarm displayed; subsequent qualifying events are queued in arrival order.
- Show exactly one camera snapshot/alarm at a time; `PREV` and `NEXT` navigate the active alarm list without changing global mode.
- Each new logical alarm episode creates one history record and captures **one snapshot at its start**. Do not repeatedly capture or save snapshots while the same episode remains active/ringing.
- Distinguish NVR `Start`/`Stop` or equivalent transitions, and deduplicate repeated updates for the same camera/object/event. Define a bounded fallback deduplication window for events without reliable IDs or stop messages.
- A distinct alarm from another camera is always eligible for its own record and snapshot, even while an earlier alarm is sounding or displayed.
- Persist at least: unique ID, local sequence, camera/channel, classification, event type, UTC start time, optional end time, zone/cell match, snapshot path, and acknowledgement/close state.
- Persist history on microSD (macOS: local directory), and provide an on-device history browser with snapshot, date/time, and camera.
- Use atomic writes where possible (temporary file + rename), validate files at startup, and tolerate abrupt power loss or a missing/full SD card.
- Retention: configurable maximum storage bytes and/or number of records; prune **oldest complete records and their snapshots** first. Reserve free space for new alarms and avoid deleting active/in-progress records. Provide diagnostics if storage is unavailable.
- Prefer an append-friendly metadata index or per-event JSON records over rewriting one large history JSON file for every alarm. The main configuration file remains pretty-printed JSON.

## 7. UI and controls

Use **`embedded-graphics`** as the shared drawing abstraction, with platform-specific display adapters. Target **480 × 320 physical pixels** on both platforms. The macOS window may scale by an integer factor but must retain the same logical resolution and touch mapping.

### Alarm screen layout (initial pixel budget)

| Region | Pixel bounds | Height |
| --- | --- | --- |
| Status bar: mode, connection, time | y=0–23 | 24 |
| Snapshot / image viewport | y=24–263 | 240 |
| Alarm metadata and queue position | y=264–291 | 28 |
| Context/help strip | y=292–319 | 28 |

The 480 × 240 image viewport is **not** the camera's native aspect ratio: preserve image aspect ratio and letterbox/crop intentionally. In the zone editor, preserve the full camera frame rather than cropping because coordinates must stay accurate. The snapshot is the dominant visual element. Do not add four fake buttons below the display: physical buttons exist outside the LCD.

Physical mapping:

- **Below screen:** `PREV`, `NEXT` (browse active alarms; contextual navigation in other screens).
- **Right side:** `SILENCE`, `CLOSE` (quick alarm actions; contextual actions in other screens).
- Touch: open alarm details, select cameras, edit grid, navigate settings/history.
- Joystick: navigate menus and grid editing; map its click if present.
- Display current global mode prominently and provide an explicit way to select `ARMED`, `SILENCED`, or `DISARMED` in a menu. Confirm whether a long press should be a quick disarm shortcut.
- Avoid accidental destructive changes: closing an alarm must not erase its history.

Screens: main/status, active alarms, alarm detail/snapshot, history list/detail, per-camera zone editor, schedule, settings/connection diagnostics.

## 8. Scheduling and clock

- One **global** weekly schedule shared by all 6 cameras, with one or more armed time windows per day.
- Allow manual mode selection independently of schedule; define deterministic precedence and show whether current mode is manual or schedule-derived.
- Synchronize ESP32 time via NTP/SNTP (default `pool.ntp.org`, configurable). Use `Europe/Warsaw` as the default display timezone and handle DST correctly.
- Persist timestamps in UTC and render local time in the UI.
- On startup without valid time, continue networking/monitoring as allowed but mark timestamps as untrusted; reconcile or annotate them after synchronization. Do not fabricate accurate wall-clock timestamps.
- macOS uses the system clock through the same clock interface; tests use an injected deterministic fake clock.

## 9. Configuration and storage

- Human-readable, **pretty-printed JSON** (`config.json`) on SD for ESP32 and in a local directory on macOS.
- Configuration covers Wi-Fi SSID/credentials, NVR address/port/auth, 6 camera/channel mappings, selected grid cells, event/object filters, NTP/timezone, schedule, audio/retention limits, and optional snapshot settings.
- Version the configuration schema; validate on startup and display actionable errors. Do not silently overwrite invalid configuration.
- Save changes atomically and avoid unnecessary SD writes. Do not expose secrets in UI diagnostics or logs.
- Suggested directories: `config.json`, `history/`, `snapshots/`, `logs/`.
- **Security consideration:** credentials on removable SD are sensitive. Document this risk and consider optional encryption or credential provisioning later.

## 10. Rust workspace architecture

Suggested layout (refine after validating embedded toolchain and driver support):

```text
crates/
  pager-core/          # pure state machines, scheduling, zones, event lifecycle
  pager-dahua/         # Dahua event transport/parser, snapshot API
  pager-ui/            # embedded-graphics screens, navigation, input model
  pager-storage/       # config/history schemas, retention, storage interfaces
  pager-platform/      # interfaces for clock, network, audio, input, display, files
apps/
  pager-macos/         # simulator window, keyboard/mouse, local storage
  pager-esp32/         # ESP32-S3 firmware, hardware drivers, SD, Wi-Fi
  mock-dahua/          # deterministic fake NVR with events and snapshots
fixtures/
  dahua-events/
tests/
  e2e/
```

- Core logic is platform-agnostic Rust, with minimal hardware-specific code.
- Define interfaces for display/framebuffer, touch/buttons/joystick, sound, clock, NVR HTTP/event stream, snapshot retrieval, and storage.
- Evaluate `embedded-graphics-simulator` or a small `winit`/SDL-backed framebuffer adapter on macOS; choose based on touch/mouse/button support and fidelity.
- JPEG decode and RGB565 conversion must be benchmarked on the ESP32-S3. Use bounded buffers, PSRAM where appropriate, and do not assume a full-resolution image fits internal RAM.
- Choose the ESP32 Rust toolchain and networking stack only after a hardware spike; compare `esp-idf-svc` and `esp-hal` suitability for Wi-Fi, SD, I2S, touch, and display. Avoid promising a pure `no_std` build prematurely.

## 11. Testing strategy

- Unit tests: grid geometry and coordinate transforms; schedule including DST; alarm mode transitions; timeout; deduplication; queue ordering; retention; config validation/migration.
- Parser tests using captured/redacted Dahua event fixtures and malformed/chunked HTTP streams.
- Integration tests with mock Dahua event stream and snapshot endpoint: concurrent alarms across cameras, reconnects, event repeats, absent stop messages, missing snapshots, and auth/network failures.
- E2E on macOS: launch simulated device + mock NVR; inject touch/button actions; assert UI state, sound state, records, and snapshots. Use deterministic fake time for 5-minute timeout and schedule transitions.
- Test `ARMED`, `SILENCED`, and `DISARMED` separately, including a second camera alarming after the first is silenced.
- Storage fault tests: full SD, missing card, interrupted write, corrupt config/history, rotation while active alarms exist.
- Hardware smoke tests: display, touch calibration, all four physical buttons, joystick, I2S, SD, Wi-Fi, NTP, and real NVR.
- CI runs formatting, linting, core tests, parser/integration tests, and macOS simulator/E2E where runners permit; ESP32 cross-build and hardware checks separately.

## 12. Milestones

1. **Hardware/API discovery:** verify `crd-esp32` wiring; capture Dahua event payloads and snapshot behavior; prove spatial metadata availability.
2. **Rust foundation:** workspace, shared models, fake clock, mock NVR, JSON schema, unit-test harness.
3. **macOS simulator:** pixel-faithful 480 × 320 framebuffer, touch and four buttons, snapshot rendering.
4. **Detection and zones:** parser, normalized geometry, 32 × 20 editor, per-camera zone matching, tests.
5. **Alarm workflow:** three global modes, multiple alarms, queue navigation, audio timeout, close behavior, schedule.
6. **Persistence:** snapshot-at-start, history browser, deduplication, rotation, fault recovery.
7. **ESP32 port:** display/touch/buttons/joystick, Wi-Fi/NVR, NTP, SD, JPEG, I2S; performance tuning.
8. **Hardening:** reconnection, long-running soak tests, six-camera concurrency, end-to-end tests, docs.

## 13. Open questions / validation gates

- **Dahua metadata:** does this exact NVR expose usable object coordinates via `eventManager.cgi`, or is an additional endpoint/stream required?
- **Audio timeout:** should 5 minutes only stop the current sound, or switch the global mode to `SILENCED`?
- **Schedule precedence:** confirm whether scheduled arming overrides manual `SILENCED` and/or `DISARMED` at the next transition.
- **Alarm episodes:** how should repeated Start messages, missing Stop messages, and new detections from the same camera be grouped?
- **Hardware:** verify exact physical GPIO mapping for bottom/right buttons and Rust peripheral-driver compatibility.
- **Snapshots:** verify supported Dahua snapshot CGI path, image size, authentication, latency, and whether a snapshot can be retrieved reliably at alarm start.
- **UI:** validate touch usability for 32 × 20 grid; add magnification if needed.

## 14. Acceptance criteria

- A single Rust core and shared `embedded-graphics` UI run on macOS simulator and ESP32-S3 at 480 × 320.
- All six cameras can be configured; their events are processed in arrival order.
- A detected person/vehicle in enabled cells generates a snapshot-backed history record when monitoring is enabled.
- Independent camera alarms remain browsable while only one is shown at a time.
- `SILENCE` stops sound globally without stopping event recording; `DISARMED` stops new event recording.
- Alarm entries remain visible until `CLOSE`, and closing never deletes historical evidence.
- Audio stops after 5 minutes without acknowledgement.
- Weekly schedule, NTP clock, pretty JSON configuration, SD storage, and automatic history rotation work.
- Automated unit, integration, and mock-NVR E2E tests cover normal and failure paths.
