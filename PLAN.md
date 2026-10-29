# Dahua Pager — Implementation Plan

## 1. Goal

Build a Rust-based, standalone Dahua NVR event pager for the existing ESP32-S3 handheld hardware from [crd-esp32](https://github.com/mtojek/crd-esp32). Develop and test primarily on macOS using a faithful device simulator, then deploy the same application logic and UI renderer to the ESP32-S3.

The device connects to the NVR, receives detection events, checks whether detected people or vehicles intersect user-selected cells in a camera-specific grid, and displays, records, and optionally sounds an alarm. Multiple cameras may alarm concurrently; the UI shows one alarm at a time and allows browsing the others.

## 2. Hardware and constraints

- Target: ESP32-S3 handheld from `crd-esp32`; use that repository as the authoritative pinout and peripheral reference, rather than assuming a generic ESP32 board.
- Display: ILI9488, **480 × 320**, landscape, SPI.
- Touch: XPT2046.
- Inputs: four physical buttons, joystick, touchscreen. **Two buttons below the screen = PREV / NEXT; two buttons on the right perform contextual actions such as ACK / BACK and Sleep / Mode.** Confirm exact GPIO-to-position mapping against the hardware before implementation.
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
| `MUTED` | Yes | Yes | No |
| `DISARMED` | No | No | No |

- Selecting `MUTED` immediately stops current audio. New events in `MUTED` still appear in the queue and history but never restart the siren.
- `DISARMED` stops current audio and ignores new detections without creating new records. Existing alarms and history remain available. The connection may remain open, and lifecycle messages may finish existing records without creating new alarms.
- Switching back to `ARMED` enables sound for **future** new alarms, not historical events.
- **Accepted audio behavior:** use a loud, repeating pager pattern intended to wake the user at night (initial pattern: three beeps followed by a two-second pause). Provide configurable volume and a sound-test action; validate the pattern and loudness on real hardware.
- Audio runs in a global episode lasting at most **5 minutes**, measured from its first qualifying alarm. New alarms received during playback join that episode without restarting or extending its deadline. NVR Stop messages do not silence it. At timeout, retire all sound requests from that episode and keep mode `ARMED`; a distinct new alarm may start a new episode. Existing queue entries do not restart audio.
- **ACK acknowledges all alarms received up to the moment of the action**, regardless of camera or selected entry, and immediately stops all current audio. Keep those records and snapshots available; do not delete them. ACK does not change the global mode. A distinct later detection can ring again in `ARMED`, but stays silent in `MUTED`.
- After ACK, the alarm screen action changes to `Back`; keep the displayed snapshot until the user leaves. Returning from the screen never deletes history. NVR Stop does not acknowledge an alarm or stop its audio. Navigation does not acknowledge alarms.
- `ALARM` is a screen/attention state, not a fourth global mode. Keep mode, acknowledgement and detection lifecycle separate.
- Schedule controls `ARMED`, `MUTED` and `DISARMED`. Scheduled arming or manual rearming enables sound only for future alarms.

## 4. NVR event ingestion

- Connect to Dahua `cgi-bin/eventManager.cgi` using authenticated HTTP and consume the event stream incrementally, with reconnection, bounded backoff, and heartbeat/timeout handling.
- Support **6 cameras** in the initial configuration, identified by NVR channel and friendly label.
- Discover or configure available event types and object classifications (person, vehicle); do not hard-code unsupported Dahua capabilities.
- Capture representative **real** NVR event payloads early. Confirm event start/stop semantics, channel IDs, object classification, object coordinates/bounding boxes, and event identifiers. The endpoint alone does **not** guarantee positional metadata.
- Resolve the exact protocol and coordinate source during the NVR integration task. The supplied VideoMotion Start/Stop sample contains channel, region and time information, but no explicit object geometry or person/vehicle classification. Capture additional messages/endpoints as needed. The pager's own 32 × 20 grid remains the target; replacing it with NVR-defined IVS zones requires a separately accepted scope change.
- Separate transport parsing from normalized events: camera ID, event type, object type, timestamp, phase, optional event/object ID, optional normalized bounding box or polygon, and raw metadata for diagnostics.
- Preserve the order in which events are received from the API for alarm ordering, with a monotonically increasing local sequence number.
- Network interruption must not freeze UI. Expose disconnected/reconnecting status.
- Never store NVR credentials in logs or event history.
- Never publish real configuration, camera/location names, device identifiers or raw private NVR responses. Use neutral synthetic location names (Backyard, Garage, Pool, etc.) and anonymized fixtures in code, tests, documentation and shared output. Keep live configuration and captures local and gitignored.

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
- Each new logical detection episode creates one history record and starts one snapshot request immediately, independently of the global audio episode. This does not guarantee an image from the exact detection instant. Do not repeatedly capture snapshots for updates to the same detection episode. A failed request must not block the alarm or metadata record; show an image placeholder and persist the failure status.
- Distinguish NVR `Start`/`Stop` or equivalent transitions, and deduplicate repeated updates for the same camera/object/event. Define a bounded fallback deduplication window for events without reliable IDs or stop messages.
- A distinct alarm from another camera is always eligible for its own record and snapshot, even while an earlier alarm is sounding or displayed.
- Persist at least: unique ID, local sequence, camera/channel, classification, event type, optional UTC start/end times with source and trust status, boot/session ID and monotonic start time when UTC is unknown, zone/cell match, optional snapshot path and retrieval status, and acknowledgement/close state.
- Persist history on microSD (macOS: local directory), and provide an on-device history browser with snapshot, date/time, and camera.
- Use atomic writes where possible (temporary file + rename), validate files at startup, and tolerate abrupt power loss or a missing/full SD card.
- Retention: configurable maximum storage bytes and/or number of records; prune **oldest complete records and their snapshots** first. Reserve free space for new alarms and avoid deleting active/in-progress records. Provide diagnostics if storage is unavailable.
- Start SD cleanup at **90% filesystem occupancy**. Proposed implementation defaults: clean down to 85%; macOS uses an artificial configurable directory budget of 1 GiB with the same thresholds. Only delete pager-owned records and snapshots. Check capacity before writes, including temporary files; if protected records or other files prevent cleanup, report degraded persistence and continue alarming without claiming successful saves.
- The disk threshold does not bound the RAM queue. Proposed default for review before implementation: 128 unclosed alarms; preserve existing entries, count and visibly report overflow, and continue processing sound and history for new qualifying detections where resources permit. All ingestion, snapshot and storage work queues must also have explicit bounded capacities and documented overflow behavior.
- Prefer an append-friendly metadata index or per-event JSON records over rewriting one large history JSON file for every alarm. The main configuration file remains pretty-printed JSON.

## 7. UI and controls

Screen sketches and interaction rules: [docs/ui](docs/ui/README.md).

Use **`embedded-graphics`** as the shared drawing abstraction, with platform-specific display adapters. Target **480 × 320 physical pixels** on both platforms. The macOS simulator uses **scale 1:1**, with an additional case area outside the LCD for the physical controls.

### Accepted screens

| Screen | Header | Main area | Actions |
| --- | --- | --- | --- |
| Standby | Mode, connection status, time | Empty while waiting | Right: red Sleep and blue Mode (ARMED → MUTED → DISARMED → ARMED); bottom: History and Settings |
| History | Mode, History, time | One snapshot with detection rectangle | Right: Back; bottom: when/what, Prev/Next and entry count. **No Clear or manual delete action.** |
| Alarm | ALARM, connection status, time | Snapshot with detection rectangle | Right: ACK, replaced by Back after acknowledgement; bottom: when/what. Browse overlapping alarms with Prev/Next. ACK always covers all received alarms. |
| Settings | Mode, Settings, time | Ustawienia obszaru; Harmonogram; audio volume/test and diagnostics | Back |
| Area camera list | Mode, Ustawienia obszaru, time | Camera names/indices discovered from NVR | Select camera; Back |
| Area editor | Mode, selected camera, time | Snapshot with 32 × 20 selected-cell grid and magnification | Save / Back |
| Schedule | Mode, Harmonogram, time | Weekly periods assigning ARMED, MUTED or DISARMED | Enable/disable schedule; edit periods; Save / Back |

Reserve a 24-pixel header and a bottom metadata row where needed. Physical action buttons and their captions sit outside the LCD: bottom-right green History then yellow Settings side by side; right red Sleep above blue Mode on Standby. The desktop-only case is 624 × 432 logical pixels around an inset 480 × 320 LCD. The header uses red for ARMED, amber for MUTED and green for DISARMED, with white bold mode text and no badge. NVR connection state affects only the status text; connecting does not turn the header blue. Refine exact pixel widths with the first rendered layouts. Fit snapshots with their aspect ratio preserved; do not crop the area editor. Main-screen emptiness is intentional, not a camera-preview placeholder.

- Physical bottom buttons perform Prev/Next. Right-side buttons map to the current screen's labelled actions; verify physical GPIO positions later. Draw the four circular physical controls only in the desktop case, never inside the shared LCD renderer.
- macOS keyboard/mouse simulates physical inputs and touch; joystick provides contextual navigation.
- MUTED is a persistent mode change; ACK is confirmation of existing alarms only. Display the current mode and manual/schedule source clearly.
- History has no Clear action. Only the configured automatic retention policy removes old eligible records.
- On Standby the red button sleeps the LCD only; it does not disarm, mute, stop audio or disconnect NVR. Any physical button wakes it and consumes that first action. The blue button cycles ARMED → MUTED → DISARMED → ARMED. Sleeping state is independent of mode and acknowledgement.
- Standby is implemented on macOS with the shared renderer, live connection status, system time, click/keyboard input and sleep/mode controls. History and Settings currently open placeholder screens. Real detection/snapshot integration and the other full views remain planned. The older synthetic alarm workflow is available only with `--demo`.

## 8. Scheduling and clock

- One **global** weekly schedule shared by all cameras. Each period selects days, start/end time and one mode: `ARMED`, `MUTED` or `DISARMED`. Expose this under Settings → Harmonogram. The schedule automatically applies the selected mode when that period begins.
- Proposed simple schedule behavior: expose one enable/disable switch and weekly windows. With the schedule disabled, mode changes are manual. With it enabled, a manual mode lasts until the next actual scheduled mode transition. Merge adjacent/overlapping periods only when their modes match; reject overlaps assigning different modes. Define an explicit fallback mode for gaps before implementing the editor. Show the mode source and next transition in the UI. No selectable precedence policies in the initial version.
- Synchronize ESP32 time via NTP/SNTP (default `pool.ntp.org`, configurable). Use `Europe/Warsaw` as the default display timezone and handle DST correctly.
- Persist known timestamps in UTC and render local time in the UI. Use NVR `UTC` as a provisional clock source when internet/NTP is unavailable, recording the source and trust level. Verify freshness and field semantics in integration; the supplied sample has advancing `UTC` while `RealUTC` remains constant between Start and Stop. Do not assume those fields are interchangeable.
- Use a monotonic clock for audio deadlines, deduplication and backoff; wall-clock synchronization must not extend audio playback. Preserve unknown timestamps as unknown, with session/monotonic context, rather than inventing UTC.
- Proposed startup default before any usable wall clock: monitor in `ARMED`, visibly mark the time unavailable and defer schedule evaluation. Make the fallback mode configurable. Apply the current scheduled state when usable time becomes available, without replaying missed transitions or sounding historical alarms. Validate timezone/DST and clock-correction behavior during the scheduling stage.
- macOS uses the system clock through the same clock interface; tests use an injected deterministic fake clock.

## 9. Configuration and storage

- Dahua event and snapshot CGI paths are protocol constants in the communication module, not user configuration.
- Discover camera indices and names from NVR ChannelTitle at runtime; do not duplicate them in connection configuration. Convert zero-based API channel indices to one-based camera indices; snapshots use the one-based index and events use `index - 1`. Future per-camera zone/filter settings reference the discovered index.
- Human-readable, **pretty-printed JSON** (`config.json`) on SD for ESP32 and in a local directory on macOS.
- During development, keep NVR credentials in gitignored `config.local.json` (template: `config.example.json`). HTTPS verification is enabled by default; explicit `tls_insecure: true` disables certificate and hostname verification like `curl -k`, as requested. Production configuration remains versioned JSON.
- Configuration covers Wi-Fi SSID/credentials, NVR address/port/auth, per-camera selected grid cells, event/object filters, NTP/timezone, schedule, audio/retention limits, and optional snapshot settings.
- Version the configuration schema; validate on startup and display actionable errors. Do not silently overwrite invalid configuration.
- Save changes atomically and avoid unnecessary SD writes. Do not expose secrets in UI diagnostics or logs.
- Suggested directories: `config.json`, `history/`, `snapshots/`, `logs/`.
- **Security consideration:** credentials on removable SD are sensitive. Document this risk and consider optional encryption or credential provisioning later.

## 10. Rust workspace architecture

Suggested layout (start on macOS; refine embedded adapters during the later ESP32 port):

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
- Test `ARMED`, `MUTED`, and `DISARMED` separately, including a second camera alarming after the first is muted.
- Storage fault tests: full SD, missing card, interrupted write, corrupt config/history, rotation while active alarms exist.
- Hardware smoke tests: display, touch calibration, all four physical buttons, joystick, I2S, SD, Wi-Fi, NTP, and real NVR.
- CI runs formatting, linting, core tests, parser/integration tests, and macOS simulator/E2E where runners permit; ESP32 cross-build and hardware checks separately.

## 12. Milestones

Work proceeds on macOS first; ESP32 discovery and porting do not block the desktop app. The user requested communication tests and connection to the real configured NVR before further simulator/UI work. The initial synthetic UI is a prototype, not the main integration path. Each subsequent stage requires explicit acceptance before implementation. Report results and validation at each stage boundary.

| Stage | Scope | Completion / acceptance evidence |
| --- | --- | --- |
| 1. Dahua communication tests and live probe (in progress) | Start from the supplied event sample; cover multipart framing, fragmented/chunked transport, heartbeat, Start/Stop, Digest/Basic auth, HTTPS including explicit tls_insecure, failures/timeouts and snapshot retrieval. Provide private local configuration and an explicit live probe. | Automated protocol and local HTTP/HTTPS tests pass. After the user fills config.local.json, verify events and snapshots against the actual NVR; document coordinate/classification and channel-mapping findings. A local mock tests transport behavior; it does not substitute for live validation. |
| 2. Connect the macOS app to NVR | Use the tested communication layer in a background worker, show connection status and real camera snapshots, and normalize/deduplicate actual detection events. Preserve shared core and renderer. | Configured NVR events drive the UI without blocking it. Missing geometry/classification is shown as unresolved; never invent detections from motion-only metadata. Six camera mappings are verified. |
| 3. Desktop preview and zones | Mouse/touch and settings navigation; per-camera 32 × 20 zone editor with magnification and coordinate transforms using the verified geometry source. | Real camera previews and editor work at physical display resolution; tests cover geometry and input transforms. |
| 4. Desktop history and configuration | Pretty JSON configuration, atomic writes, snapshot persistence, history browser and startup recovery. | Alarm metadata survives restart; snapshot failures preserve records; credentials stay out of diagnostics. |
| 5. Schedule and resilience on macOS | Weekly schedule/settings, time-source handling, storage budgets, 90% cleanup threshold, bounded queues and fault behavior. Confirm proposed defaults before coding. | Tests cover manual overrides, DST, time correction, full storage, protected records, overflow and six-camera load. |
| 6. ESP32-S3 discovery and port | Inspect crd-esp32 wiring, select Rust stack and drivers, connect shared application/UI to display, touch, buttons, joystick, SD, Wi-Fi and I2S; benchmark JPEG/PSRAM. | Cross-build and hardware smoke tests pass; user checks pager loudness and input responsiveness. Adapt shared interfaces where hardware evidence requires it. |
| 7. Hardening and delivery | Long-running tests on both platforms, network/power/storage recovery, CI, setup instructions and diagnostics. | Unit/integration/macOS E2E checks and ESP32 cross-build pass; report hardware evidence, resource usage and remaining limitations. |

## 13. Open questions / validation gates

- **Native audio and reconnect implemented:** macOS audio is generated in-process through rodio/CPAL/CoreAudio. The communication library exposes a background subscription and a monitor command with bounded exponential retry (1–30 seconds), idle-read timeout, cancellation and visible mailbox overflow. Tests cover service errors, dropped and blackholed connections, recovery, healthy long-lived streams and stopping retries. The GUI remains a synthetic prototype pending integration of real detection/zone processing.


- **Channel titles verified (2026-10-28):** the read-only ChannelTitle API returned eight channel slots. All six configured camera names match the NVR ignoring ASCII case. Names were compared locally without printing them; camera names are now discovered rather than persisted in the connection config. Extra slots are not treated as proof of connected cameras.
- **Live communication verified (2026-10-28):** explicit HTTPS/Digest E2E tests passed using the private local configuration with `tls_insecure`. All six configured snapshot channels returned JPEG-marked responses (not saved); the stream delivered three heartbeats and two parsed events. No human rectangles arrived during that observation, so live geometry and visual channel mapping remain unverified. The complete GUI workflow is not covered by this communication check. Real addresses, credentials, names and raw responses are intentionally omitted.
- **Dahua metadata:** does this exact NVR expose usable object coordinates via `eventManager.cgi`, or is an additional endpoint/stream required?
- **Audio:** behavior in section 3 is accepted; validate actual pattern and volume on the speaker during hardware work.
- **Proposed defaults:** confirm the simple schedule rule, startup fallback, RAM queue limit, macOS storage budget and cleanup target before their implementation stages. SD cleanup starts at the agreed 90% threshold.
- **Alarm episodes:** how should repeated Start messages, missing Stop messages, and new detections from the same camera be grouped?
- **Hardware:** verify exact physical GPIO mapping for bottom/right buttons and Rust peripheral-driver compatibility.
- **Snapshots:** verify supported Dahua snapshot CGI path, image size, authentication, latency, and whether a snapshot can be retrieved reliably at alarm start.
- **UI:** validate touch usability for 32 × 20 grid; add magnification if needed.

## 14. Acceptance criteria

- A single Rust core and shared `embedded-graphics` UI run on macOS simulator and ESP32-S3 at 480 × 320.
- All six cameras can be configured; their events are processed in arrival order.
- A detected person/vehicle in enabled cells generates an alarm and history record when monitoring and storage are available, with one snapshot request at detection start. Snapshot failure is recorded without suppressing the alarm; storage failure is visible and does not suppress sound/UI alarms.
- Independent camera alarms remain browsable while only one is shown at a time.
- `MUTED` stops sound globally without stopping event recording; `DISARMED` stops new event recording.
- Alarm entries remain visible through ACK until Back. ACK confirms all alarms received so far, stops their sound and retains history; no manual Clear action is exposed.
- Repeating pager audio stops within 5 minutes of the global audio episode start; concurrent detections never extend that deadline. ACK stops audio for all alarms received so far without changing mode, MUTED suppresses future audio, and a distinct new alarm after timeout can sound again in ARMED mode.
- Weekly ARMED/MUTED/DISARMED schedule, NTP clock, pretty JSON configuration, SD storage, and automatic history rotation work.
- Automated unit, integration, and mock-NVR E2E tests cover normal and failure paths.
