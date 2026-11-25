# Domain Model — Package 2: Attendance

Phase 1 artefact (SKILL.md §4). Revision 1 · 24 November 2026 · Status: **Approved at Gate 2 on 24 November 2026**, with DQ-08 and DQ-AT-01–07 accepted as proposed.
Inputs: stories US-AT-01–44 and rules BR-AT-01–23 approved at Gate 1 (`../product/`), BRD v0.14, TSD v0.8, DEC-01–51, Issue Resolutions v0.2 §3, §4, §6, §7. Package 1 model: `DOMAIN_MODEL.md` (unchanged). This models business concepts, not tables; tables come in Phase 3.

---

## 1. Modules and ownership

Still one API service (modular monolith), plus the separate device gateway for eSSL pushes (TSD §3, §9.1). Each module owns its data; others read it only through the module's interface or its events.

| Module | Owns (aggregates) | Reads from |
|---|---|---|
| **Schedules** | Shift, RosterEntry, RosterBatch, HolidayCalendar, OptionalHolidayPick, ShiftSwapRequest | People (job versions: default shift, allowed shifts) |
| **Punch Collection** | Device, GeoFence, RawPunch, PunchImport, ManualPunch, PunchAdjudication, FlaggedPunchReview | People (enrolment ID by date), Identity (bound phone) |
| **Attendance Rules** | PolicySet, PolicyAssignment, Waiver, DayResult, PeriodResult | Schedules, Punch Collection, People, Leave (Package 3, empty slot) |
| **Regularisation** | RegularisationRequest, DayOverride | Attendance Rules (day result), People (manager chain) |
| **Periods** | AttendancePeriod, LockUnit, Adjustment, PeriodSettings | Org Structure (payroll group), People (membership by date) |
| **Notifications** | NotificationSetting, InAppNotification | Every module's events |
| People (Package 1) | Gains on JobVersion: default shift, allowed shifts, policy-set override (the reserved Package 2 columns) | — |
| Identity & Access (Package 1) | RegisteredDevice gains "bound" (DEC-20) | — |

Package 1 placeholders now become real: **PolicySetResolver** (owned by Attendance Rules) and **PeriodLockQuery** (owned by Periods). People keeps calling them through the same interfaces (Package 1 §5, §9).

---

## 2. Aggregates

### Schedules

#### 2.1 Shift
- **Root:** Shift — id, organisation, code, name, branch (optional; empty = all branches), state (Active, Inactive).
- **ShiftVersion** (entity, effective-dated): type (fixed / flexible / flexible with core / split), start, end, night flag (required when end ≤ start, DEC-23), required hours per day, window before and after, flexible band, core start and end, split break (fixed or window, start, end), half-day point (middle / fixed time / break start for split, ATT-05), SH-06 attendance date rule.
- A change is a new version from a date; versions are never edited. A version can't start inside a locked period (BR-AT-18).

#### 2.2 RosterEntry
- **Root:** RosterEntry — organisation, employee, date, either a shift or "weekly off", source (screen / upload / copy / rotation / swap / shift change request), recorded by, recorded at, superseded by.
- One current entry per (employee, date). Replacing it supersedes the old one, which is kept (history and recalculation reasons).
- **RosterBatch** (separate aggregate): an upload, copy-forward or rotation run, with its row outcomes; same lifecycle as Package 1 bulk import (SM-7). Idempotent by file checksum + row, or by run key.

#### 2.3 HolidayCalendar
- **Root:** HolidayCalendar — id, organisation, year, name, linked branches (set).
- **Holiday** (entity): date, name, kind (national / festival / optional).
- A branch is linked to at most one calendar per year (SH-10). Holiday changes on locked dates are refused (BR-AT-18).

#### 2.4 OptionalHolidayPick
- **Root:** OptionalHolidayPick — employee, holiday, state (SM-AT-6), decided by, reason.
- Counting toward HD-01 uses approved and pending picks in the calendar year.

#### 2.5 ShiftSwapRequest
- **Root:** ShiftSwapRequest — kind (change / swap), requester, date, wanted shift, colleague (swap only), state (SM-AT-7), approver. Approval writes RosterEntries (source "swap") in the same transaction.

### Punch Collection

