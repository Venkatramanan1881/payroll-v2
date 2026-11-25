# User Stories and Acceptance Criteria — Package 2: Attendance

Phase 0 artefact (SKILL.md §4). Revision 1 · 24 November 2026 · Status: **Approved at Gate 1 on 24 November 2026**, with proposed values ATQ-05–19 and the Issue Resolutions §3 items listed in OPEN_QUESTIONS accepted as proposed. [P] items below are now accepted.
Sources: BRD v0.13 (C04, C05, C06, C07, C12, §13.1, §14), TSD v0.7, `../../Payroll-App-Issue-Resolutions-v0.2.md` (DEC-01–51, §3, §4, §5, §6, §7), ATQ-01–04.

Acceptance criteria (AC) are written Given / When / Then. Rules BR-AT-nn are in `../BUSINESS_RULES.md`; BR-01–15 are the fixed rules in BRD §12.
Tags: **[C]** confirmed (BRD or a decision) · **[P]** proposed, needs approval at Gate 1 · **[B]** blocked (see `../../requirements/OPEN_QUESTIONS.md`).

**Where the BRD and the decisions differ, the decision wins.** BRD v0.13 has not yet absorbed DEC-14, 17, 18, 21–25, 41, 46–51 (for example PR-01 still says default "paired"). The stories below follow the decisions; the BRD change list is in OPEN_QUESTIONS ATQ-20.

## Actors

| Actor | In this package | Source |
|---|---|---|
| HR Manager | Shifts, rosters, calendars, devices, fences, policy sets; flagged punches and regularisation in scope; manual attendance; corrections; waivers; period lock in V1 | §5, PC-R8, PC-R9, ATQ-05 |
| Organisation Admin | Everything the HR Manager can; approves going back a period step; approves settings when maker-checker is on | §5, PL-R5, AC-04 |
| Manager | Approves own team's regularisation, flagged punches, optional holidays, shift swaps | RG-05, DEC-18 |
| Employee | Punches by app or web; sees own roster and attendance; raises regularisation, optional holiday and shift-swap requests | PC-R5, PC-R11, SH-R4 |
| Device (eSSL) | Pushes punches with its own token | PC-R1, DEC-46 |
| System jobs | Daily calculation, finalisation, reminders, offline alerts, period-end rules | §7, §3.26 |

Not in this package: Payroll Officer (V2) and Supervisor (V3). Their V1 duties go to the HR Manager (ATQ-05, PC-11). Kiosk is removed (DEC-17). Work-from-home is V3 (DEC-14), so PC-09 and the WFH part of PC-17 are hidden. Overtime is not calculated, shown or exported in V1 (DEC-47).

**Package 3 dependency.** Leave, permission, on-duty and comp-off (C08, C09, M02) come in Package 3, also V1. The engine has input slots for them (§7 step 8, LA-15, EC-13, UA-02/04). Until Package 3 is built, those slots are empty: no leave or permission overlays, and UA debits go straight to loss of pay. Both packages ship together in V1, so no client ever runs with empty slots.

---

## A. Shifts and roster (C05)

### US-AT-01 Define shifts [C]
As an **HR Manager**, I want to define the shifts my organisation works, so that each day has an expected schedule.
Source: SH-R1, SH-01–07, SH-11, DEC-22, DEC-23, ATT-05.

- **AC-AT-01.1** Given I create a shift, when I choose its type, then the choices are fixed / flexible / flexible with core hours / split. Open shifts and weekly-hours staff are not offered (DEC-22). "Roster" is not a shift type; rosters assign shifts (US-AT-02).
- **AC-AT-01.2** Given a fixed shift, when I save it, then it has start and end (default 09:00–18:00), required hours per day (default 8), window before and after (default 3 h / 6 h), half-day split point (default middle of shift) and break settings by reference to the policy set.
- **AC-AT-01.3** Given a flexible shift, when I save it, then it has a flexible band (earliest check-in, latest check-out) and required hours; with core hours it also has core start and end, which must fall inside the band.
- **AC-AT-01.4** Given a split shift, when I save it, then it is one shift with a fixed or windowed break of any length. Late is measured from the first segment's start, early from the last segment's end, and the half-day point is the break start (ATT-05).
- **AC-AT-01.5** Given any shift whose end is before its start, when I save it, then it must carry the **night flag** (DEC-23); SH-06 decides whether its attendance date is the start or end date (default start date).
- **AC-AT-01.6** Given a shift in use on any date, when I change its times, then the change is a new version from a chosen date; earlier dates keep the old version (BR-05). Changes into a locked period are refused (BR-AT-18).
- **AC-AT-01.7** Given a shift used by any employee or roster, when I try to delete it, then I can only deactivate it.

### US-AT-02 Assign shifts and build rosters [C]
As an **HR Manager**, I want to give each employee a default shift and, where needed, a roster, so that every date has the right shift.
Source: SH-R2, SH-R3, SH-09, EM-R3.

