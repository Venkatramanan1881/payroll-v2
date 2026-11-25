# Default Permission Matrix — Package 1

Phase 2 artefact (SKILL.md §4, §6). Status: **Approved at Gate 2, 22 November 2026.** Answers ACC-01.
These are the **defaults** for the built-in roles. The Org Admin and HR Manager can change them (AC-01, CL-13), except rows marked 🔒, which are fixed by business rules and can't be granted or removed.

Legend: ✔ allowed · S allowed within the user's access scope (branches/departments, EM-10) · Own = own record only · Chain = own reporting chain · — not allowed.
Every operation is checked in the backend on every request. Hiding a button is never the control (SKILL.md §6).

| Operation code | Operation | Vendor Admin | Org Admin | HR Manager | Manager | Employee |
|---|---|---|---|---|---|---|
| `org.create` | Create organisation, set short code, plan, limit | ✔ 🔒 | — | — | — | — |
| `org.limit.set` | Change employee limit / VC-01 | ✔ 🔒 | — | — | — | — |
| `org.settings.edit` | Edit OR-01–04, EM-01–11 | — | ✔ | ✔ | — | — |
| `org.branch.manage` | Create, edit, deactivate branches | — | ✔ | ✔ | — | — |
| `org.lists.manage` | Departments, designations, categories, types | — | ✔ | ✔ | — | — |
| `org.contractor.manage` | Contractors | — | ✔ | ✔ | — | — |
| `org.payroll_group.manage` | Create/edit groups, bulk assign by rule | — | ✔ | ✔ | — | — |
| `employee.view` | View employee record | — 🔒 (support access only) | ✔ | S | Chain | Own |
| `employee.create` | Create employee | — | ✔ | S | — | — |
| `employee.edit` | Edit personal and job details, schedule/cancel job changes | — | ✔ | S | — | — |
| `employee.history.view` | View dated history, "as on date" | — | ✔ | S | — | — |
| `employee.status.raise` | Raise / withdraw a status change | — | ✔ | S | — | — |
| `employee.status.approve` | Approve or reject a status change | — | ✔ (not own 🔒) | ✔ only if the Org Admin raised it, and in scope 🔒 | — | — |
| `employee.status.cancel` | Cancel an approved future status change, with reason | — | ✔ | S | — | — |
| `employee.probation.extend` | Change probation end date, with reason | — | ✔ | S | — | — |
| `employee.rejoin` | Rejoin (keeps old code) | — | ✔ | S | — | — |
| `employee.bulk_import` | Upload, view result | — | ✔ | S | — | — |
| `employee.pin.issue` | Grant or remove portal access; issue / reset temporary PIN | — | ✔ | S | — | — |
| `employee.profile.edit_own` | Edit own contact details, applied at once (EM-03) | — | — | — | — | Own |
| `employee.custom_fields.manage` | Define custom fields | — | ✔ | ✔ | — | — |
| `access.role.edit` | Edit role permissions, create custom roles | — | ✔ | ✔ (only within own scope 🔒) | — | — |
| `access.role.assign` | Assign users to roles and scopes | — | ✔ | ✔ (only within own scope 🔒) | — | — |
| `report.r10` | Employee list and headcount | — | ✔ | S | Chain | — |
| `report.r11` | Joiners and leavers | — | ✔ | S | Chain | — |
| `report.export` | Export a report to spreadsheet | — | ✔ | S | — | — |
| `audit.view` | View the audit trail | — | ✔ | S | — | — |
| `support.access` | Open an organisation's data for support (VC-R6) | ✔ with the Org Admin's approval, time-limited 🔒 | approves 🔒 | — | — | — |

## Fixed rules (🔒)

1. No one approves a request they raised (BR-07, BR-EM-16).
2. The vendor never sees employee data except through approved, time-limited support access, which is logged and visible to the client (TB-01, VC-R6).
3. The last Org Admin can't lose the role (BR-EM-29).
4. No one can grant a permission or scope wider than their own (BR-EM-29).
5. Employees act only on their own record.
6. Organisation isolation is enforced by the database for every role (TSD §4.1). No permission overrides it.

## Notes

- A read-only "auditor-like" access is made as a custom role with `*.view`, `report.*` and `audit.view` (DEC-29).
- Package 2 adds attendance operations to this matrix. Package 1 operation codes won't change.

---

## Package 2 — Attendance (approved at Gate 2, 24 November 2026)

Same legend. **Org** = the HR Manager must have every branch of the organisation in scope (for organisation-wide settings). Package 1 codes are unchanged.