#### 2.6 Device
- **Root:** Device — id, organisation, branch, serial (unique per organisation), make, model, token hash, token rotated at, last contact at, state (Active, Inactive).
- The token is shown once; only its hash is kept (DEC-46). Online/offline is **derived** from last contact and PC-10; the change between them emits events (SM-AT-2).
- Its own aggregate (not part of Branch) because it is written on every contact and has its own lifecycle.

#### 2.7 GeoFence
- **Root:** GeoFence — id, organisation, branch, name, state (Draft, Active, Inactive).
- **GeoFenceVersion** (entity): centre latitude and longitude, radius, valid from, valid to. A punch is checked against the versions **in force at its punch time** (BR-AT-09), so offline punches synced later use the old fence.

#### 2.8 RawPunch — append-only
- organisation, id, source (device / app / web / import), employee (empty when unmapped), enrolment ID, device, import batch, direction (in / out / untagged), device or phone time, uptime-derived time (offline app), receipt time, **effective punch time** (worked out once at receipt by PunchTimeResolver, §5), branch where punched, latitude, longitude, accuracy, distance to nearest allowed fence, fence version used, selfie reference, client IP (web), idempotency key, flags (set of reasons).
- Never updated or deleted (BR-AT-01). Two exceptions, both additive: an unmapped punch gets its employee link when HR maps the ID (DEC-44; recorded as a mapping record, the punch row itself keeps its original fields), and the selfie file is deleted after 13 months (DEC-51) while the punch stays.
- Partitioned by month of effective punch time (DEC-48).

#### 2.9 PunchImport
- **Root:** PunchImport — file checksum, imported by, row outcomes; lifecycle SM-7. Rows become RawPunches with source "import" (§6 format).

#### 2.10 ManualPunch — append-only, voidable
- employee, time, direction, reason, actor, origin (device-failure entry / outage fill / regularisation / HR correction), batch id, voided by (a later record; never deleted).
- Counts like a punch in the day calculation but is always shown as manual (PUN-08).

#### 2.11 PunchAdjudication — append-only
- punch, kind (exclude / include again / change attendance date / change direction), new value, reason, actor, origin (HR / flagged-punch rejection / regularisation). The latest adjudication of a punch wins. Raw punch untouched (CON-06).

#### 2.12 FlaggedPunchReview
- **Root:** FlaggedPunchReview — punch, flag reasons, state (SM-AT-4), decided by, reason. Created when a punch is stored with flags that need a decision (DEC-18). Rejection writes an exclude adjudication in the same transaction.

### Attendance Rules

#### 2.13 PolicySet
- **Root:** PolicySet — id, organisation, name, is-default, state.
- **PolicySetVersion** (entity): effective from, approval state (SM-AT-1), and the settings as value objects, one per group: PunchCollection (PC), ShiftOptions (SH-06–12), PunchReading (PR), Breaks (BK), Late (LA), Early (EC), EarlyArrival (EA), MissedPunch (MP), DayStatus (DS), Rounding (RD), UnapprovedAbsence (UA), Regularisation (RG), Holidays (HD), WeeklyOff (SH-09 pattern). Each value object validates itself (for example slabs start at grace + 1, BR-AT-14).
- Every organisation has one default set from creation (AC-AT-21.1). Versions are never edited once Active.

#### 2.14 PolicyAssignment
- level (organisation default / branch / department / category / employee type), target, policy set, effective range. The employee-level override lives on the employee's JobVersion (People). Resolution order per BR-AT-21.

#### 2.15 Waiver
- date, kind (late / early), branch or shift, reason, actor, state (Active, Withdrawn).

#### 2.16 DayResult — versioned, partitioned
- **Root:** DayResult — organisation, employee, attendance date; **current version** pointer; finalised at; lock state (read from Periods).
- **DayResultVersion** (entity, append-only): version number, reason, input fingerprint (versions of every input used), calculated at, and the values: expected schedule (shift version, weekly off, holiday, branch, time zone), sessions, gross / break / net hours (unrounded and rounded), late and early minutes, marks, slab actions, day status, absence handling state (for example Absent – pending until a date), flags, the branch each worked hour is costed to, and the **explanation** (BR-08).
- Partitioned by month of attendance date (DEC-48).

