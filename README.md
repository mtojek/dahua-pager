# Dahua Pager

Rust alarm pager, developed on macOS first and intended for a later ESP32-S3 port.

## Dahua communication — current work

The current priority is testing communication and connecting to the real NVR.
The existing GUI is a synthetic prototype; it is not wired to the NVR yet.

Copy `config.example.json` to **`config.local.json`** (already prepared in this
workspace). The local file is ignored by Git. Fill `nvr.base_url`, `username`
and `password` locally. Use HTTPS and set `tls_insecure` to `true` if you want
curl `-k` behavior: certificate and hostname verification are then disabled.
The default is `false`. Authentication defaults to Digest; Basic is explicit.

Privacy: real configuration, camera labels, device identifiers, addresses and raw
NVR responses stay local. Public examples and test fixtures use neutral names.
Do not commit `config.local.json`, private certificates, snapshots or captures;
`captures/` is ignored. Redact new fixtures before adding them to the repository.

Camera numbers and names are discovered from the NVR ChannelTitle API and are
not stored in connection configuration. The API's zero-based channel index is
converted to a one-based camera index; snapshots use that index directly.

Read up to ten events/heartbeats:

```sh
cargo run -p pager-dahua --example probe -- config.local.json
```

Check a snapshot from the first discovered camera (not saved):

```sh
cargo run -p pager-dahua --example probe -- config.local.json snapshot
```

The probe prints event type, action and channel, not credentials or raw metadata.
This mode stops after ten messages. Each blocking network operation has the
configured `session_timeout_secs` timeout (60 seconds by default); this is an
idle/read timeout, not a maximum lifetime for a healthy stream. Snapshot checks verify MIME type, JPEG markers and a size
limit; full image decoding is a later step.

Communication tests use the supplied redacted event fixture and local HTTP/HTTPS
servers. They do not require NVR credentials and never connect to the real NVR.

```sh
cargo test -p pager-dahua
```

Live communication E2E tests are ignored by default. With a private local config
and explicit access to the NVR, run:

```sh
cargo test -p pager-dahua --test live_nvr -- --ignored --nocapture --test-threads=1
```

The live tests check snapshot retrieval for the first six discovered channel slots and an event
stream with three heartbeats. If human events arrive during that observation,
their rectangles are validated; a quiet camera does not prove human detection.
Output contains only counters and snapshot byte sizes. Images and raw events
are not saved. This verifies the communication layer, not the complete GUI alarm
workflow. Set `DAHUA_CONFIG` to select a different private config path.

Camera names can be read through the fixed Dahua ChannelTitle API. To verify
discovery without printing private names:

```sh
cargo test -p pager-dahua --test live_nvr live_camera_names -- --ignored --nocapture
```

This checks that channel discovery succeeds and reports the number of slots.
NVR channel slots may outnumber connected cameras;
the API listing alone does not prove a camera is connected in every slot.

## Reconnecting event monitor

```sh
cargo run -p pager-dahua --example probe -- config.local.json monitor
```

The monitor uses a background subscription with a bounded 128-message mailbox.
A disconnect, read timeout, malformed stream or HTTP/auth error triggers retry
waits of 1, 2, 4, 8, 16 and then at most 30 seconds. Valid traffic resets the
backoff. A healthy stream stays connected while receiving messages; it is not
periodically restarted merely because the read timeout elapsed since connection.

Connection states and dropped-message counts are exposed to callers. The monitor
prints status changes and traffic counters without names or raw payloads. Stop it
with Ctrl-C. Library callers can stop a subscription without blocking their UI;
retry waits stop immediately, and an in-flight blocking read exits at its I/O
timeout. Default idle-read timeout is currently 60 seconds, configurable through
`session_timeout_secs`. Messages missed during an outage cannot be recovered from
this live stream. The GUI still uses synthetic alarms; connecting the subscription
to the full GUI detection/zone workflow remains a separate stage.

Check native audio device initialization without playing sound:

```sh
cargo run -p pager-macos -- --check-audio
```

## Run the macOS prototype

Install Rust, then run:

```sh
cargo run -p pager-macos
```

The app displays a 480 × 320 logical screen at 2× scale. It starts quietly in
ARMED mode. Pressing a camera key starts repeating audible beeps using macOS
native CoreAudio through rodio/CPAL, with no subprocess or temporary audio file;
use S to silence or Escape to exit. Volume currently follows the
Mac's output volume (an in-app volume setting comes in a later stage).

| Key | Action |
| --- | --- |
| 1–6 | Generate a distinct synthetic alarm for that camera |
| Left / Right | Previous / next alarm |
| S | Silence globally |
| C | Close the selected alarm |
| M | Cycle ARMED → SILENCED → DISARMED → ARMED |
| Escape | Exit |

Audio stops after five minutes; additional alarms do not extend the deadline.
Closing one alarm leaves audio playing if another alarm in the current sound
episode still requires it. Rearming does not replay old alarms.

This first prototype uses synthetic detections and an explicit image placeholder.
NVR connectivity, snapshots, mouse/touch, settings and persistent history are
subsequent stages. Closing an alarm currently removes it from memory; nothing is
saved across app restarts. At the provisional 128-entry queue limit, new detections
are counted as overflow but do not get individual entries or sound requests.

The workspace separates `pager-core` (alarm rules), `pager-ui` (shared
embedded-graphics renderer) and `pager-macos` (window, keyboard, audio and clock).

## Development

GitHub Actions runs formatting, Clippy and workspace tests on macOS after each
push to `main`, using Rust 1.92.0 and `Cargo.lock`. Live NVR tests remain ignored;
CI needs no private configuration or audio device.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

See [PLAN.md](PLAN.md) for stages and acceptance criteria.
