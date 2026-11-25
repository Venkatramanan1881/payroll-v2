# Business Rules Catalogue

Phase 0 artefact (SKILL.md §4). Revision 2 · 22 November 2026 · Status: **Package 1 approved at Gate 1 on 22 November 2026.** [P] rules in Package 1 are accepted as proposed.
Revision 3 · 24 November 2026: Package 2 (Attendance) rules BR-AT-01–23 added, **approved at Gate 1 on 24 November 2026** ([P] accepted as proposed). Package 1 rules unchanged.
Sources: BRD v0.10, TSD v0.4. The fixed rules BR-01 to BR-15 stay in BRD §12; this catalogue adds module rules and cites those where they apply.
Tags: **[C]** confirmed from BRD v0.10 or a decision · **[P]** proposed, needs approval.
**Enforced in** says where a rule is checked: **DB** (database constraint), **API** (backend), or both. The UI may also check, but that never replaces the backend (SKILL.md §6).

---

## Package 1 — Organisation structure (BR-OR)

| ID | Rule | Source | Tag | Enforced in |
|---|---|---|---|---|
| BR-OR-01 | Branch short code, department, designation, category and contractor codes are each unique within an organisation. | OR-R1, TSD §4.2 | C | DB |
| BR-OR-02 | A list entry or branch that is, or ever was, used by an employee is deactivated, never deleted. | OR-R1, §16 | C | API |
| BR-OR-03 | Every branch has a time zone. Attendance dates are worked out in it. | OR-R1, BR-11 | C | DB |
| BR-OR-04 | Organisation settings and list changes record who, when, old value, new value and reason. | G9 | C | API |
| BR-OR-05 | The organisation short code is unique across the platform and set by the vendor. | VC-R1 | C | DB |
| BR-OR-06 | Each organisation always has a default payroll group, created with the organisation. A group with members on any date can't be deleted. | PG-R5 | C / P (delete) | API |

## Package 1 — Employee Master (BR-EM)

