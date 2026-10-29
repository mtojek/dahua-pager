# Dahua Pager

macOS pager for Dahua NVR alarms. Development starts on the Mac; an ESP32-S3
port is planned.

## Run

The app reads `config.local.json` from the project directory. Create it from
`config.example.json` and fill in the NVR address and credentials. Set
`tls_insecure` to `true` only when the NVR needs the equivalent of `curl -k`.

```sh
cargo run -p pager-macos
```

Keep `config.local.json` private; it is ignored by Git.

## What it does

- Connects to the NVR event stream and reconnects after network loss.
- Opens **Area Settings** to refresh the camera list and edit each camera's
  32 × 20 detection mask. Saving writes the masks to the local config.
- Accepts `SmartMotionHuman` Start events only when the detected rectangle
  intersects that camera's saved mask. Motion events without a person rectangle
  do not trigger alarms.
- Shows the NVR snapshot and detected rectangle on the Alarm screen. The still
  is requested after detection and may not be from the exact detection instant.
- Records accepted alarms and JPEG snapshots in local History. The main screen
  shows the four latest events; History lets you browse snapshots.

History metadata is stored in `config.local.history.json`; JPEG files are in
`config.local.history/`. The newest 1,000 records are retained. Both paths are
ignored by Git.

## Controls

On the desktop, click the physical buttons around the LCD or use Tab/arrows and
Enter. **H** opens History, **T** opens Settings, Backspace goes back, and Esc
closes the app. The red button sleeps the display; an alarm wakes it. The blue
button cycles ARMED, MUTED and DISARMED.

For the synthetic alarm flow, set `demo_mode` to `true` in the local config. It
fires once after 15 seconds, uses a selected area mask and fetches its snapshot
from the configured NVR. Live human events are suppressed while demo mode is on.
