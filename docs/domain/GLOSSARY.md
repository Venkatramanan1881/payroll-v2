# Domain Glossary — Package 1

Phase 1 artefact (SKILL.md §4). These terms are used the same way in the documents, the code and the API.

| Term | Meaning | Not to be confused with |
|---|---|---|
| **Organisation** | A client of the platform: the tenant. Every client record belongs to exactly one. Identified by its platform-unique **short code**. | Branch |
| **Branch** | A workplace of an organisation, with its own short code and time zone. | Department |
| **Reference list** | Departments, designations, categories, employee types: org-maintained lists, each its own entity. | A generic key/value table |
| **Contractor** | A firm that supplies contract workers. | Employee type "contract" |
| **Payroll group** | A set of employees sharing one pay period (V1) and, from V2, pay rules. | Department |
| **Pay period** | The cycle a payroll group follows: daily, weekly, bi-weekly or monthly (calendar or custom). | Financial year |
| **Employee** | One person's record in one organisation, identified by an employee code. Holds one or more employment periods. | User (sign-in account) |
| **Employment period** | One continuous stretch from joining date to last working day. A rejoin always keeps the old code and adds a new period to the same employee. | Status period |
| **Job version** | A dated snapshot of an employee's job fields (master branch, allowed branches, department, designation, category, type, manager, shift/roster reference, policy-set override, payroll group), valid over a date range. | Audit entry |
| **Effective date** | The calendar date from which a version or status applies, read in the employee's master-branch time zone. | Recorded time (when the change was entered) |
| **Recorded time** | The moment a change was saved. Together with the effective date, it answers "what did we know, and when". | Effective date |
| **Scheduled change** | A job version or approved status change whose effective date is in the future. | Pending request |
| **Status** | The employee's employment state: Active, Probation, Notice, Long leave, Absconding, Exited. | Request state (pending/approved) |
| **Status change request** | HR's request to change a status, approved by someone other than the raiser. | Status |
| **Master branch** | The one branch the employee belongs to on a date. | Allowed branch |
| **Allowed branch** | Another branch where the employee may punch. | Master branch |
| **Enrolment ID** | The ID a biometric device knows the employee by. One current employee per ID in the organisation. | Employee code |
| **User** | A sign-in account (admin or employee). An employee user is linked to one employee. | Employee |
| **Portal access** | Whether an employee may sign in (none / employee portal). Default none: the employee is a record only. | Role |
| **Temporary PIN** | A PIN issued by HR, valid for 7 days, replaced at first sign-in. | PIN |
| **Role** | A named set of permissions. Built-in or custom. | Access scope |
| **Access scope** | Which branches or departments a user may act on. | Role |
| **Permission** | The right to perform one operation (e.g. `employee.status.approve`). | Screen visibility |
| **Bulk import** | One uploaded spreadsheet and its row-by-row outcome. | Punch import (Package 2) |
| **Audit entry** | An append-only record of who did what, to which record, when, with old/new values. | Job version |

## Package 2 — Attendance (approved at Gate 2, 24 November 2026)

| Term | Meaning | Not to be confused with |
|---|---|---|
| **Raw punch** | A check-in or check-out exactly as received from a device, app, web or import. Never changed or deleted. | Manual punch |
| **Manual punch** | A punch entered by HR or created by an approved regularisation; always shown as manual. | Raw punch |
| **Adjudication** | A decision about a raw punch: exclude, include again, move to another date, change direction. The raw punch stays as it was. | Editing a punch |
| **Day override** | HR's direct correction of a day's status or hours, with a reason. | Regularisation request |
| **Flagged punch** | A punch with a reason to check (outside fence, clock mismatch, late sync…). It counts until rejected. | Missed punch |
| **Unmapped punch** | A device punch whose enrolment ID matches no current employee. Stored, not counted. | Rejected push |
| **Attendance date** | The date a punch counts toward, in the master branch's time zone, using the shift window and the night rule. | Calendar date of the punch |
| **Shift window** | The hours before shift start and after shift end in which punches belong to that shift. | Flexible band |
| **Policy set** | A named bundle of every attendance setting, versioned by date, assigned to groups or employees. | Payroll group |
| **Day result** | The calculated outcome of one employee-date; every recalculation adds a version. | Punch |
| **Finalised day** | A day past its window with nothing open on it. | Locked day |
| **Lock unit** | One attendance period × one branch (for one payroll group). The thing that gets locked. | Attendance period |
| **Adjustment** | The difference from a correction to a locked date, posted to the next open period, tagged with the original date. | Editing a locked period |
| **Shadow recalculation** | Recalculating a locked period in memory to find the difference for an adjustment. | Unlocking |
| **Absent – pending** | An absent day still within the UA-01 days for applying leave. | Loss of pay |
| **Waiver** | HR clearing late or early marks for a date and branch or shift. | Regularisation |
| **Device token** | The secret in each device's push URL; rotating it stops the old one at once. | Employee sign-in token |