- **AC-AT-02.1** Given an employee, when I set a default shift from a date, then every date from then has that shift unless a roster entry says otherwise.
- **AC-AT-02.2** Given a branch, team (department) or chosen employees, when I build a roster for a week or a month on screen, then each employee-date gets a shift or a weekly off. A roster entry beats the default shift for that date.
- **AC-AT-02.3** Given a roster spreadsheet, when I upload it, then rows are validated like the screen; valid rows are applied, errors are reported per row, and a repeated upload creates no duplicates (same pattern as BR-EM-20). Limit 5,000 rows per file **[P]**.
- **AC-AT-02.4** Given a roster for a week or month, when I copy it forward, then the next week or month gets the same entries, skipping employees who have exited or not yet joined.
- **AC-AT-02.5** Given a rotation pattern (for example morning → evening → night, a week each), when I apply it to employees from a date for N weeks, then the roster entries are generated.
- **AC-AT-02.6** Given a roster change for a date in a locked period, when I save it, then it is refused (BR-AT-18). A change for an open period recalculates the affected days.

### US-AT-03 Detect the shift from the first punch [C]
As an **HR Manager**, I want staff without a roster to get the shift that fits their first punch, so that rotating staff need no daily roster.
Source: SH-08, ATT-04.

- **AC-AT-03.1** Given SH-08 is on and the employee has no roster entry for the date, when the day is calculated, then among the shifts allowed for the employee the one whose start is nearest the first punch, within that shift's window, is used. On a tie, the earlier shift.
- **AC-AT-03.2** Given no allowed shift's window contains the first punch, when the day is calculated, then the default shift is used and the day is flagged "shift not detected".
- **AC-AT-03.3** Given HR sets the employee's allowed shifts, when none are set, then auto-detect uses all active shifts of the master branch **[P]**.

### US-AT-04 See my roster [C]
As an **Employee**, I want to see my shifts and weekly offs, so that I know when to work.
Source: SH-R4.

- **AC-AT-04.1** Given I am signed in, when I open my roster, then I see the current and next week: shift name, times, weekly offs and holidays.
- **AC-AT-04.2** Given my roster changes, when the change is saved, then I see it the next time I open the roster. (Push notification of roster changes is not in V1.) **[P]**

### US-AT-05 Request a shift change or swap [C]
As an **Employee**, I want to ask for a different shift or swap with a colleague, so that I can manage my week.
Source: SH-12 (default off).

- **AC-AT-05.1** Given SH-12 is on, when I request a shift change for a future date, then my manager approves or rejects it; on approval the roster entry changes and the day is recalculated.
- **AC-AT-05.2** Given a swap with a colleague on the same date and branch, when both shifts are allowed for both of us, then one approval by my manager applies both changes **[P]**. The colleague is told.
- **AC-AT-05.3** Given SH-12 is off, then the request option is not shown and the API refuses it.

---

## B. Holidays and weekly offs (C07)

### US-AT-06 Holiday calendars per branch [C]
As an **HR Manager**, I want a holiday calendar for each branch, so that holidays follow the location.
Source: HD-R1, SH-10, DEC-26, DEC-38.

- **AC-AT-06.1** Given I create a holiday calendar for a year, when I add holidays, then each has a date, name and kind: national / festival / optional.
- **AC-AT-06.2** Given a calendar, when I link it to branches, then each branch has exactly one calendar per year (SH-10).
- **AC-AT-06.3** Given an employee transferred mid-period, when a date is calculated, then the calendar of **the branch the employee belonged to on that date** applies (DEC-26, BR-EM-19, W5).
- **AC-AT-06.4** Given a holiday that falls on a weekly off, then nothing moves (DEC-38). HD-02 is not offered.
- **AC-AT-06.5** Given a calendar used by a locked period, when I add, remove or change a holiday on a locked date, then it is refused (BR-AT-18). Changes on open dates recalculate the affected days.
- **AC-AT-06.6** Given last year's calendar, when I copy it to a new year, then national holidays keep their dates; festival and optional holidays are copied for HR to adjust **[P]**.

### US-AT-07 Weekly offs [C]
As an **HR Manager**, I want to set weekly offs, so that rest days are not counted as absence.
Source: HD-R2, SH-09.

- **AC-AT-07.1** Given a weekly-off pattern, when I set it for a branch, policy set or employee, then the choices are fixed days (for example Sunday), alternating (for example 2nd and 4th Saturday) or from the roster.
- **AC-AT-07.2** Given "from the roster", when a date has no roster entry, then it is a working day with the default shift and is flagged "no roster" **[P]**.
- **AC-AT-07.3** Given a punch on a weekly off or holiday, when PC-06 is "allow and flag" (default), then the punch is stored, the day shows worked hours, and it is flagged. (Comp-off earning from that day comes in Package 3.)

### US-AT-08 Pick optional holidays [C]
As an **Employee**, I want to pick my optional holidays, so that I can take the festivals I observe.
Source: HD-R3, HD-01, LV-06.

- **AC-AT-08.1** Given HD-01 = N (default 0), when I open optional holidays, then I see my branch's optional holidays for the year and can pick up to N.
- **AC-AT-08.2** Given I pick one, then my manager approves or rejects it. An approved pick makes that date a holiday for me only.
- **AC-AT-08.3** Given an approved pick, when I cancel it before the date, then the date is a working day again and the pick is freed.
- **AC-AT-08.4** Given HD-01 = 0, then optional holidays are shown for information only and cannot be picked.

### US-AT-09 Paid-holiday presence rule [C]
As an **HR Manager**, I want to require presence around a holiday, so that a holiday next to an absence isn't paid.
Source: HD-03, §3.12.

