# ESP32-S3 Pager Port Plan

## Goal

Run the Dahua pager on the LOLIN S3 with behavior matching the Mac app: the
same 480 × 320 interface, ARMED/MUTED/DISARMED modes, camera-area alarms,
snapshot history, schedule, alarm sound, and automatic NVR reconnection.

The original hardware project is in [`~/code/crd-esp32`](../../crd-esp32/README.md)
and is the source of truth for the board, components, pinout, and working
peripheral setup. Check it before choosing drivers or changing hardware details
in this plan. Connection testing will use the real NVR, not simulated cameras.

## Architecture

- Create a separate ESP-IDF Rust `std` application in `apps/pager-esp32`,
  outside the main Cargo workspace so the Xtensa toolchain does not affect the
  Mac app build.
- Prefer ESP-IDF Rust with `std`. `pager-core`, `pager-ui`, the configuration
  format, and the Dahua protocol parser currently use standard-library types,
  so this approach should allow us to reuse existing code.
- Use ESP-IDF for HTTP/TLS transport on the ESP32. Keep `reqwest` for the Mac
  app; share the event parser and alarm model.
- Provide detailed, timestamped console logs from boot through normal operation
  so hardware bring-up and runtime behavior can be diagnosed from a serial
  console.
- Keep the existing shared configuration sections and their order. Put
  device-specific settings, including Wi-Fi credentials, under a top-level
  `esp32` section after `dev`.
- Keep credentials local and out of Git and logs.