| ID | Rule | Source | Tag | Enforced in |
|---|---|---|---|---|
| BR-EM-01 | An employee code is unique within an organisation and belongs to one person for good. Default pattern: organisation short code + branch short code + 4-digit running number per branch. The pattern is changeable (EM-01). Running numbers are never reused, and the code never changes on transfer. | EM-01, EM-05 | C | DB + API |
| BR-EM-02 | On every date between joining and exit, an employee has exactly one master branch. | EM-R9 | C | DB (no overlapping ranges) |
| BR-EM-03 | The master branch is always allowed. Allowed branches follow EM-06. A branch is never both master and allowed for the same dates. | EM-R9, EM-06 | C | API |
| BR-EM-04 | On every date, an employee is in exactly one payroll group. Moves are dated. | PG-R3, PG-R5 | C | DB (no overlap, no gap) |
| BR-EM-05 | Dated fields (master and allowed branches, department, designation, category, employee type, reporting manager, shift/roster, policy set, payroll group, status) are kept as versions with effective dates. A saved version is never edited in place; a correction is a new version with a reason. | EM-R6, G5 | C | DB + API |
| BR-EM-06 | A dated change may take effect in an open period, today or in the future. It may not take effect inside a locked period unless that period is unlocked first (PL-06). A scheduled change can be cancelled before it starts. | EM-R6, BR-06 | C (dates) / P (cancel) | API |
| BR-EM-07 | Statuses: Active, Probation, Notice, Long leave, Absconding, Exited. Every change is raised with a reason and approved before it takes effect. Only one change can be pending per employee. | EM-R5 | C | DB + API |
| BR-EM-08 | When an exit takes effect, every session and refresh token of that employee is revoked and sign-in is refused. | ACC-09, TSD §8 | C | API |
| BR-EM-09 | A mobile number belongs to only one current employee in the organisation, and is freed on exit. Mobile number is mandatory for every employee. | EM-R12, DEC-30, DQ-03 | C | DB + API |
| BR-EM-10 | Employees sign in with organisation short code + employee code + PIN. PINs are stored only as an argon2 hash. A temporary PIN expires after 7 days and must be replaced at first sign-in. An email one-time code is offered only where an email address is held. | UM-R3, TSD §8, §15 | C | API |
| BR-EM-11 | The reporting manager is a current employee of the same organisation, never the employee themself, and the chain never loops. | EM-R2 | C | API |
| BR-EM-12 | Employees in status Active, Probation, Notice, Long leave or Absconding count toward the plan limit. Exited employees don't. VC-01 decides: block, or allow and alert. | VC-01, DEC-35 | C | API |
| BR-EM-13 | Always mandatory: name, employee code, mobile number, master branch, joining date, employee type, category, department, payroll group, status. | EM-02 | C | API |
| BR-EM-14 | From V2, bank numbers are encrypted per field and masked (last 4 digits). Showing them in full is a separate permission, and is audited. | TSD §15 | C | API + DB |
| BR-EM-15 | Custom fields (text, number, date, yes/no, dropdown, multiple choice) are for record-keeping. No V1 attendance rule reads them. | EM-R8 | C | API |
| BR-EM-16 | No one approves a request they raised (BR-07). | BR-07 | C | API |
| BR-EM-17 | Every create, change, approval, rejection, PIN reset, role-permission change and sign-in lockout is written to the audit log in the same database transaction, with actor, organisation, action, target, time, old/new values and reason where required. | TSD §3.3, SKILL §6 | C | DB + API |
| BR-EM-18 | A user sees and changes only employees whose master branch (and department, where scoped) is in their access scope today. Employees whose allowed branch is in scope follow EM-10 (default view only). A Manager sees their whole chain below, read-only. | UM-R2, AC-07, EM-10, EM-R14 | C | API + DB (org isolation) |
| BR-EM-19 | When an employee changes branch mid-period, each day is calculated with the branch (and its holiday calendar) the employee belonged to on that day. | DEC-26 | C | Package 2 engine |
| BR-EM-20 | Bulk upload applies the same validation as the screen. Valid rows are applied; invalid rows are reported and not applied. A repeated upload creates no duplicates. | EM-R7 | C | API |
| BR-EM-21 | A rejoin keeps the old employee code and adds a new employment period to the same employee. Earlier periods and their history are unchanged. | EM-05, EM-R13 | C | API |
| BR-EM-22 | No attendance result is produced for dates before joining or after the last working day. Punches on those dates are stored but not counted. | BR-01, PC-R6 | P | Package 2 engine |
| BR-EM-23 | An employee's own contact-detail edit (EM-03 on) applies at once without approval, and is audited. | EM-03 | C | API |
| BR-EM-30 | Status moves are limited to those listed in EM-R5; Exited is final for that employment. An approved future status change can be cancelled before it starts by the Org Admin or an HR Manager, with a reason. | EM-R5 | C | API |
| BR-EM-32 | Portal access is set per employee (none / employee portal), default none. An employee without portal access has no user account and no sign-in details, and cannot log in. Removing access revokes all their sessions. | EM-R15, UM-R3, DQ-09 | C | API |
| BR-EM-31 | HR can extend probation by changing its end date, with a reason, without approval. | EM-04 | C | API |
| BR-EM-24 | A device enrolment ID belongs to one current employee in the organisation, and is freed on exit. | PC-R3 | C | DB |
| BR-EM-25 | Two users saving the same employee: the second save is refused as a conflict, never silently overwritten. | SKILL Ph6 | P | API |
| BR-EM-26 | Contract employees need a contractor only when EM-11 = yes (default no). | EM-11 | C | API |
| BR-EM-27 | Status changes are approved by the Organisation Admin. When the Organisation Admin raised the change, an HR Manager approves. | EM-R5 | C | API |
| BR-EM-28 | V1 holds no identity numbers and no pay or bank details. | EM-R11, EM-R4 | C | API + schema |
| BR-EM-29 | Role permissions and role assignment can be changed by the Organisation Admin and the HR Manager. The last Org Admin can't be removed, and no one can grant rights beyond their own scope. | AC-01, CL-13 | C / P (limits) | API |

---

## Package 2 — Attendance (BR-AT) — approved at Gate 1, 24 November 2026

Sources: BRD v0.13, `../Payroll-App-Issue-Resolutions-v0.2.md` (DEC-01–51). Stories: `USER_STORIES/AT-attendance.md`.