- **AC-AT-09.1** Given HD-03 is on, when a holiday is calculated, then the nearest working day before and after (skipping weekly offs and other holidays) must each count as presence; otherwise the holiday is marked "holiday – not paid".
- **AC-AT-09.2** Presence = present, half day, approved paid leave, on-duty, weekly off, another holiday. Not presence = loss of pay, uninformed absence, unpaid leave **[P]** (§3.12).
- **AC-AT-09.3** HD-04 and HD-05 (holiday and weekly-off pay for daily-wage and hourly workers) are stored as settings and passed to the payroll input; they change no day status in V1.

---

## C. Punch collection (C04)

### US-AT-10 Register devices [C]
As an **HR Manager**, I want to register each biometric device against a branch, so that its punches are trusted and its health is visible.
Source: PC-R2, PC-R10, PC-10, DEC-46.

- **AC-AT-10.1** Given I register a device with serial number, make, model and branch, when I save it, then the device gets **its own secret token**, shown once, to put in the device's push URL (DEC-46).
- **AC-AT-10.2** Given a device token is lost or exposed, when I rotate it, then the old token stops working at once and a new one is shown once.
- **AC-AT-10.3** Given a device, then its list entry shows online (contacted within PC-10 minutes, default 30) or offline, with the last contact time.
- **AC-AT-10.4** Given a device offline longer than PC-10 minutes, then HR Managers in scope of its branch get one alert per offline spell (NT, PC-R10), and another when it comes back **[P]**.
- **AC-AT-10.5** Given a device that has sent punches, when I remove it, then it is deactivated, never deleted; its token stops working.

### US-AT-11 Receive punches from eSSL devices [B]
As the **system**, I want to accept punches pushed by eSSL devices, so that biometric attendance arrives without manual work.
Source: PC-R1, PC-R6, PC-R7, PC-08, DEC-21, DEC-44, DEC-46, PUN-10. **Blocked by PUN-01** (device test: HTTPS support and contact rate). Written now so the rest of the design leaves room for it.

- **AC-AT-11.1** Given a push with a valid device token, when it carries punches, then every punch is stored unchanged with source "device", device, enrolment ID, device time and receipt time (BR-01, PC-R6).
- **AC-AT-11.2** Given a push without a valid token, or with a token for a different device serial, then it is rejected and logged; nothing is stored (DEC-46).
- **AC-AT-11.3** Given a live push (received within 2 minutes of device time), when the device clock differs from server time by more than PC-08 (default 5 minutes), then receipt time is used and the punch is flagged "clock mismatch" (DEC-21, PUN-10).
- **AC-AT-11.4** Given a late-synced batch (received more than 2 minutes after device time), then device time is kept, and punches older than PC-07 days (default 3) are flagged "late sync".
- **AC-AT-11.5** Given an enrolment ID mapped to no current employee, then the punch is stored as **unmapped** and not counted (DEC-44).
- **AC-AT-11.6** Given the same punch pushed twice, then it is stored once.
- **AC-AT-11.7** Given a punch for a past date, then the affected days are recalculated (PC-R7); if that date is locked, the difference goes to the next open period (PL-R4).

### US-AT-12 Map enrolment IDs [C]
As an **HR Manager**, I want to map each employee to their device enrolment ID, so that device punches reach the right person.
Source: PC-R3, PC-R8, BR-EM-24, DEC-44, DEC-45.

- **AC-AT-12.1** Given an employee, when I set their enrolment ID, then it must not belong to another current employee in the organisation (BR-EM-24). (Field already built in Package 1.)
- **AC-AT-12.2** Given each employee, then an enrolment status shows: not enrolled / enrolled / re-enrolment needed, set by HR.
- **AC-AT-12.3** Given unmapped punches exist for an enrolment ID, when HR maps that ID, then those punches are linked to the employee from the mapping's effective date and their days are recalculated (DEC-44). HR sees a list of unmapped enrolment IDs with punch counts.
- **AC-AT-12.4** Fingerprint and face templates are never stored by the system (DEC-45). Enrolment happens on the device.

### US-AT-13 Geo-fences per branch [C]
As an **HR Manager**, I want to fence each branch, so that app and web punches are checked against the site.
Source: PC-R14, PC-03, PC-16, §3.4.

- **AC-AT-13.1** Given a branch, when I add a fence by entering coordinates or picking a point on a map, then it has a centre and radius (default 200 m). A branch may have several fences, up to 10 **[P]**; radius 20 m to 5,000 m **[P]**.
- **AC-AT-13.2** Given a new fence, when I test it from the branch, then the test shows my distance and whether I am inside, before I switch it on.
- **AC-AT-13.3** Given a fence is changed, then it is a new version from now; a punch is always checked against the version **in force at punch time** (§3.4).
- **AC-AT-13.4** Given PC-16 (default master and allowed branches), then an employee's punch is checked against the fences of those branches on the punch date.

### US-AT-14 Punch from the app [C]
As an **Employee**, I want to check in and out from my phone, even offline, so that my attendance is recorded where I work.
Source: PC-R4, PC-R5, PC-R13, PC-R15, PC-02, PC-04, PC-14, PC-17, DEC-04, DEC-18, DEC-19, DEC-20, DEC-51, §3.4.