#### 2.17 PeriodResult — versioned
- employee, attendance period, counters (late marks, early marks, minutes), penalties with recovery (UA-04), excess absence, absconding suggestion, version, reason. Worked out at period end and recalculated as a shadow after lock (§3.17).

### Regularisation

#### 2.18 RegularisationRequest
- **Root:** RegularisationRequest — employee, date, type (missed punch / late / early / wrong punch / whole day), requested times or punch fix, reason, attachment, raised by (employee or HR on behalf, REL-09), level (1 or 2), state (SM-AT-5), decisions (actor, level, time, reason), created manual punches / adjudications.
- Approval writes its ManualPunches or adjudications in the same transaction; cancellation voids them (ATQ-03).

#### 2.19 DayOverride — append-only, voidable
- employee, date, values overridden (status and/or hours), reason, actor, voided by. Used by HR direct correction (RG-10).

### Periods

#### 2.20 AttendancePeriod
- payroll group, start, end, cut-off (PL-02). Generated ahead from the group's pay period (PL-01, Package 1). Never overlapping within a group.

#### 2.21 LockUnit
- **Root:** LockUnit — attendance period × branch (the group comes from the period), state (SM-AT-9), history of transitions with actor and reason.
- An employee's date is locked when the LockUnit for (their payroll group on that date, their master branch on that date, the period containing it) is not Open.

#### 2.22 Adjustment — append-only
- employee, target period (next open one), original date or original period, kind (correction / actual after cut-off / penalty reversal / late punch), days and minutes delta, reason, source (what changed), created at. Never edited; a later shadow difference adds another Adjustment.

#### 2.23 PeriodSettings
- per payroll group, versioned: PL-02 cut-off, PL-03, PL-04, PL-05, PL-06, PL-07. (PL-01 stays on PayrollGroup, Package 1.)

### Notifications

#### 2.24 NotificationSetting and InAppNotification
- NotificationSetting: event, channel (email / in-app), recipient role, on/off. InAppNotification: user, event, payload, created, read at. Email goes through the existing outbox and mailer.

---

## 3. Relationships

```
Branch 1──* Device, 1──* GeoFence 1──* GeoFenceVersion
Branch *──1 HolidayCalendar (per year) 1──* Holiday
Shift 1──* ShiftVersion;  Employee 1──* RosterEntry (one current per date)
Employee JobVersion ──> default Shift, allowed Shifts, PolicySet override
PolicySet 1──* PolicySetVersion;  PolicyAssignment *──1 PolicySet
RawPunch *──0..1 Employee, *──0..1 Device, *──0..1 PunchImport;  RawPunch 1──* PunchAdjudication, 0..1 FlaggedPunchReview
Employee 1──* ManualPunch, DayOverride, RegularisationRequest
Employee 1──* DayResult (one per date) 1──* DayResultVersion
PayrollGroup 1──* AttendancePeriod 1──* LockUnit (one per branch);  Employee 1──* Adjustment, PeriodResult
```

---

## 4. Invariants

