# UI

Design for the 480 × 320 pager display. The macOS window scales the same logical
screen; mouse input stands in for touch. These documents describe the target UI,
not the current prototype's implementation status.

- [Screens](screens.md): layout, actions and navigation.
- [Schedule](schedule.md): weekly ARMED / QUIET / DISARMED periods.
- [Implementation plan](../../PLAN.md): stages, hardware and integration work.

## Modes and acknowledgement

| Mode | Create alarms and history | Sound |
| --- | --- | --- |
| ARMED | Yes | Yes |
| QUIET | Yes | No |
| DISARMED | No new detections | No |

`ALARM` is a screen, not a fourth mode.

**ACK confirms every alarm received before that action**, across all cameras,
and stops their audio. It leaves the mode unchanged and keeps history. A new
detection after ACK can ring again in ARMED. In QUIET, it stays silent.

ACK does not navigate away: the snapshot stays visible and the action becomes
Back. A later new alarm requires ACK again. An NVR Stop ends detection tracking;
it neither acknowledges an alarm nor stops its sound.

Quiet changes the global mode and suppresses sound for future alarms too.
Returning to ARMED never replays old alarms. Audio stops after at most five
minutes per sound episode; concurrent alarms do not extend that deadline.

## Navigation

```text
Standby ── History ── Back
        └─ Settings ── Ustawienia obszaru ── Camera ── Area editor
                    └─ Harmonogram

New alarm ── Alarm ── ACK all received ── Back ── Standby
```

The first alarm opens the alarm screen. Additional alarms queue in arrival order
without moving the user to another camera. Prev/Next browses the queue without
acknowledging anything. After ACK, records remain available in History.

## Layout and input

- Header: mode or ALARM, context/connection status, time.
- Right column: actions for the current screen.
- Bottom row: metadata and navigation where needed.
- Preserve snapshot aspect ratio. Never crop the area editor's camera frame.
- Physical bottom buttons map to Prev/Next. Right buttons perform contextual
  actions; exact GPIO mapping remains hardware work.
- Labels in the sketches describe actions, not four extra buttons below the LCD.
- Show reconnecting status without blocking navigation. Missing images use a
  placeholder; a retained old image must show its age.

Exact dimensions, touch targets and focus behavior will be checked in rendered
layouts. Settings drafts must survive an alarm interrupting their screen.