- **AC-AT-14.1** Given PC-01 allows app punches and my phone is the one bound to me (DEC-20), when I check in or out, then the punch is stored with direction, phone time, location (latitude, longitude, accuracy), distance from the nearest allowed fence, and a selfie when PC-02 requires one.
- **AC-AT-14.2** Given a phone that is not bound to me, then the punch is refused. HR can reset the binding; the next sign-in on a new phone binds it **[P]**.
- **AC-AT-14.3** Given PC-02 requires location and the phone gives none, then the punch is refused with the reason.
- **AC-AT-14.4** Given the location is outside every allowed fence, then PC-04 decides: block / record and flag (default) / allow. A flagged punch **counts provisionally** (DEC-18).
- **AC-AT-14.5** Given the accuracy is worse than PC-14 metres (default 100), then the punch is accepted / flagged (default) / blocked as set.
- **AC-AT-14.6** Given I am offline, when I punch, then the app queues it (encrypted) with a unique key and the uptime-derived time; on sync the server applies §3.4: phone time vs uptime time differ by more than PC-08 → flagged and recorded at the uptime time; over PC-07 days old → flagged late sync; into a locked period → post-lock adjustment. Syncing twice creates no duplicate.
- **AC-AT-14.7** Given an approved on-duty day, then PC-17 (default) skips the fence check but still records location. (On-duty itself arrives in Package 3.)
- **AC-AT-14.8** Given DEC-19 liveness is on for the organisation, then the selfie is checked for liveness on the server after upload and a failed check flags the punch **[B]** (provider not chosen, ATQ-13).
- **AC-AT-14.9** Selfies are kept 13 months, then deleted (DEC-51). Location is read only at the moment of a punch (PC-R15).

### US-AT-15 Punch from the web [C]
As an **Employee**, I want to check in and out from a browser, so that office staff don't need a device.
Source: PC-R11, PC-R12, PC-05, PC-12, PC-13, PC-15.

- **AC-AT-15.1** Given PC-01 allows web punches, when I check in or out, then the browser location is captured at that moment (PC-12 location on, default).
- **AC-AT-15.2** Given I refuse location or the browser can't provide it, then PC-13 decides: block (default, and I am told why) / record and flag.
- **AC-AT-15.3** Given PC-12 fence check on, then the same fence rules as the app apply (PC-04, PC-14, PC-16), including EM-07 for a fence of a branch that isn't allowed.
- **AC-AT-15.4** Given PC-05 IP restriction is on, then the client IP (taken as SEC-12 says) must be in the organisation's list. With location also on, PC-15 decides: either may pass (default) / both must pass.

### US-AT-16 Approve flagged punches [C]
As a **Manager or HR Manager**, I want to review flagged punches, so that doubtful punches are confirmed or excluded.
Source: DEC-18, CON-06, W9, NT-08.

- **AC-AT-16.1** Given a flagged punch (outside fence, poor accuracy, clock mismatch, late sync, overlap, weekly off or holiday, branch not allowed, liveness failed), then it counts provisionally and appears in the approver's queue with its reason, location, distance and selfie.
- **AC-AT-16.2** Given I am in the employee's scope and I did not make the punch, when I approve it, then the flag is cleared. Anyone in scope except the person who punched may decide (DEC-18).
- **AC-AT-16.3** Given I reject it with a reason, then an adjudication record excludes the punch (the raw punch is never changed, BR-01) and the day is recalculated (W9). If the date is locked, the difference goes to the next open period.
- **AC-AT-16.4** Given the employee is told of a flag (NT-08), when it is decided, then they are told the outcome.

### US-AT-17 Import punches from a file [C]
As an **HR Manager**, I want to import punches from a file or API call, so that punches from other systems are counted.
Source: PC-01, REL-10 (§6).

- **AC-AT-17.1** Given import is on in PC-01, when I upload a CSV or post a JSON array in the §6 format, then each row needs `external_id`, `punch_time` with offset, and `employee_code` or `enrolment_id` (+ `device_serial`).
- **AC-AT-17.2** Given a repeated `external_id` for the same organisation and source, then the row is ignored (no duplicate).
- **AC-AT-17.3** Given valid and invalid rows, then valid rows are stored with source "import", the importing user and the file checksum; invalid rows are listed in a downloadable error report.
- **AC-AT-17.4** Given latitude and longitude in a row, then the fence check applies as for app punches.

### US-AT-18 Record manual attendance during a device failure [C]
As an **HR Manager**, I want to record attendance for a branch and date when the device failed, so that employees aren't marked absent.
Source: PC-R9, PC-11, PUN-08, RG-11.

- **AC-AT-18.1** Given a branch and date, when I open manual attendance, then I see every employee expected there that day and can enter in and out times for each, with one reason for the batch.
- **AC-AT-18.2** Given I save, then each entry is a **manual punch** record, marked manual, separate from raw punches, with actor and reason, and audited (PUN-08).
- **AC-AT-18.3** PC-11 is fixed at "HR Manager only" in V1 because the Supervisor role is V3.

### US-AT-19 Adjudicate a punch [C]
As an **HR Manager**, I want to exclude a wrong punch, move it to another date or fix its direction, so that the day is right without touching the raw record.
Source: CON-06, ATQ-01, RG-10.