| ID | Invariant | Rule | Where |
|---|---|---|---|
| INV-AT-01 | Raw punches, manual punches, adjudications, day overrides and adjustments are never updated or deleted (voiding and "include again" are new records) | BR-AT-01 | DB (no UPDATE/DELETE grant, trigger) |
| INV-AT-02 | One raw punch per idempotency key per organisation and source | BR-AT-02 | DB (unique) |
| INV-AT-03 | A device push is stored only if the token matches that device | BR-AT-03 | Gateway |
| INV-AT-04 | Device serial unique per organisation | PC-R2 | DB |
| INV-AT-05 | Shift versions, policy-set versions, geo-fence versions and policy assignments don't overlap for the same target | BR-AT-21, §2 | DB (exclusion) |
| INV-AT-06 | One current RosterEntry per employee and date | AC-AT-02.2 | DB (partial unique) |
| INV-AT-07 | One holiday calendar per branch per year | SH-10 | DB |
| INV-AT-08 | No schedule, calendar or policy change effective inside a locked period | BR-AT-18 | API (PeriodLockQuery) |
| INV-AT-09 | Exactly one current DayResultVersion per employee-date; a result is stored only if its input fingerprint is newer than the current one | BR-AT-10, DM-04 | DB (unique) + API (row lock) |
| INV-AT-10 | No day result before joining or after the last working day | BR-EM-22 | API |
| INV-AT-11 | Approver ≠ person who raised the request or made the punch; fallback approver is the Org Admin | BR-AT-08, 15 | API |
| INV-AT-12 | Attendance periods of a payroll group don't overlap and leave no gaps | PL-R1 | DB (exclusion) |
| INV-AT-13 | One LockUnit per period and branch; state moves only one step at a time | BR-AT-17, 19 | DB (unique) + API |
| INV-AT-14 | A locked period's results never change; changes for its dates become Adjustments in the next open period | BR-AT-18 | API |
| INV-AT-15 | Late and early slabs are contiguous from grace + 1 with no overlap | BR-AT-14 | API (value object) |
| INV-AT-16 | App punches only from the employee's bound phone | BR-AT-22 | API |
| INV-AT-17 | Optional holiday picks per year ≤ HD-01 | HD-R3 | API |
| INV-AT-18 | Every record carries an organisation taken from the token (device: from the device record); RLS on every table | TB-01 | DB (RLS) |

---

## 5. Domain services

| Service | Does | Notes |
|---|---|---|
| PunchTimeResolver | Works out the effective punch time: live device push with clock off by more than PC-08 → receipt time; late-synced batch → device time; offline app → uptime-derived time if the phone clock is off by more than PC-08 (DEC-21, §3.4). Sets clock-mismatch and late-sync flags. | Pure, unit-tested |
| FenceChecker | Distance to the nearest allowed fence (PC-16) using the versions in force at punch time; applies PC-04, PC-14, PC-12/13/15 and EM-07. | Pure |
| PunchIngest | One path for every source: dedupe by key, resolve employee by enrolment ID on the punch date (or leave unmapped), flag, store, open a review if needed, queue recalculation. | Gateway and API share it |
| ScheduleResolver | Expected schedule for an employee-date: roster entry → default shift (or auto-detect, ATT-04) → weekly off → holiday from the branch calendar on that date (DEC-26) → night flag and window; time zone of the master branch (DEC-50). | Pure given inputs |
| PolicySetResolver | The policy-set version in force for an employee on a date and its source (BR-AT-21). | Replaces the Package 1 stub |
| DayCalculator | The §7 steps as one pure function: inputs (schedule, punches after adjudication, manual punches, overrides, approved requests, waivers, overlays slot, policy version) → DayResultVersion values and explanation. | The core; worked examples E1–E19 and W1–W11 are its tests |
| RecalculationQueue | Turns change events into (employee, date range) tasks via the outbox; per employee-date lock; skip if the input fingerprint is unchanged; nightly sweep; TEC-03 rate limits. | |
| Finaliser | Hourly: finalises (branch, date) once branch-local time passes the latest window end + 1 hour (§3.26). | Job J-AT1 |
| PeriodEndCalculator | Counters, penalties (BR-AT-13), recovery order, excess absence, absconding suggestion (flag only, DEC-37). | Pure |
| ShadowRecalculator | For a change touching a locked date: recalculates the locked period in memory, compares with the stored result, posts the difference as Adjustments (§3.17). | |
| PeriodLockQuery | Is this employee-date locked? (BR-AT-17) | Replaces the Package 1 stub |
| ApproverRouter | Level 1 = reporting manager; level 2 = HR Manager in scope; never the raiser or puncher; fallback Org Admin (DEC-31). Escalation per RG-07. | |
| MissedPunchReminder | Schedules MP-04 reminders at shift end + X and next morning. | Job |
| DeviceHealthMonitor | Marks devices offline after PC-10 minutes and online again; one alert per spell. | Job |
| PayrollInputBuilder | Builds the locked period's export (US-AT-40). | Report |

---

## 6. Effective dating and time

- **Attendance date** is worked out in the master branch's time zone on that date (DEC-50); SH-06 decides it for night shifts. The punch also keeps the local time of where it was made, for display.
- Settings (shift versions, policy-set versions, fences, assignments, period settings) are effective-dated and never edited once in force; a change is a new version.
- Day results keep every version (§3.25); "as known on date X" is the latest version calculated before X.

