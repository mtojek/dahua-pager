# Schedule

The Schedule screen has two daily transitions: **Auto-arm** and
**Auto-disarm**. Each can be turned on or off and has a 24-hour local time.
Both are off by default.

- Left/Right selects ON/OFF, hour or minute. Up/Down changes the selected
  value; hour and minute wrap around.
- Blue **Save** writes the schedule to `config.local.json`. Red **Back**
  discards edits.
- When both transitions are on, Auto-arm sets ARMED and Auto-disarm sets
  DISARMED. The armed interval can cross midnight. Their times must differ.
- A single enabled transition applies its mode at that time. A manual mode
  change lasts until the next enabled transition.
- On startup, when both transitions are enabled, the current mode is inferred
  from the time interval. The schedule uses the Mac's local timezone.

The schedule is evaluated by the Mac app. The ESP32 port will reuse these
settings and apply them using its local clock.
