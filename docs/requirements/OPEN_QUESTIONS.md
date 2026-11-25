# Open Questions — Build Packages

Phase 0 artefact (SKILL.md §4). Revision 3 · 24 November 2026: Package 2 Phase 0 questions ATQ-05–20 added. Revision 2 · 22 November 2026.
The project-wide register stays in `../Payroll-App-Open-Issues-v0.1.md` and `../Payroll-App-Issue-Resolutions-v0.2.md`.

## Package 1 — Employee Master: answered 22 November 2026

All applied in BRD v0.10 / TSD v0.4 and in the stories (revision 2).

| ID | Answer | Now in |
|---|---|---|
| EMQ-01 | CTC in V2 | EM-R4 |
| EMQ-02 | Bank details in V2 | EM-R4 |
| EMQ-03 | Statuses Active, Probation, Notice, Long leave, Absconding, Exited | EM-R5 |
| EMQ-04 | Org Admin approves; HR Manager approves when the Org Admin raised it | EM-R5, BR-EM-27 |
| EMQ-05 | Default code = org short code + branch short code + 4-digit number; custom pattern in settings | EM-01, VC-R1, OR-R1 |
| EMQ-06 | Mobile unique within the organisation; freed on exit | EM-R12 |
| EMQ-07 | "No mobile" allowed. **Superseded by DQ-03:** mobile number is mandatory | EM-R12 (v0.12) |
| EMQ-08 | Temporary PIN shown once / printed / emailed; 7-day expiry | UM-R3 |
| EMQ-09 | Text, number, date, yes/no, dropdown, multiple choice | EM-R8 |
| EMQ-10 | Valid rows applied, errors reported | EM-R7 |
| EMQ-11 | Always-mandatory list | EM-02 |
| EMQ-12 | Future-dated changes allowed | EM-R6 |
| EMQ-13 | No identity numbers in V1 | EM-R11 |
| EMQ-14 | Probation is a status, not a type | OR-R3 |
| EMQ-15 | Default payroll group created | PG-R5 |
| EMQ-16 | Policy set shown as resolved, employee override optional | EM-R3 |
| EMQ-17 | Enrolment ID unique per organisation | PC-R3 |
| EMQ-18 | Exit: last working day, reason list, remarks | EM-R5 |
| EMQ-19 | Rejoin linked; link changeable. **Superseded by DQ-07:** rejoin always keeps the old code, no link | EM-R13 (v0.11) |
| EMQ-20 | Contractor optional; setting EM-11 | EM-11 |
| EMQ-21 | Reminder default 15 days, changeable | EM-04 |
| EMQ-22 | Allowed-branch employees view only by default; setting EM-10 | EM-10 |
| EMQ-23 | Manager sees the whole chain | EM-R14 |
| CL-13 | Both Org Admin and HR Manager edit role permissions and assign roles | AC-01, §5 |
| ACC-01 | Default permission matrix produced in Phase 1 | AC-01 |

## Package 1 — proposed values accepted at Gate 1 (22 November 2026)

| ID | Item | Proposed |
|---|---|---|
| EMQ-24 | PIN length | 6 digits |
| EMQ-25 | Lockout after wrong PIN / code | 5 tries → 15 minutes |
| EMQ-26 | Short code length and characters | Org and branch: 2–6 letters or digits, upper case |
| EMQ-27 | Running number passes 9999 | Add a digit automatically; warn HR at 90% used |
| EMQ-28 | Branch short code changed after use | Existing employee codes unchanged |
| EMQ-29 | Bulk upload speed | 2,000 rows within 2 minutes |
| EMQ-30 | Field made mandatory later | Existing records listed as "incomplete", not blocked |
| EMQ-31 | Attendance before joining / after exit | Punches stored, not counted (BR-EM-22) |
| EMQ-32 | Concurrent edits | Second save gets a conflict (BR-EM-25) |

## Package 2 — Attendance: early answers (22 November 2026)

Given while reviewing the draft attendance lifecycles, before Package 2 Phase 0. Applied in BRD v0.13 (RG-12, PL-R5, PL-06, BR-06).

| ID | Question | Answer |
|---|---|---|
| ATQ-01 | Can a punch be cancelled? | No. Raw punches are never changed or deleted; a wrong punch is fixed by regularisation (RG-02). |
| ATQ-02 | Can a day result be cancelled? | No. It is recalculated, never cancelled. |
| ATQ-03 | Regularisation request end states | Add **Withdrawn** (employee, while Pending or Escalated), **Cancelled** (HR, after approval, with a reason, before lock; day recalculated) and **Rejected (auto)** (RG-07 auto-reject). |
| ATQ-04 | Can an attendance period go back? | Yes. Each step can go back one: Marked as paid → Finalised → Locked → Open. Every step back, including from Marked as paid, needs Org Admin approval and a reason, and is audited (answered 22 November 2026). BR-06 ("a closed pay run never changes") must be reworded in Package 2 Phase 0 to allow this controlled exception. |