---

## 7. Events

**Consumed from Package 1:** EmployeeCreated, JobChanged (also covers default shift, allowed shifts and policy override, now job fields), StatusChanged, EmployeeExited, EnrolmentChanged (relinks unmapped punches, DEC-44).

**Published:**

| Event | When | Consumers |
|---|---|---|
| PunchRecorded (employee, date) | A punch, manual punch or adjudication is stored | RecalculationQueue |
| PunchFlagged / PunchReviewed | Review opened / decided | Notifications (NT-08), RecalculationQueue |
| ScheduleChanged (employees, date range) | Roster, shift version, calendar, optional holiday, swap | RecalculationQueue, Notifications |
| PolicyChanged (scope, from date) | Policy version Active, assignment changed, waiver added or withdrawn | RecalculationQueue |
| DayResultChanged (employee, date, version) | New current version | Notifications (NT-02, NT-03), dashboards |
| RegularisationChanged | Any SM-AT-5 transition | Notifications (NT-05, NT-06), RecalculationQueue |
| PeriodStateChanged (lock unit, state) | SM-AT-9 transition | Notifications (NT-09), People (PeriodLockQuery cache, none in V1) |
| AdjustmentPosted | Shadow difference stored | Notifications (NT-10) |
| DeviceOffline / DeviceOnline | Health change | Notifications |

---

## 8. Tenancy, volume and audit

- Every aggregate carries organisation; RLS everywhere. The device gateway has no user: it finds the organisation from the device record matched by serial and token, then runs as that organisation.
- **Volume (DEC-13, DEC-48):** RawPunch and DayResultVersion are partitioned by month from day one; every query on them filters by organisation and date range. DayResultVersion grows about 1.5–2× DayResult (§3.25).
- Audited: every change to settings, shifts, rosters, calendars, devices (including token rotation), fences, assignments, waivers; every decision on reviews, requests and picks; manual punches, adjudications, overrides; lock transitions; exports. Raw punches and day result versions are their own history and are not copied into the audit log.

---

## 9. Cross-domain dependencies

| Needs | From | Until then |
|---|---|---|
| Leave, permission, on-duty, comp-off overlays; leave debits for UA and penalties | Leave (Package 3) | Empty slot: no overlays; debits go to loss of pay; open days recalculated when Package 3 arrives |
| Pay values (fixed-amount penalties, holiday pay for daily-wage) | Payroll (V2) | Carried as counts and settings in the payroll input |
| eSSL push format and rate | PUN-01 device test | Gateway adapter not built; everything behind PunchIngest is |
| Liveness check service | ATQ-13 | Selfies stored without a check |

---

## 10. Gate 2 questions

**Accepted as proposed at Gate 2, 24 November 2026.**

| ID | Question | Proposed |
|---|---|---|
| DQ-08 | Long leave and attendance (deferred from Package 1) | Days in Long leave status show "long leave": no absence, no late or early, no UA debits. Punches are still stored; a day with punches is calculated normally and flagged for HR. |
| DQ-AT-01 | Absconding status days | Calculated as normal (absent days go through UA, so loss of pay); absconding alerts stop (SM-1 note); no pay hold (DEC-37). |
| DQ-AT-02 | Who steps a period back | The Organisation Admin does it with a reason; doing it is the approval. HR Managers can't step back. |
| DQ-AT-03 | Where the default shift, allowed shifts and policy override live | On the employee's job version (People), using the columns Package 1 reserved; changed through the existing job-change route, so they are dated and history comes free. |
| DQ-AT-04 | Undoing a flagged-punch rejection | Decisions are final; HR fixes a wrong one with an "include again" adjudication and a reason. |
| DQ-AT-05 | How long day result versions are kept | As long as the organisation's data (DEC-33); no separate purge. |
| DQ-AT-06 | Lock unit for an employee who changes branch mid-period | Each date belongs to the lock unit of the branch on that date; HR sees the employee in both branches' checklists. |
| DQ-AT-07 | Unmapped punches older than the mapping | Linked only from the mapping's effective date; older unmapped punches stay unmapped unless HR back-dates the mapping. |