- **AC-AT-19.1** Given a punch, when I exclude it, change its attendance date, or change its direction, with a reason, then an adjudication record is stored with actor and reason, and the raw punch is unchanged.
- **AC-AT-19.2** Given an adjudication, when the affected days are recalculated, then the result explanation names the adjudication.
- **AC-AT-19.3** Given I adjudicate a punch on a locked date, then the difference goes to the next open period (PL-R4).
- **AC-AT-19.4** Raw punches can never be edited or deleted by anyone (ATQ-01, BR-01).

### US-AT-20 Punch at another branch [C]
As an **Employee**, I want punches at an allowed branch to count under my own rules, so that working at a sister site is normal.
Source: E17, EM-07, EM-R9, DEC-50.

- **AC-AT-20.1** Given my master branch is A and B is allowed, when I punch at B, then the day uses my own shift and rules, in **branch A's time zone** (DEC-50), and the day is costed to B.
- **AC-AT-20.2** Given C is not allowed, when I punch at C, then the punch is stored and flagged for approval (EM-07).
- **AC-AT-20.3** The punch shows the local time of the branch where it was made, for display only (DEC-50).

---

## D. Attendance rules engine (C06)

### US-AT-21 Attendance policy sets [C]
As an **HR Manager**, I want named policy sets holding all attendance settings, so that different groups follow different rules.
Source: BR-05, EM-R3, AC-04, ATT-19, §3.23, settings PR, BK, LA, EC, EA, MP, DS, RD, UA, RG, PC, SH, HD.

- **AC-AT-21.1** Given a new organisation, then it has one default policy set holding every BRD default, adjusted by the decisions (DEC-10). Settings removed by decisions are not offered (DEC-22, 24, 25, 38, 41, 47; ATQ-20 lists them).
- **AC-AT-21.2** Given I create a policy set, when I assign it to a branch, department, category, employee type or employee, then the most specific assignment wins; the employee record shows the resolved set and its source (EM-R3, already shown as "not configured" in Package 1).
- **AC-AT-21.3** Given I change a policy set, then it is a new version effective from a date. A date in a locked period is refused unless the period is first stepped back (ATT-19, DEC-07). Affected open days are recalculated.
- **AC-AT-21.4** Given AC-04 = "HR Manager with Org Admin approval", then a change goes Draft → Pending approval → Approved → Active, and nothing changes results until Active; the Org Admin can't approve their own draft (§3.23) **[P]**.
- **AC-AT-21.5** Given I save settings that don't fit together, then the save is refused with the reason. At least: the first late slab starts at grace + 1, slabs don't overlap or leave gaps (§3.5); the early-slab equivalent; half-day minimum below full-day minimum; break window inside the shift.
- **AC-AT-21.6** Every settings version is kept with who, when, old and new values and reason (G9, R26).

### US-AT-22 Calculate each day [C]
As the **system**, I want to calculate every employee's day in one fixed order, so that results are consistent and explainable.
Source: §7 (ATT-09), BR-08, DM-02, DM-04, TEC-02, BR-EM-22, DEC-50.

- **AC-AT-22.1** Given an employee and date, when the day is calculated, then the steps run in the §7 order: context → expected schedule → collect punches → clean → read into sessions → validate → hours → overlays → late and early → day status and rounding → absence handling → store.
- **AC-AT-22.2** Given a date before joining or after the last working day, then no result is produced; punches on those dates are stored, not counted (BR-EM-22).
- **AC-AT-22.3** Given a recalculation, then a **new result version** is stored with its reason (new punch, approval, settings change, adjudication); the latest is current, older ones are kept for audit (DM-02) **[P]**.
- **AC-AT-22.4** Given two recalculations of the same employee-date race, then a per-(employee, date) lock and an input version ensure an older result never overwrites a newer one (DM-04).
- **AC-AT-22.5** Given each result, then it carries an explanation: punches used and excluded, shift, rules and their versions, approvals and adjustments (BR-08). The employee and HR can read it.
- **AC-AT-22.6** Given branch-local time is past the latest shift window end for that date + 1 hour, then the finalisation job (hourly) finalises that branch-date; it runs per branch, not once a day for all (§3.26) **[P]**.

### US-AT-23 Recalculate when inputs change [C]
As the **system**, I want affected days recalculated whenever an input changes, so that results are never stale.
Source: PC-R7, CON-02, TEC-03.

- **AC-AT-23.1** Given a late or out-of-order punch, approval, adjudication, roster, calendar or policy change, then the affected employee-dates are queued for recalculation in the same transaction as the change (outbox, CON-02).
- **AC-AT-23.2** Given a nightly sweep, when any day's inputs changed after its last result, then it is queued again.
- **AC-AT-23.3** Given a settings change for 2,000 employees × 31 days, then the 62,000 days are recalculated within 30 minutes, at most 50 tasks per second per organisation (TEC-03).
- **AC-AT-23.4** Given a date in a locked period, then recalculation runs as a shadow and only the difference is posted to the next open period (§3.17).

### US-AT-24 Read punches into sessions [C]
Source: PR-01–06, DEC-41, ATT-02, W2.