ESP-IDF Rust supports the `xtensa-esp32s3-espidf` target and `std` applications:
[esp-idf-template](https://github.com/esp-rs/esp-idf-template). The `mipidsi`
library includes an ILI9488 model with RGB565:
[mipidsi models](https://docs.rs/mipidsi/latest/mipidsi/models/index.html).

## Hardware reference

The pinout below is copied from `arduino/lolin_s3_ili9488.ino` in `crd-esp32`.
If this table conflicts with the source project, the source project wins.
The hardware baseline was checked against `crd-esp32` revision `0c1d2c2`.

| Component | GPIO |
| --- | --- |
| Red / green LEDs | 1 / 2 |
| Joystick: button / X / Y | 4 / 5 / 6 |
| Four action buttons | 7 / 15 / 16 / 17 |
| LCD: CS / DC / reset / backlight | 3 / 10 / 9 / 14 |
| LCD SPI: SCK / MOSI | 12 / 11 |
| XPT2046 touch: CS / IRQ / MISO | 8 / 18 / 13 |
| microSD SPI: SCK / MISO / MOSI / CS | 39 / 40 / 41 / 42 |
| MAX98357A: WS / BCLK / DIN / enable | 45 / 48 / 47 / 21 |

The LCD and touch controller share an SPI bus; the SD card uses a separate
bus. Before implementing image display, verify the exact module and memory
variant. ESP32-S3 internal memory is limited, and PSRAM availability depends on
the module:
[ESP-IDF external RAM guide](https://docs.espressif.com/projects/esp-idf/en/latest/esp32s3/api-guides/external-ram.html).

### Verified hardware details

- The source identifies the board as LOLIN S3 / ESP32-S3, but does not record
  the exact board revision, flash size, or PSRAM size. Verify these from the
  physical board before finalizing image-memory requirements.
- The ILI9488 is 480 × 320, landscape with BGR ordering, and uses RGB666
  (18-bit) over FSPI at 78 MHz. The pager renderer uses RGB565 pixels, so the
  display backend must convert them correctly for the panel's RGB666 interface.
- XPT2046 shares SCK/MOSI with the display, has its own MISO/CS/IRQ, and is read
  at 1 MHz. The reference mapping inverts raw Y into screen X and maps raw X to
  screen Y; its 200–3900 calibration is marked approximate.
- The SD card is on a separate HSPI bus at 4 MHz.
- MAX98357A uses I2S controller 0 at 22,050 Hz, 16-bit stereo, with eight DMA
  buffers of 256 samples in the Arduino test. Treat buffer settings as a
  starting point and measure them under the Rust firmware.
- All five push buttons use `INPUT_PULLUP` and are active-low. Joystick X/Y are
  analog inputs read at 12-bit resolution.
- The existing Arduino sketch already provides a working sequential hardware
  diagnostic. Use it as the reference when wiring or peripheral bring-up is in
  doubt; do not create another diagnostic application by default.

## Implementation tasks

Tasks are ordered by dependency. Each success criterion should be checked
before moving on. Hardware bring-up uses the pager firmware with temporary
diagnostics when useful; a separate diagnostic app is not planned unless a
specific issue requires one.

### 1. Project and build foundation

#### Task 1.1 — Confirm the hardware baseline

- Inspect `crd-esp32` for exact board/module variant, wiring, peripheral
  initialization, display orientation, and known working settings.
- Update this plan's pin and component notes if the source project differs.

**Success:** the hardware facts used by the port are verified against
`crd-esp32`, with unresolved differences recorded before driver work begins.

#### Task 1.2 — Establish the ESP-IDF Rust build

- Select and document compatible ESP-IDF, Rust, and tool versions.
- Create `apps/pager-esp32` targeting `xtensa-esp32s3-espidf`, outside the main
  Cargo workspace.
- Add documented build and flash commands.

**Success:** a minimal firmware image builds for ESP32-S3, and the existing Mac
app still builds independently.

#### Task 1.3 — Check shared crate compatibility

- Build `pager-core`, `pager-ui`, configuration types, and Dahua parsing code
  for the ESP-IDF target.
- Identify platform-only dependencies and isolate them behind interfaces.

**Success:** shared logic compiles for both Mac and ESP32 targets without
changing the Mac transport.

#### Task 1.4 — Establish console logging

- Make timestamped, module-tagged logs available from boot on the serial
  console, with clear levels for routine progress, warnings, and errors.
- Log startup, configuration loading (without values), component initialization
  and failures, UI screen/input changes, mode and LED/audio transitions,
  schedule decisions, and storage operations with used/free capacity before
  and after writes or pruning.
- Log NVR connection attempts, state changes, reconnect delay, heartbeat
  activity, event code/action/channel/index and timestamp, event acceptance or
  rejection reason, alarm lifecycle, and snapshot request/result.
- Keep logs understandable during hardware bring-up while preventing noisy
  data from obscuring errors; make verbose protocol diagnostics selectable in
  the `dev` configuration.
- Never log passwords, Wi-Fi credentials, NVR addresses, camera names, or raw
  event payloads.

**Success:** a captured serial log explains the path from boot through an
alarm and snapshot save, or identifies the component/error where progress
stopped, without exposing secrets or private camera details.

### 2. Incremental hardware bring-up

Verify each peripheral individually before integrating its pager behavior.
Use temporary diagnostics or serial output as needed, then remove one-off test
code after integration. The existing Arduino diagnostic in `crd-esp32` is the
known-good comparison when a peripheral does not behave as expected; a second
diagnostic app is not planned by default.

#### Task 2.1 — LEDs

- Verify polarity and control of both status LEDs using the reference project.

**Success:** firmware can turn each LED on and off reliably.

#### Task 2.2 — Buttons and joystick

- Verify all four action buttons, joystick button, and analog X/Y ranges.
- Determine active levels, direction mapping, and debounce behavior from the
  hardware reference and device.

**Success:** each input is distinguishable and stable during repeated presses
and joystick movement.

#### Task 2.3 — LCD and touch controller

- Initialize ILI9488 at 480 × 320 with the verified orientation and color mode.
- Verify touch controller communication if touch is used by the device design.

**Success:** test patterns render without corruption and any required touch
input reports stable coordinates.

#### Task 2.4 — microSD

- Mount the card and verify create, write, read, and delete operations.

**Success:** a test file survives unmount/remount and can be removed cleanly.

#### Task 2.5 — Audio amplifier

- Initialize MAX98357A over I2S using the known wiring and 22,050 Hz test rate.

**Success:** a short test tone plays clearly without resetting or blocking
input handling.

#### Task 2.6 — Wi-Fi hardware

- Join a configured Wi-Fi network and verify link loss and recovery.

**Success:** firmware detects connection state changes and can reconnect after
Wi-Fi returns.

### 3. Display and control integration

#### Task 3.1 — Render the shared LCD UI

- Connect `pager-ui` to the ILI9488 drawing backend using RGB565.
- Match the Mac preview's 480 × 320 layout, headers, legends, and selection
  treatment.

**Success:** all current screens render at the device's native resolution with
no clipping or layout changes caused by the display backend.

#### Task 3.2 — Map physical controls

- Map the four action buttons to the current screen's actions.
- Map joystick directions to navigation and Area Settings grid editing.
- Keep button actions separate from joystick navigation.

**Success:** screens and Area Settings can be operated without a computer
keyboard, with the same behavior as the Mac preview.

#### Task 3.3 — Connect status LEDs

- Keep the red LED on while ARMED.
- Use the green LED during local file reads and writes.

**Success:** LED behavior follows the pager state and storage activity.

### 4. Configuration and SD storage

#### Task 4.1 — Read and write configuration

- Read the existing JSON format from SD and preserve top-level key order:
  `schema_version`, `nvr`, `schedule`, `cameras`, `dev`, `esp32`.
- Keep Wi-Fi credentials and device-specific options under `esp32`; do not add
  them to shared Mac configuration sections.
- Save updates safely so an interrupted write does not leave a partial config.

**Success:** existing config loads, ESP32 settings can be saved and reloaded,
and a failed write does not destroy the last valid config.

#### Task 4.2 — Persist history and snapshots

- Store event metadata and original JPEGs on SD using the existing history
  format where practical.
- Load history images only after entering History.

**Success:** saved history survives restart and is browsable on the device.

#### Task 4.3 — Enforce storage limit

- Measure actual filesystem usage and free space; keep history below 90% of SD
  capacity.
- Remove the oldest complete records and snapshots first when space is needed.
- If no complete record can be pruned, reject new snapshots while retaining
  event metadata if it fits.

**Success:** full-card tests preserve valid configuration and existing history,
and logs report storage changes without exposing sensitive data.

### 5. Alarm audio

#### Task 5.1 — Integrate the pager alarm pattern

- Play the Mac app's alarm pattern without allocating a large sample buffer in
  RAM.
- Connect playback to the shared `pager-core` alarm state.

**Success:** alarm audio is clear, does not block UI/input, and follows alarm
state transitions.

#### Task 5.2 — Implement acknowledgement behavior

- ACK stops audio for every alarm received up to that point.
- Retain the maximum alarm duration limit.

**Success:** ACK stops current sound without changing mode or clearing history;
a later alarm can sound again while ARMED.

### 6. Wi-Fi and NVR connection

#### Task 6.1 — Configure Wi-Fi securely

- Read network credentials from the `esp32` configuration section.
- Keep secrets out of Git and logs.

**Success:** the device connects using local configuration and logs contain no
credentials.

#### Task 6.2 — Implement Dahua HTTPS transport

- Use ESP-IDF for HTTPS and Digest authentication.
- Keep transport-specific code separate from shared event parsing.

**Success:** firmware can authenticate to the real NVR over HTTPS and receive a
valid API response without logging secrets.

#### Task 6.3 — Subscribe to events and reconnect

- Open the `eventManager` stream and handle multipart boundaries, heartbeats,
timeouts, disconnects, and reconnects with backoff.
- Update NVR online/offline status from stream activity.

**Success:** real NVR events and heartbeats are parsed, Wi-Fi loss changes the
status promptly, and the pager reconnects after network recovery.

#### Task 6.4 — Discover cameras and fetch snapshots

- Discover cameras only when entering Area Settings; save only defined cameras
  and their existing names.
- Fetch snapshots through the Dahua API and preserve the NVR caption.

**Success:** Area Settings refreshes the camera list on entry, skips undefined
channels, and snapshots retain the NVR caption.

### 7. Alarms, masks, and images

#### Task 7.1 — Share event and geometry logic

- Share `SmartMotionHuman` parsing, rectangle validation, and alarm logic.
- Accept a detection only when its human rectangle intersects the saved mask
  for the corresponding camera.

**Success:** host tests cover valid/malformed rectangles, camera matching, and
mask intersection; non-human motion does not create alarms.

#### Task 7.2 — Save and show alarm snapshots

- Save JPEGs to SD and decode them for the LCD without loading a full 4K frame
  into RAM.
- Show a download state while fetching an alarm snapshot and draw the detected
  rectangle over the image.

**Success:** the Alarm view shows the image, camera caption, and detection box;
image fetching does not block event reception or input handling.

#### Task 7.3 — Browse history

- Display saved event metadata and snapshots in History.
- Keep the Standby screen's four recent events text-only.

**Success:** history navigation displays the correct event and image after
restart, and Standby does not eagerly load history images.

### 8. Schedule and time

#### Task 8.1 — Apply schedule transitions

- Load Auto-arm and Auto-disarm settings and apply the daily transitions using
  the shared schedule rules.

**Success:** enabled transitions set the expected mode, including intervals
that cross midnight, and manual mode changes last until the next transition.

#### Task 8.2 — Establish reliable time

- Check the NVR's local time API and determine whether it can seed the device
  clock; evaluate NTP and an added RTC as needed.
- Do not apply the schedule using uncertain time after a cold boot; show that
  time is not synchronized.

**Success:** schedule behavior remains correct after restart and internet loss;
without trusted time, the device does not make an unexpected automatic mode
change.

### 9. End-to-end validation

#### Task 9.1 — Host tests and target CI

- Keep host tests for parsing, mask geometry, reconnect backoff, schedule, and
  storage policy.
- Add an ESP32-S3 build to CI without requiring device secrets.

**Success:** host tests pass and CI builds the firmware target from a clean
checkout.

#### Task 9.2 — Validate with the real NVR and hardware

- Check heartbeat, `SmartMotionHuman` Start/Stop, mask rejection, Wi-Fi loss,
  snapshot, ACK, alarm audio, and full SD card.
- Repeat key checks after a power cycle.

**Success:** the core flow works without the Mac app and recovers from restart,
network loss, and return of network access.

## Decisions before implementation

1. **ESP32 configuration:** keep Wi-Fi credentials and other hardware-specific
   settings in a top-level `esp32` section after `dev`. This keeps the shared
   Mac configuration unchanged. Provisioning through a portal and storing
   credentials in NVS can be considered separately if needed.
2. **Time without internet:** the original hardware setup has no separate RTC.
   First check the NVR's time API; if unavailable, decide whether to add an RTC
   or block automatic transitions until time is synchronized.
3. **PSRAM:** before designing JPEG decoding, identify the exact LOLIN S3/module
   variant and available memory.

## Project constraints

- Do not change the working Mac flow unless code sharing requires it.
- Do not add a camera simulator for communication tests.
- Keep detailed timestamped console logging available throughout startup and
  operation; log state transitions and failure reasons.
- Do not log the NVR address, camera names, passwords, or raw event payloads.
- Keep alarm, history, mask, and schedule behavior consistent between Mac and
  ESP32.
