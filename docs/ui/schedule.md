# Harmonogram

Open from Settings → Harmonogram. One weekly schedule controls all cameras.
Each period has days, a start time, an end time and a mode:

| Mode | Behavior |
| --- | --- |
| ARMED | Detect, record and sound new alarms |
| QUIET | Detect and record without sound |
| DISARMED | Ignore new detections |

```text
ARMED                  Harmonogram           22:15

Enabled: Yes

Mon–Fri  00:00–07:00   ARMED
Mon–Fri  07:00–09:00   QUIET
Mon–Fri  09:00–22:00   DISARMED
Mon–Fri  22:00–24:00   ARMED

Add period                         Save
                                   Back
```

These are example periods, not default hours. The complete editor must also
support selecting weekend days, changing and removing periods.

At a period's start, the pager applies its mode automatically. Switching to
QUIET or DISARMED stops current audio. Switching to ARMED allows future alarms
to sound without replaying existing ones. ACK does not edit the schedule.

Save validates and persists changes. Back leaves without saving the draft.
Disabled scheduling leaves mode control manual. Display local time using
Europe/Warsaw by default; store event timestamps in UTC.

## Proposed rules to settle before implementation

- A manual mode change lasts until the next scheduled change of mode.
- Merge adjoining/overlapping periods only if their modes match; reject
  conflicting overlaps instead of choosing an implicit priority.
- Define a fallback mode for uncovered hours and a rule for missing clock time.
- Support periods crossing midnight and test daylight-saving transitions.

The schedule editor and evaluator are not implemented yet.
