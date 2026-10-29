# Schedule

The schedule editor is planned under Settings. One weekly schedule will select
ARMED, MUTED or DISARMED for all cameras.

- Each period has days, a start/end time and a mode.
- ARMED records and sounds new alarms; MUTED records without sound; DISARMED
  ignores new detections.
- Display local time using Europe/Warsaw by default and handle daylight-saving
  changes. Store event timestamps in UTC.
- Save validates periods and writes the config. Proposed rule: a manual mode
  change lasts until the next scheduled transition. Define gaps and overlaps
  before implementation.
- Show whether the current mode is manual or scheduled and the next transition.

The editor and schedule evaluator are not implemented yet.