## Package 2 — Attendance: decided 24 November 2026 (before Phase 0)

| ID | Answer | DEC |
|---|---|---|
| CL-07 | Per-device secret token in the push URL | DEC-46 |
| CL-08 | No overtime calculated, shown or exported in V1 | DEC-47 |
| CL-14 | Punch and daily-result tables partitioned by month from day one | DEC-48 |
| CL-17 | Lock unit = payroll group × branch × period | DEC-49 |
| ATT-08 | Master branch time zone decides date and minutes | DEC-50 |
| SEC-07 | Selfies kept 13 months | DEC-51 |
| PUN-01 | Device test not run; Package 2 goes ahead, US-AT-11 (eSSL gateway) stays blocked until it is | — |

## Package 2 — Attendance: Gate 1 (approved as proposed, 24 November 2026)

**Proposed values — accepted as proposed at Gate 1.**

| ID | Item | Proposed | Story |
|---|---|---|---|
| ATQ-05 | Who locks periods in V1 (Payroll Officer is V2) | HR Manager; Finalised and Marked as paid set by hand in V1 | US-AT-37, US-AT-38 |
| ATQ-06 | Roster upload limit | 5,000 rows per file, same behaviour as employee bulk upload | US-AT-02 |
| ATQ-07 | Auto-detect shift with no allowed shifts set | All active shifts of the master branch | US-AT-03 |
| ATQ-08 | Weekly offs "from roster" with no roster entry | Working day on the default shift, flagged "no roster" | US-AT-07 |
| ATQ-09 | Geo-fence limits | Up to 10 fences per branch; radius 20 m – 5,000 m | US-AT-13 |
| ATQ-10 | Device offline alerts | One alert per offline spell, and one when back online | US-AT-10 |
| ATQ-11 | Phone binding reset | HR resets; the next sign-in on a new phone binds it | US-AT-14 |
| ATQ-12 | Shift swap | One approval by the requester's manager applies both sides | US-AT-05 |
| ATQ-14 | Bulk outage regularisation (RG-11) | Fills missed punches from shift times as manual punches | US-AT-35 |
| ATQ-15 | Corrections older than PL-07 periods | Allowed for HR with a warning (DEC-34) | US-AT-39 |
| ATQ-16 | Notification channels in V1 | Email and in-app; app push with the mobile phase | US-AT-42 |
| ATQ-17 | Policy set resolution order | Employee → employee type → category → department → branch → organisation default | BR-AT-21 |
| ATQ-18 | Holiday calendar copy to a new year | National dates kept; festival and optional copied for HR to adjust | US-AT-06 |
| ATQ-19 | Fixed-amount count penalty (LA-09) in V1 | Stored and exported as a count; rupee value applied in V2 | US-AT-31 |

**Explained in Issue Resolutions §3 — confirmed at Gate 1** (used in the stories as written there):

| Ref | Recommendation | Story |
|---|---|---|
| §3.2 | Day cut replaces minute deduction; rupee comparison in V2 | US-AT-26, 31 |
| §3.6 | Missed-punch cut-off = RG-04 window end or lock, whichever first | US-AT-28 |
| §3.8 | Round daily net hours; status from unrounded hours | US-AT-29 |
| §3.9 | Same-week compensation provisional until week end; locked part judged at lock | US-AT-26 |
| §3.10 | No grace on permission over-stay | US-AT-26 |
| §3.12 | Presence around a holiday (HD-03) list | US-AT-09 |
| §3.23 | Maker-checker states for settings | US-AT-21 |
| §3.25 | Keep every result version | US-AT-22 |
| §3.26 | Hourly finalisation per branch-local date | US-AT-22 |

**Still open [Q]**

| ID | Question | Blocks |
|---|---|---|
| ATQ-13 | Liveness check for selfies (DEC-19): which service, and its cost per check? Until chosen, selfies are stored without a liveness check. | AC-AT-14.8 |
| PUN-01 | eSSL device test: exact models, HTTPS push support, contact rate. | US-AT-11 |
| ATQ-20 | **Done 24 November 2026: BRD v0.14 and TSD v0.8 written.** BRD v0.14 to absorb decisions the stories already follow: PR-01 "paired" removed (DEC-41); quarter-day options removed (DEC-24); DS-10/LM-28/LV-04 merged into UA (DEC-25); HD-02 removed (DEC-38); kiosk removed (DEC-17); open shifts and weekly-hours removed (DEC-22); night shift as a flag (DEC-23); PC-09 and WFH out of V1 (DEC-14); E18 counted provisionally (DEC-18); RD-04 net hours only, no overtime in V1 (DEC-47, §3.7); device token (DEC-46); master-branch time zone (DEC-50); selfie retention (DEC-51). TSD v0.8: size D and monthly partitioning (DEC-48), selfie retention. | Documents only, not the build |