- **AC-AT-24.1** Given PR-01, then the choices are direction-tagged (default) and first-in/last-out. "Paired" is removed; untagged punches are never paired by count (DEC-41).
- **AC-AT-24.2** Given punches closer than PR-02 (default 60 s), then they are one punch, keeping the first or last (PR-03).
- **AC-AT-24.3** Given an out-then-in gap shorter than PR-04 (default 5 min), then it is continuous work.
- **AC-AT-24.4** Given a session longer than PR-05 (default 16 h), then it is treated as a missed punch; more than PR-06 punches (default 12) flags the day.
- **AC-AT-24.5** Given a punch inside two shift windows, then the overlap option decides: nearest shift boundary (default) / earlier shift wins; the punch is flagged "overlap" (ATT-02, W2).
- **AC-AT-24.6** Given a night-flagged shift, then punches across midnight belong to one attendance date per SH-06 (W1).

### US-AT-25 Breaks [C]
Source: BK-01–09, W3.

- **AC-AT-25.1** Given BK-01, then break type is none / fixed auto-deduct (default) / window / flexible / flexible with cap; paid or unpaid (BK-02, default unpaid); duration default 60 min.
- **AC-AT-25.2** Given excess break time, then BK-06 decides: deduct from hours (default) / count as early minutes / flag only. "Use permission balance" applies once Package 3 exists.
- **AC-AT-25.3** Given a break outside the window, then BK-07 decides: deduct / flag (default) / both. BK-05 caps and BK-08 auto-deduct apply as set; BK-09 named breaks each carry their own rule.

### US-AT-26 Late arrival [C]
Source: LA-01–23, DEC-24, §3.5, §3.9, §3.10, PUN-09, E1–E6, E10, E12, E14.

- **AC-AT-26.1** Given LA-01 on, then late minutes are measured from shift start (default) or end of grace (LA-02); grace 0–120 min (default 10), uses per period unlimited or N (LA-04).
- **AC-AT-26.2** Given late slabs (up to 5), then actions are no action / late mark / deduct actual minutes / half day / full day / hold check-in. Quarter day is not offered (DEC-24). Defaults: 11–120 late mark, above 120 half day.
- **AC-AT-26.3** Given "hold check-in", then the punch is stored and the day is held pending manager approval; until then the slab's cut applies provisionally (PUN-09).
- **AC-AT-26.4** Given a day cut by a slab, then it also counts as a late mark only if LA-23 = yes (default no) (E6). A day cut replaces minute deductions for the same minutes (§3.2).
- **AC-AT-26.5** Given LA-13 compensation on, then extra minutes on the same day (default) or the same week cover late minutes up to the maximum (default 60). A same-week mark stays provisional until the week ends; a week crossing a lock is judged at lock with the locked part only (§3.9) **[P]**.
- **AC-AT-26.6** Given a return from an approved permission later than its end, then the extra minutes are late minutes **with no grace** (§3.10, E10) **[P]**. (Permissions arrive in Package 3.)
- **AC-AT-26.7** Given an adjusted start (after half-day leave or a permission), then grace applies to it if LA-20 = yes (default) (E12).
- **AC-AT-26.8** Given a flexible shift, then LA-21 decides (default: from core start where core hours exist, otherwise late does not apply).
- **AC-AT-26.9** Given LA-22 exemptions by category, department, employee type, employee or date, then exempt days get no late mark.
- **AC-AT-26.10** Given LA-19 on, then the manager is alerted after N marks (default 3) and HR after M (default 5) in the counter period (LA-18).
- **AC-AT-26.11** LA-15 (convert late to a permission) is stored as a setting and applies once Package 3 exists (E3).

### US-AT-27 Early check-out and early arrival [C]
Source: EC-01–17, EA-01–02, E7, E9, E16.

- **AC-AT-27.1** Early check-out mirrors late arrival with its own settings: grace default 5, slabs default 6–120 early mark / above 120 half day, no quarter day, no hold check-in, own counter or combined with late (EC-10).
- **AC-AT-27.2** Given EC-15 hours-met override (default on for flexible, off for fixed), then no early mark when required net hours are already done.
- **AC-AT-27.3** Given EC-14 prior approval on, then an unapproved early exit is early mark / half day / flag for HR as set.
- **AC-AT-27.4** Early arrival counts from shift start (default) / punch time / punch time when approved, up to EA-02 minutes (default 60). It feeds compensation (LA-13, EC-12) only; it is not overtime (DEC-47).

### US-AT-28 Missed punches [C]
Source: MP-01–05, §3.6, E13.

- **AC-AT-28.1** Given an odd number of punches or a single punch, then MP-01/MP-02 decide: flag for regularisation (default) / auto-close at shift end / count completed pairs only / half day / absent.
- **AC-AT-28.2** Given MP-04 on, then the employee is reminded at shift end + X minutes (default 30) and next morning (NT-01).
- **AC-AT-28.3** Given still unresolved at the cut-off, then MP-03 applies (default count completed pairs only, absent if none). Cut-off = end of the regularisation window (RG-04) or the period lock, whichever comes first (§3.6) **[P]**.
- **AC-AT-28.4** Given MP-05 auto check-out, then the day is always flagged.

### US-AT-29 Day status and rounding [C]
Source: DS-01–09, RD-01–04, DEC-24, §3.7, §3.8, E8.

