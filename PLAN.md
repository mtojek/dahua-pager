# Implementation plan

Build the Dahua pager on macOS first, then port its shared Rust UI and alarm
logic to the ESP32-S3 handheld. The LCD is 480 × 320; hardware details come
from the [crd-esp32 project](https://github.com/mtojek/crd-esp32).

## Implemented

- Dahua event subscription with reconnects, live connection status and native
  macOS audio.
- Camera discovery from the NVR when entering Area Settings. Per-camera 32 × 20
  masks are saved in the local JSON config.
- `SmartMotionHuman` Start filtering by intersection with the saved camera
  mask. Motion events without a human rectangle do not create alarms.
- Alarm view with an NVR snapshot and detection rectangle. Snapshot capture is
  asynchronous and is not guaranteed to show the exact detection frame.
- ARMED, MUTED and DISARMED modes. ACK confirms alarms received so far; a new
  alarm wakes the display.
- Local event history with JPEG snapshots, browsing controls and a four-event
  text list on Standby. History keeps up to 1,000 records.
- macOS device preview with the 480 × 320 LCD and physical controls.
- The Mac device preview shows two status LEDs between the red button and LCD:
  red is on while ARMED; green blinks while local file reads and writes. The
  ESP32 port will connect these indicators to the actual LEDs and SD activity.

Run the Mac app with `cargo run -p pager-macos`. It reads private
`config.local.json`; see [README.md](README.md).

## Remaining work

1. **Weekly schedule:** add Settings controls for ARMED, MUTED and DISARMED
   periods, with local-time display and DST-safe transitions. See
   [docs/ui/schedule.md](docs/ui/schedule.md).
2. **Storage limits:** implement the 90% occupancy threshold against actual SD
   capacity on the device. The Mac development build already has a configurable
   artificial budget and prunes oldest complete records and snapshots first.
3. **ESP32-S3 port:** connect the shared renderer and alarm logic to the actual
   display, buttons, status LEDs, Wi-Fi, SD card and audio hardware. Measure
   memory and JPEG decode time on the target device.
4. **End-to-end validation:** check reconnects, alarms, snapshots, audio and
   storage failure behavior on the Mac and then on the handheld.

## Accepted behavior

- Only accepted human detections inside a camera's saved mask create alarms.
- ARMED sounds new alarms; MUTED records them silently; DISARMED ignores new
  detections. The repeating alarm lasts at most five minutes per episode.
- ACK confirms all alarms received before the action and stops current sound.
  It does not change the mode or clear history.
- Alarm and History snapshots come from the NVR after detection; they may show
  a later frame. The original NVR image caption is preserved.
- The red status LED stays on in ARMED and is off in MUTED and DISARMED. The
  green status LED blinks during file reads and writes.
- The Mac history store uses up to 90% of its configurable development budget,
  pruning oldest complete records and snapshots first. If no complete record
  can be pruned, new snapshots are rejected; event metadata is kept when it fits.
- Keep local NVR credentials, camera names and captures out of Git and logs.