| Operation code | Operation | Vendor Admin | Org Admin | HR Manager | Manager | Employee |
|---|---|---|---|---|---|---|
| `attendance.shift.manage` | Shifts and shift versions | — | ✔ | Org | — | — |
| `attendance.roster.manage` | Default shifts, rosters, upload, copy, rotation | — | ✔ | S | — | — |
| `attendance.roster.view` | View rosters | — | ✔ | S | Chain | Own |
| `attendance.shift_swap.request` | Ask for a shift change or swap (SH-12) | — | — | — | — | Own |
| `attendance.shift_swap.approve` | Decide a shift change or swap | — | ✔ | S | Chain (not own 🔒) | — |
| `attendance.calendar.manage` | Holiday calendars and branch links, weekly-off patterns | — | ✔ | S (own branches' links) | — | — |
| `attendance.optional_holiday.request` | Pick / withdraw / cancel optional holidays | — | — | — | — | Own |
| `attendance.optional_holiday.approve` | Decide picks | — | ✔ | S | Chain (not own 🔒) | — |
| `attendance.device.manage` | Register, rotate token, deactivate devices | — | ✔ | S | — | — |
| `attendance.enrolment.manage` | Map enrolment IDs, set enrolment status, see unmapped IDs | — | ✔ | S | — | — |
| `attendance.fence.manage` | Geo-fences: create, test, activate | — | ✔ | S | — | — |
| `attendance.device_binding.reset` | Reset an employee's bound phone | — | ✔ | S | — | — |
| `attendance.punch.self` | Check in / out from app or web | — | — | — | — | Own 🔒 |
| `attendance.punch.view` | View punches (with location and selfie) | — | ✔ | S | Chain | Own |
| `attendance.punch.review` | Approve or reject flagged punches | — | ✔ | S | Chain | — (never own punch 🔒) |
| `attendance.punch.import` | Import punches from file or API | — | ✔ | S | — | — |
| `attendance.punch.manual` | Manual attendance for a device failure | — | ✔ | S | — | — |
| `attendance.punch.adjudicate` | Exclude, include again, move date, change direction | — | ✔ | S | — | — |
| `attendance.policy.manage` | Policy sets, versions, assignments; period settings | — | ✔ | Org | — | — |
| `attendance.policy.approve` | Approve settings when maker-checker is on | — | ✔ (not own 🔒) | — | — | — |
| `attendance.day.view` | View day results and explanations | — | ✔ | S | Chain | Own |
| `attendance.day.correct` | Direct correction / day override (RG-10) | — | ✔ | S | — | — |
| `attendance.waiver.manage` | Bulk waivers (LA-17, EC-16) | — | ✔ | S | — | — |
| `attendance.outage.regularise` | Bulk regularisation for a device outage (RG-11) | — | ✔ | S | — | — |
| `attendance.regularisation.request` | Raise / withdraw a regularisation | — | ✔ (on behalf) | S (on behalf) | — | Own |
| `attendance.regularisation.approve` | Decide at level 1 or 2 | — | ✔ | S (level 2, and level 1 when escalated) | Chain (level 1) | — (never own 🔒) |
| `attendance.regularisation.cancel` | Cancel an approved request, with reason | — | ✔ | S | — | — |
| `attendance.period.view` | View periods, lock units, checklist | — | ✔ | S | — | — |
| `attendance.period.lock` | Lock, finalise, mark as paid | — | ✔ | S (units of own branches) | — | — |
| `attendance.period.step_back` | Step a lock unit back, with reason | — | ✔ 🔒 | — 🔒 | — | — |
| `attendance.adjustment.view` | View adjustments | — | ✔ | S | — | Own |
| `attendance.payroll_input.export` | Payroll input export (MD-06, R13) | — | ✔ | S | — | — |
| `report.r1`–`report.r5` | Daily, monthly register, late/early, missed punch and regularisation, exceptions | — | ✔ | S | Chain | — |
| `report.r23` | Adjustments and arrears | — | ✔ | S | — | — |
| `report.r26` | Settings history and assignments | — | ✔ | Org | — | — |
| `dashboard.attendance` | Attendance dashboard widgets | — | ✔ | S | Chain | Own |
| `notification.settings.manage` | Notification events, channels, recipients | — | ✔ | Org | — | — |

Fixed rules added (🔒):
7. No one decides a flagged punch they made, or a request, pick or swap they raised (BR-AT-08, BR-AT-15).
8. Only the Organisation Admin steps a period back (DQ-AT-02).
9. Employees punch only for themselves, from their bound phone (BR-AT-22).
10. A user limited to some branches locks only those branches' lock units; "lock organisation" needs every branch in scope.