- **AC-AT-29.1** Given DS-01, then status basis is shift-based (default for fixed, roster and split) / hours-based (default for flexible) / hybrid, with DS-02, DS-03, DS-04, DS-05, DS-07 as set. No quarter-day statuses (DEC-24).
- **AC-AT-29.2** Given DS-09 on for chosen employees, then they are present by default without punches.
- **AC-AT-29.3** Given RD-01 on, then **daily** net hours are rounded for reporting (unit and direction RD-02, RD-03), and day status uses the **unrounded** hours (§3.8) **[P]**. RD-04 applies to net hours only in V1 (§3.7, DEC-47).
- **AC-AT-29.4** Day statuses: present, half day, absent, absent – pending, weekly off, holiday, holiday – not paid; plus leave and on-duty statuses from Package 3.

### US-AT-30 Unapproved absence [C]
Source: DEC-25 (UA-01–05), §3.1, W11.

- **AC-AT-30.1** Given an absent day with no leave, then it shows "Absent – pending" for UA-01 days (default 3).
- **AC-AT-30.2** Given UA-01 has passed, then UA-02 decides: loss of pay / debit leave in the UA-03 order (default Casual → LOP). Sick is debited only if UA-05 = yes (default no).
- **AC-AT-30.3** Until Package 3 delivers leave balances, the debit step goes straight to LOP; when leave arrives, open days are recalculated.
- **AC-AT-30.4** DS-10, LM-28 and LV-04 are not offered (DEC-25).

### US-AT-31 Period-end penalties [C]
Source: LA-06–12, EC-05–11, UA-04, BR-04, §3.2, E5.

- **AC-AT-31.1** Given count-based penalties on, then after the free marks (LA-07) every Nth mark (LA-08, default 3rd) adds a half or full day penalty (LA-09; fixed amount is a pay value carried to V2) **[P]**.
- **AC-AT-31.2** Given minute-based penalties on, then when total late minutes pass X (default 60) the excess is deducted as actual minutes, or as half or full days per Y minutes.
- **AC-AT-31.3** Penalties are worked out at period end, after regularisation is settled (BR-10), and recovered in the UA-04 order (default same as UA-03).
- **AC-AT-31.4** A day cut always replaces a minute deduction for the same minutes (BR-04, §3.2). Comparing in rupees waits for V2.

---

## E. Regularisation (C06 RG)

### US-AT-32 Request a regularisation [C]
As an **Employee**, I want to correct a missed or wrong punch, so that my day is right.
Source: RG-01–09, RG-12, REL-09, ATQ-03, E13.

- **AC-AT-32.1** Given RG-01 on, when I raise a request of an enabled type (missed punch, late arrival, early check-out, wrong punch, whole day) for a date within RG-04 days (default 7) and before lock, then it is saved Pending with my reason (RG-09) and any required attachment.
- **AC-AT-32.2** Given I am over the RG-03 limit (default 3 per period), then RG-08 decides: block / HR approval only (default).
- **AC-AT-32.3** Given RG-05, then one level (manager, default) or two (manager then HR) approve. No one approves their own request (BR-07); with no other approver the Org Admin decides (DEC-31).
- **AC-AT-32.4** Given no action for RG-07 days (default 2), then it is escalated (default) / auto-approved / auto-rejected.
- **AC-AT-32.5** States: Pending, Escalated, Approved, Rejected (by approver or automatically), Withdrawn (by me before a decision), Cancelled (by HR after approval, with a reason, before lock). Every change recalculates the day (RG-12, ATQ-03).
- **AC-AT-32.6** Given HR raises a request for an employee without a smartphone, then it records "raised by HR for employee" and that HR user can't approve it (REL-09).
- **AC-AT-32.7** Given RG-06 auto-approved types, then those are approved on submission and logged.

### US-AT-33 Correct a day directly [C]
As an **HR Manager**, I want to correct a day directly with a reason, so that obvious errors don't need a request.
Source: RG-10, PUN-08.

- **AC-AT-33.1** Given RG-10 on, when I correct a day, then a **day override** or manual punch is stored with my reason, separate from raw punches, and audited.
- **AC-AT-33.2** Given the date is locked, then the difference goes to the next open period (PL-R4); DEC-34 lets HR correct any past month.

### US-AT-34 Waive late or early marks in bulk [C]
As an **HR Manager**, I want to waive marks for a date and location or shift, so that rain or a strike doesn't punish everyone.
Source: LA-17, EC-16, E14.

- **AC-AT-34.1** Given LA-17 / EC-16 on, when I waive late (or early) for a date and a branch or shift with a reason, then all those marks are cleared on recalculation and the waiver is logged.
- **AC-AT-34.2** Given a waiver is withdrawn, then the days are recalculated and the marks return.

### US-AT-35 Regularise a device outage in bulk [C]
Source: RG-11, PC-R9.

- **AC-AT-35.1** Given RG-11 on, when I regularise a branch and date range for a device outage, then each affected employee's missed punches are filled from their shift times as manual punches, with one reason, and the days are recalculated. (Differs from US-AT-18, where HR enters real times.) **[P]**

---

## F. Attendance periods and lock (C12)

### US-AT-36 Attendance periods [C]
As an **HR Manager**, I want periods generated for each payroll group, so that attendance is closed on a regular cycle.
Source: PL-R1, PL-01–03, DEC-06, §3.18.

