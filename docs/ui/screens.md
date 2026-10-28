# Screens

See [shared rules](README.md) for mode, ACK and navigation behavior. Sketches show
placement, not exact pixel dimensions. Names below are synthetic examples.

## Standby

```text
ARMED                  <status>              22:15


                                  Disarm
              Empty               Quiet


History                           Settings
```

The main area stays empty while waiting. There is no default camera preview.
Disarm selects DISARMED and becomes Arm. Quiet selects QUIET. The header shows
the resulting mode; Arm restores ARMED. History and Settings open their screens.

## History

```text
ARMED                  History               22:15

┌────────────────────────────┐
│ Snapshot                   │       Back
│   ┌──────┐                 │
│   │person│                 │
│   └──────┘                 │
└────────────────────────────┘
<when>  <camera / detection>     2/15   Prev  Next
```

Show one stored event with its snapshot and detection rectangle. Prev/Next
browses records; Back returns to Standby. An empty history shows an empty state
and Back. A missing snapshot does not hide the record's metadata.

**There is no Clear or manual delete action.** Automatic storage retention is
responsible for removing old eligible records.

## Alarm

```text
ALARM                  <status>              22:15

┌────────────────────────────┐
│ Snapshot                   │       ACK
│   ┌──────┐                 │       ↓
│   │person│                 │       Back
│   └──────┘                 │
└────────────────────────────┘
<when>  <camera / detection>      1/3   Prev  Next
```

ACK is replaced by Back after all currently received alarms are acknowledged;
the two are successive states of the action. ACK always covers all cameras,
regardless of which record is visible. Back returns to Standby and does not
delete history. If a new alarm arrives after ACK, the action returns to ACK.

Prev/Next allows browsing concurrent alarms. Keep the current selection when
another camera alarms. The rectangle describes the detection payload; snapshot
capture may happen later, so the image is not guaranteed to show that exact pose.

## Settings

```text
ARMED                  Settings              22:15

Ustawienia obszaru
Harmonogram
Głośność / Test dźwięku
Diagnostyka

                                       Back
```

Ustawienia obszaru opens the camera list. Harmonogram opens the
[schedule editor](schedule.md). Audio settings provide volume and a sound test.
Diagnostics shows connection/time/storage status without credentials or raw NVR
responses. Back returns to Standby.

## Area: camera list

```text
ARMED              Ustawienia obszaru         22:15

1  Backyard 1
2  Backyard 2
3  Garage
4  Pool
…
                                       Back
```

Read camera numbers and names from the NVR. Selecting one opens its area editor.
Do not duplicate discovery data in connection configuration. Channel slots
reported by the NVR are not proof that every slot has a connected camera.
Discovery errors leave Back available and show a retry action.

## Area editor

```text
ARMED                  Backyard 1            22:15

┌────────────────────────────┐
│ Snapshot + 32 × 20 grid     │       Save
│ Selected cells highlighted │       Back
│                            │
└────────────────────────────┘
<zoom / selection controls>
```

Touch selects or deselects cells. Provide magnification for precise selection.
Keep the complete camera frame and exclude letterboxed margins from selection.
Save persists the selected cells for this camera atomically. Back leaves without
saving the draft. An incoming alarm preserves the draft for returning to it.

Snapshot refresh and zoom control placement will be settled with the rendered
editor. No live video is required.
