# Dahua Pager

Alarm pager for Dahua NVRs, written in Rust. macOS first, ESP32-S3 later.

The NVR client supports Digest authentication, camera discovery, snapshots and
reconnecting event streams. The desktop UI shows live connection status and
opens an alarm view for accepted human-detection events. macOS route changes
cancel stale connections; 15 seconds without valid messages triggers reconnect.

## Run

```sh
cargo run -p pager-macos
```

Runtime diagnostics are printed to the terminal with timestamps. They include
NVR connection changes, event handling, alarm and snapshot activity, and UI
actions; credentials, host addresses and camera names are omitted.

Click the physical buttons, or use **Tab/arrows + Enter**. Red/**S** sleeps the
LCD; blue/**M** cycles ARMED → MUTED → DISARMED. Any button/key wakes the LCD
without also acting. **H** opens History, **T** opens Settings, **Backspace**
returns, **Esc** exits when awake.

## Connect to an NVR

Copy `config.example.json` to `config.local.json` and fill in the address and
credentials. Set `tls_insecure: true` for self-signed certificates (`curl -k`).
Camera IDs come from the NVR's unique `Name` values. Area Settings saves each ID
beside its mask in `config.local.json`; IP addresses and channel indexes stay
out of the config.

To exercise the alarm screen, set the top-level `demo_mode` field in
`config.local.json` to `true`. After 15 seconds the app chooses a random channel
and draws a random person rectangle over that channel's live NVR snapshot. The
demo event is synthetic; the image is fetched from the real NVR. Live human
events are suppressed while demo mode is on. Set it back to `false` for normal
operation.

```sh
# Watch events; reconnect automatically
cargo run -p pager-dahua --example probe -- config.local.json monitor

# Check snapshot retrieval
cargo run -p pager-dahua --example probe -- config.local.json snapshot
```

Keep real credentials and camera names out of Git. `config.local.json` and
`captures/` are ignored.

## Tests

```sh
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

CI runs these checks on macOS for pushes to `main`. Tests against a real NVR
require local configuration and must be run explicitly:

```sh
cargo test -p pager-dahua --test live_nvr -- --ignored --nocapture --test-threads=1
```

See [PLAN.md](PLAN.md) for the implementation plan and [docs/ui](docs/ui/README.md)
for screen layouts and navigation.
