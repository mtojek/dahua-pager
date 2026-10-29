# UI

Design for the 480 × 320 ILI9488 display. The macOS simulator uses scale 1:1;
the additional case area holds physical controls outside the LCD. Mouse input
stands in for touch. These documents describe the target UI.
Standby and the macOS Area Settings camera list/editor are implemented. History
is still a placeholder; alarm handling and scheduling remain planned.

**Do not recreate `screens.md`.** Keep the screen backlog in this file.

## Screens still to implement

- **History:** browse saved detections with their snapshots and marked rectangles; Prev/Next changes the selected record, Back returns.
- **Alarm:** show the active alarm snapshot and marked rectangle; ACK confirms all alarms received up to that moment, then Back returns.
- **Schedule editor:** configure weekly ARMED / MUTED / DISARMED periods. See [schedule.md](schedule.md) for the current proposal.
- **ESP32 port:** bring the working macOS Area Settings camera list and mask editor to the device UI, including physical-button navigation and the LCD legend.

Standby, Settings menu, and the macOS Area Settings flow are already implemented;
do not list them as new screens to create.

- [Implementation plan](../../PLAN.md): stages, hardware and integration work.

## Modes and acknowledgement

| Mode | Create alarms and history | Sound |
| --- | --- | --- |
| ARMED | Yes | Yes |
| MUTED | Yes | No |
| DISARMED | No new detections | No |

`ALARM` is a screen, not a fourth mode.

**ACK confirms every alarm received before that action**, across all cameras,
and stops their audio. It leaves the mode unchanged and keeps history. A new
detection after ACK can ring again in ARMED. In MUTED, it stays silent.

ACK does not navigate away: the snapshot stays visible and the action becomes
Back. A later new alarm requires ACK again. An NVR Stop ends detection tracking;
it neither acknowledges an alarm nor stops its sound.

MUTED suppresses sound for future alarms too. The blue Mode button cycles
ARMED → MUTED → DISARMED → ARMED. The red Sleep button only blanks the LCD;
monitoring and audio continue. Any physical button wakes it without also
performing its normal action.
Returning to ARMED never replays old alarms. Audio stops after at most five
minutes per sound episode; concurrent alarms do not extend that deadline.

## Navigation

```text
Standby ── History ── Back
        └─ Settings ── Area Settings ── Camera ── Area editor
                    └─ Schedule

New alarm ── Alarm ── ACK all received ── Back ── Standby
```

The first alarm opens the alarm screen. Additional alarms queue in arrival order
without moving the user to another camera. Prev/Next browses the queue without
acknowledging anything. After ACK, records remain available in History.

## Layout and input

- Header: mode or ALARM, context/connection status, time.
- LCD text uses the Latin-2 bitmap font family, including Polish glyphs. Mode
  text uses the real bold font; context titles and NVR status share a small
  uppercase font and alignment. Colors are subdued red/amber/green on near-black.
- The bold mode text is white, with no badge or border. The entire header is
  red for ARMED, amber for MUTED and green for DISARMED.
- Physical right-side controls: red above blue, outside the LCD.
- Physical bottom controls: green then yellow, side by side below the LCD near its right edge. Metadata remains on the LCD where needed.
- Preserve snapshot aspect ratio. Never crop the area editor's camera frame.
- Physical bottom buttons map to Prev/Next. Right buttons perform contextual
  actions; exact GPIO mapping remains hardware work.
- macOS draws the case and four circular physical buttons outside the 480 × 320 LCD. Their captions are outside too. ESP32 uses the real buttons; it only renders the LCD content.
- Show reconnecting status without blocking navigation. Missing images use a
  placeholder; a retained old image must show its age.

The desktop case is 624 × 432 logical pixels with the 480 × 320 LCD inset at
(16, 16). Mouse coordinates hit physical circles, not rectangles on the LCD.
The header color follows the monitoring mode, independently of connection
state. Connecting/retry/error changes only the NVR status text, never the bar color. Settings drafts must survive an alarm interrupting their screen.