- **AC-AT-36.1** Given a payroll group's pay period (PL-01, already stored in Package 1), then periods are generated ahead for daily, weekly, bi-weekly or monthly (calendar or custom cycle such as 26th–25th).
- **AC-AT-36.2** Given a cut-off (PL-02), then PL-03 decides days after it: assumed present and corrected next period (default) / held. Corrections from "assumed present" are automatic adjustments, tagged "actual after cut-off" (§3.18).

### US-AT-37 Pre-lock checklist and lock [C]
As an **HR Manager**, I want to see what's pending and then lock the period, so that attendance is frozen for payroll.
Source: PL-R2, PL-R3, PL-04, PL-05, DEC-49, NT-09, ATQ-05.

- **AC-AT-37.1** Given a lock unit (payroll group × branch × period, DEC-49), when I open the checklist, then I see pending requests, flagged punches, unresolved missed punches and days not finalised.
- **AC-AT-37.2** Given pending items, then PL-05 decides: block (default) / allow treating pending days as absent / allow treating them as present and correcting next period.
- **AC-AT-37.3** Given I lock "an organisation" or "a branch", then every matching lock unit is locked; an employee is never half-locked (DEC-49).
- **AC-AT-37.4** Given PL-04 = automatic on a date, then the lock runs on that date if PL-05 allows.
- **AC-AT-37.5** Who locks in V1: the HR Manager (the Payroll Officer is V2) **[P]** (ATQ-05).
- **AC-AT-37.6** Given a lock, then period-end rules run, the period's results are frozen, and the payroll input (US-AT-40) is produced.

### US-AT-38 Step a period back [C]
Source: PL-R5, PL-06, DEC-07, ATQ-04.

- **AC-AT-38.1** States per lock unit: Open → Attendance locked → Finalised → Marked as paid. In V1 (no payroll), Finalised and Marked as paid are set by hand by the HR Manager **[P]**.
- **AC-AT-38.2** Given a step back (Marked as paid → Finalised → Locked → Open), then it needs Organisation Admin approval and a reason, one step at a time, and is audited (PL-06 default).

### US-AT-39 Correct after lock [C]
Source: PL-R4, PL-07, DEC-34, §3.17, E15, W8.

- **AC-AT-39.1** Given a locked day is corrected, then the locked period is recalculated as a shadow, including period-end rules; the difference is posted to the next open period, tagged with the original date; the locked period never changes (E15, W8).
- **AC-AT-39.2** Given PL-07 (default 3 past periods), then corrections further back are allowed for HR with a warning, because DEC-34 lets HR override any past month **[P]**.
- **AC-AT-39.3** Adjustments are listed in R23 with HR corrections and automatic adjustments shown separately.

### US-AT-40 Payroll input export [C]
As an **HR Manager**, I want the period's attendance summary as an export, so that payroll (outside the system in V1) can use it.
Source: MD-06, R13, LV-10, DEC-47.

- **AC-AT-40.1** Given a locked period, then the export per employee holds payable days, deduction days with reasons, late and early minutes and marks, penalties, adjustments tagged with original dates, and days by branch. No overtime (DEC-47).
- **AC-AT-40.2** Unpaid time is shown in minutes, with hours and day-equivalents alongside (LV-10).
- **AC-AT-40.3** Formats: xlsx and csv, as in Package 1 exports.

---

## G. Views, notifications and reports

### US-AT-41 See my attendance [C]
As an **Employee**, I want to see my punches and days, so that I can spot problems early.
Source: DB-R7, BR-08.

- **AC-AT-41.1** Given I am signed in, then I see today's punch status and this period's days with status, in, out, net hours, marks and flags, and each day's explanation.
- **AC-AT-41.2** From a flagged or missed day I can start a regularisation.

### US-AT-42 Attendance notifications [C]
Source: NT-01, NT-02, NT-03, NT-05, NT-06, NT-08, NT-09, NT-10, DEC-36.

- **AC-AT-42.1** Given each event, then it can be switched on or off per channel and recipient. V1 channels: email and in-app **[P]** (SMS and WhatsApp are V3, DEC-36; push to the app comes with the mobile phase).
- **AC-AT-42.2** Events: missed-punch reminder, late or early mark, count nearing a penalty, request submitted/decided/escalated, manager daily digest of pending approvals, punch flagged, pre-lock reminder, adjustment posted, device offline.

### US-AT-43 Attendance reports [C]
Source: §14 R1–R5, R10 (days by branch), R13, R23, R26.

- **AC-AT-43.1** Reports: R1 daily attendance; R2 monthly register; R3 late and early (marks, minutes, penalties); R4 missed punch and regularisation; R5 exceptions (flagged punches, geo-fence, late sync, device status); R10 days worked by branch; R13 payroll input summary; R23 adjustments; R26 policy settings history and assignments.
- **AC-AT-43.2** Every report respects the user's scope and is exportable (xlsx/csv), using the Package 1 export pattern.

### US-AT-44 Attendance dashboard [C]
Source: DB-R3, DB-R5, DB-R7, TEC-04.

- **AC-AT-44.1** HR and Org Admin: today's present, absent, late and on leave by branch; missed punches; pending approvals; devices offline. Manager: team today and pending approvals. Employee: US-AT-41.
- **AC-AT-44.2** Figures refresh every 15 minutes, with a reload button (TEC-04), and respect scope.
