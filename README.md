# Dahua Pager

Alarm pager for Dahua NVRs, written in Rust. macOS first, ESP32-S3 later.

The NVR client supports Digest authentication, camera discovery, snapshots and
reconnecting event streams. The desktop UI currently uses simulated alarms.

## Run

```sh
cargo run -p pager-macos
```

Keys: **1–6** trigger alarms, **←/→** browse, **A** acknowledges all received
alarms, **S** sets QUIET, **C** dismisses an acknowledged alarm,
**M** changes mode, **Esc** exits. Alarms play through CoreAudio for up to five minutes.

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