| ID | Rule | Source | Tag | Enforced in |
|---|---|---|---|---|
| BR-AT-01 | Raw punches are never edited or deleted, by anyone. A wrong punch is fixed by an adjudication (exclude, change date, change direction), a manual punch or a day override, each with actor and reason. | BR-01, ATQ-01, CON-06, PUN-08 | C | DB (no UPDATE/DELETE grant) + API |
| BR-AT-02 | Every punch is stored with source, device or user, device/phone time, receipt time and location when given. The same punch sent twice (same device + time, same offline key, same import `external_id`) is stored once. | PC-R6, §3.4, §6 | C | DB (unique keys) |
| BR-AT-03 | A device push is accepted only with that device's own valid token. A rotated or removed device's old token stops working at once. | DEC-46 | C | API |
| BR-AT-04 | A punch whose enrolment ID maps to no current employee is stored as unmapped and not counted. When the ID is mapped, those punches are linked and their days recalculated. | DEC-44 | C | API |
| BR-AT-05 | Fingerprint and face templates are never stored. Selfies are sensitive personal data, kept 13 months, then deleted. | DEC-45, SEC-08, DEC-51 | C | API + storage policy |
| BR-AT-06 | The attendance date, the shift and late/early minutes use the employee's **master branch time zone on that date**, wherever the punch is made. | DEC-50, BR-11 | C | API (engine) |
| BR-AT-07 | Each date uses the holiday calendar of the branch the employee belonged to on that date (BR-EM-19). A holiday on a weekly off doesn't move. | DEC-26, DEC-38 | C | API (engine) |
| BR-AT-08 | A flagged punch counts provisionally until decided. Anyone in the employee's scope except the person who punched may approve or reject it. Rejection excludes it through an adjudication. | DEC-18 | C | API |
| BR-AT-09 | A geo-fence check uses the fence version in force at the punch time, not at sync time. | §3.4 | C | API |
| BR-AT-10 | A day is calculated in the fixed §7 order. Each recalculation adds a result version with its reason; the latest is current. An older calculation can never overwrite a newer one. | ATT-09, DM-02, DM-04 | C / P (versions) | DB + API |
| BR-AT-11 | Day status uses unrounded net hours. Rounding (RD) applies to daily net hours for reporting only. | §3.8, RD-01–04 | P | API (engine) |
| BR-AT-12 | There are no quarter-day statuses or cuts. A half day is half of the employee's own shift. | DEC-24 | C | API |
| BR-AT-13 | One shortfall has one effect: a day cut (half or full day) replaces any minute deduction for the same minutes. | BR-04, §3.2 | C | API (engine) |
| BR-AT-14 | Late and early slabs are inclusive whole-minute ranges. The first slab starts at grace + 1; slabs don't overlap or leave gaps. Settings that break this aren't saved. | §3.5 | C | API |
| BR-AT-15 | No one approves a request they raised (BR-07). When no other approver is available, the Organisation Admin decides. | BR-07, DEC-31 | C | API |
| BR-AT-16 | Count- and minute-based penalties, excess absence and absconding flags are worked out at period end, after regularisation is settled. | BR-10, §7 | C | API |
| BR-AT-17 | The lock unit is payroll group × branch × period. Locking an organisation or branch locks every matching unit; an employee is never half-locked. | DEC-49 | C | DB + API |
| BR-AT-18 | A locked period never changes. Settings, shift, roster and calendar changes dated inside it are refused. Punches, approvals and corrections for its dates are recalculated as a shadow, and only the difference is posted to the next open period, tagged with the original date. | BR-06, PL-R4, ATT-19, §3.17 | C | API |
| BR-AT-19 | A period goes back one step at a time (Marked as paid → Finalised → Locked → Open), each step with Organisation Admin approval and a reason, and audited. | PL-R5, PL-06, ATQ-04 | C | API |
| BR-AT-20 | No overtime is calculated, shown or exported in V1. Early arrival only feeds compensation. | DEC-47, DEC-14 | C | API |
| BR-AT-21 | Exactly one attendance policy set version is in force for an employee on a date; the most specific assignment wins (employee → employee type → category → department → branch → organisation default). | BR-05, EM-R3 | P (order) | API |
| BR-AT-22 | An employee app punch is accepted only from the phone bound to that employee. HR can reset the binding. | DEC-20 | C / P (reset) | API |
| BR-AT-23 | Every create, change, approval, rejection, adjudication, waiver, lock and step back is written to the audit log in the same transaction (BR-EM-17). | G9, SEC-10 | C | API + DB |

