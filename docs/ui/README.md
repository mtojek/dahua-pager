# LCD views

The Mac app renders the same 480 × 320 LCD layout planned for the ILI9488 on
the ESP32-S3. Its case and physical controls are drawn outside the LCD. The
case shows two LEDs between the red button and LCD: red for ARMED, green
blinking during local file reads and writes.

## Standby

The header shows the mode, NVR status and time. The body lists the four latest
events as date, camera and event type. Red sleeps the display; blue cycles
ARMED → MUTED → DISARMED. An alarm wakes a sleeping display.

## Alarm

Shows the NVR snapshot with the detected rectangle. The image is requested
after detection, so it may not be the exact frame that caused the event. ACK
acknowledges all alarms received so far; the screen stays open until Back.

## History

Shows the saved NVR snapshot and a `HISTORY (n/total)` counter. Prev/Next
browse records; Back returns. Snapshots retain any caption supplied by the NVR.
The Standby list shows metadata for the latest four records without images.

## Settings and Area Settings

Settings currently opens Area Settings. The camera list refreshes when entering
that view. Selecting a camera opens its snapshot with a 32 × 20 grid; selected
cells are translucent red. Save writes all camera masks to the local config.

## Controls

Green and yellow buttons below the LCD are Prev/Next where available. Red and
blue buttons on the right perform the current view's actions, such as Back, ACK
or Save. Their captions are shown in the desktop case and LCD legend.

The Schedule screen sets daily Auto-arm and Auto-disarm times. See
[schedule.md](schedule.md) for the controls and transition rules. The ESP32
port is not implemented yet; see [PLAN.md](../../PLAN.md).
