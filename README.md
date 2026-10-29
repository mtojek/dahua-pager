# Dahua Pager

Alarm pager for Dahua NVRs, written in Rust. macOS first, ESP32-S3 later.

The NVR client supports Digest authentication, camera discovery, snapshots and
reconnecting event streams. The desktop UI has a standby screen with live NVR
connection status. macOS route changes cancel stale connections; 15 seconds
without valid messages also triggers reconnect. Real detection/snapshot handling
in the UI comes next.

## Run

```sh
cargo run -p pager-macos
```

Click the physical buttons, or use **Tab/arrows + Enter**. Red/**S** sleeps the
LCD; blue/**M** cycles ARMED → MUTED → DISARMED. Any button/key wakes the LCD
without also acting. **H** opens History, **T** opens Settings, **Backspace**
returns, **Esc** exits when awake. History and Settings are placeholders for the next views.

The earlier synthetic alarm demo is available with `cargo run -p pager-macos -- --demo`.
In the demo: **1–6** trigger alarms, **←/→** browse, **A** acknowledges all,
**S** sets MUTED, **C** dismisses an acknowledged alarm, **M** changes mode.

## Connect to an NVR

Copy `config.example.json` to `config.local.json` and fill in the address and
credentials. Set `tls_insecure: true` for self-signed certificates (`curl -k`).
Camera numbers and names come from the NVR.

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
