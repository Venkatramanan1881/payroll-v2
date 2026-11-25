# Domain Model — Package 1: Employee Master

Phase 1 artefact (SKILL.md §4). Status: **Approved at Gate 2, 22 November 2026.**
Inputs: stories and rules approved at Gate 1 (`../product/`), BRD v0.10, TSD v0.4. This models business concepts, not tables. Tables come in Phase 3.

---

## 1. Modules and ownership

The TSD runs one API service (a modular monolith, TSD §3, §5.1). Each module owns its data; other modules read it only through that module's interface or its events, never by writing to its records.

| Module | Owns (aggregates) | Package |
|---|---|---|
| **Platform** (vendor) | Organisation, Plan | 1 (minimal) |
| **Org Structure** | Branch, Department, Designation, Category, EmployeeType, Contractor, PayrollGroup, OrgSettings | 1 |
| **People** | Employee, StatusChangeRequest, BulkImport, EmployeeCodeSequence | 1 |
| **Identity & Access** | User, Role | 1 (minimal slice) |
| **Audit** | AuditEntry (append-only) | 1 (shared) |
| Attendance | Policy assignments, shifts, punches, periods, locks | 2 — People reads lock status and the resolved policy set through interfaces |

---

## 2. Aggregates

An aggregate is changed as one unit in one transaction. Only the root is referenced from outside.

### 2.1 Organisation (Platform)
- **Root:** Organisation — id, name, short code (platform-unique), plan, employee limit, limit behaviour (block / allow and alert, VC-01), state (Active, Suspended).
- Created by the Vendor Admin with its first Org Admin user (VC-R1). Creating it also creates the default payroll group and template lists (G6, PG-R5).

### 2.2 Branch (Org Structure)
- **Root:** Branch — id, organisation, short code, name, address, time zone, state (Active, Inactive).
- Package 2 adds holiday calendar, geo-fences and devices to this aggregate.

### 2.3 Reference lists (Org Structure)
- **Department, Designation, Category, EmployeeType**: four separate aggregates, each with id, organisation, code, name, state (Active, Inactive). Renames keep their history (AC-OR-02.3).

### 2.4 Contractor (Org Structure)
- **Root:** Contractor — id, organisation, code, name, contact, state.

### 2.5 PayrollGroup (Org Structure)
- **Root:** PayrollGroup — id, organisation, name, pay frequency, cycle definition (calendar month / custom start day / week start), is-default, state.
- V2 adds pay options. Membership is **not** stored here; it lives in each employee's job versions (the invariant is per employee).

### 2.6 OrgSettings (Org Structure)
- **Root:** OrgSettings — one per organisation, versioned (G5). Holds OR-01–04 and EM-01–11 values, including the employee-code pattern, probation length and reminder days, EM-10 and EM-11.
- Every change is a new version with who, when, old, new and reason (G9).

### 2.7 Employee (People) — the core aggregate
- **Root:** Employee — id, organisation, employee code.
- **PersonalDetails** (value object): name, date of birth, gender, photo reference, contact (mobile — mandatory, email, address), emergency contact. Current value only; every change, including the employee's own edits (EM-03, no approval), is audited (BR-EM-17, DQ-06).
- **PortalAccess** (value on the root): none / employee portal, default none (EM-R15). Only "employee portal" has a User account (§2.12). Changes are audited.
- **EmploymentPeriod** (entity, 1..n): joining date, probation end date (extendable by HR, DQ-01), last working day (empty while open), exit reason, exit remarks. At most one open period. A rejoin always adds a new period to the same employee and keeps the code (DQ-07).
- **JobVersion** (entity, 1..n per period): effective-from, effective-to, recorded time, superseded-by, and the job fields: master branch, allowed branches (set, per EM-06), department, designation, category, employee type, contractor, reporting manager (employee id), shift/roster reference (Package 2), policy-set override (optional), payroll group, enrolment ID. The enrolment ID is dated because freed IDs can be reused (BR-EM-24), so a punch is always matched to whoever held that ID on the punch date.
- **EnrolmentStatus** (value on the root): not enrolled / enrolled / re-enrolment needed (PC-R8). Current value plus audit.
- **StatusPeriod** (entity, 1..n): status, effective-from, effective-to, approved request id.
- **CustomFieldValues** (value object): values keyed by custom field definition.

**Why one job-version timeline:** answering "as on date" (AC-EM-11.2) is a single lookup, and every change is one new version. Unchanged fields are copied forward.

**How a back-dated change works without editing history (BR-EM-05).** Versions are never edited. A change effective on a past date D:
1. Saves new versions from D onward carrying the new value.
2. Marks the versions it replaces as *superseded* (recorded time kept).

So the employee "as on 10 November" can be shown both as now known and as known on any earlier day. Example: on 20 November HR backdates a department change to 1 November. The version for 1–19 November is superseded by a new one with the new department. The old version remains, marked superseded on 20 November.

### 2.8 StatusChangeRequest (People)
- **Root:** StatusChangeRequest — id, employee, from status, to status, effective date, reason, raised by, approver role, state (see SM-2), decision by, decision reason, exit details (last working day, reason, remarks) when to = Exited.
- A separate aggregate, because it has its own lifecycle and approver. Approval adds a StatusPeriod to the Employee in the same transaction.

### 2.9 (removed) ProfileChangeRequest
Not needed: employees' own contact edits apply at once (EM-03, DQ-05).

### 2.10 BulkImport (People)
- **Root:** BulkImport — id, file reference and checksum, uploaded by, state (SM-7), row outcomes (row, employee code, applied / error + reason).

### 2.11 EmployeeCodeSequence (People)
- **Root:** one per (organisation, pattern key — e.g. branch short code). Next number. Only ever increases (BR-EM-01).

### 2.12 User (Identity & Access)
- **Root:** User — id, organisation, kind (admin / employee), employee id (for employee users), email, role assignments with access scope (AC-07), state.
- **Credential** (entity): PIN or password hash, temporary-PIN expiry, failed-attempt count, locked-until, state (SM-6).
- **RegisteredDevice** (entity): app device id, first seen, last seen (DEC-20).
- **Session** (entity): refresh token id, issued, revoked.

### 2.13 Role (Identity & Access)
- **Root:** Role — id, organisation (empty for built-in templates), name, built-in flag, permissions (set of operation codes, see PERMISSION_MATRIX.md).

### 2.14 AuditEntry (Audit)
- Append-only: organisation, actor, action, target type and id, time, old values, new values, reason, correlation id. Never updated or deleted.

---

## 3. Relationships

```
Organisation 1──* Branch, Department, Designation, Category, EmployeeType, Contractor, PayrollGroup
Organisation 1──1 OrgSettings (versioned)
Organisation 1──* Employee 1──* EmploymentPeriod 1──* JobVersion
                       Employee 1──* StatusPeriod
                       JobVersion *──1 Branch (master), *──* Branch (allowed)
                       JobVersion *──1 Department, Designation, Category, EmployeeType, PayrollGroup
                       JobVersion *──0..1 Contractor, *──0..1 Employee (reporting manager)
Employee 1──* StatusChangeRequest
Employee 1──0..1 User (employee kind)
User *──* Role (with access scope)
```

---

## 4. Invariants (enforced on the server; DB where noted)

| ID | Invariant | Rule | Where |
|---|---|---|---|
| INV-01 | Employee code unique in the organisation, never reassigned to another person | BR-EM-01 | DB |
| INV-02 | Within an open employment period, non-superseded job versions are contiguous from the joining date, with no overlap: exactly one master branch and one payroll group on every date | BR-EM-02, 04 | DB (exclusion) + API |
| INV-03 | Master branch not in its own allowed set; allowed set obeys EM-06 | BR-EM-03 | API |
| INV-04 | At most one open employment period per employee | BR-EM-21 | DB |
| INV-05 | Non-overlapping status periods; at most one pending StatusChangeRequest per employee | BR-EM-07 | DB |
| INV-06 | Approver ≠ raiser, for every request | BR-EM-16, 27 | API |
| INV-07 | Mobile number present, and unique among current employees of the organisation | BR-EM-09 | DB (not null + partial unique) |
| INV-16 | An employee user account exists only while the employee has portal access | BR-EM-32 | API |
| INV-08 | Enrolment ID unique among current employees of the organisation | BR-EM-24 | DB (partial unique) |
| INV-09 | Reporting manager is a current employee of the same organisation, not self, no loop | BR-EM-11 | API |
| INV-10 | No change effective inside a locked period (asks Attendance for lock status) | BR-EM-06 | API |
| INV-11 | Always-mandatory fields present | BR-EM-13 | API |
| INV-12 | Code sequences only increase | BR-EM-01 | DB |
| INV-13 | Org always has ≥ 1 user holding Org Admin; no one grants rights beyond their own scope | BR-EM-29 | API |
| INV-14 | Every pay- or access-affecting write has its audit entry in the same transaction | BR-EM-17 | API + DB |
| INV-15 | Every record carries an organisation; the organisation comes from the signed-in token, never the request | TB-01, TSD §4.1 | DB (RLS) + API |

---

## 5. Domain services

| Service | Does | Notes |
|---|---|---|
| EmployeeCodeGenerator | Builds the next code from the OrgSettings pattern and the sequence. Skips codes already taken (edge case 16). | Locks the sequence row. |
| ReportingChain | Checks for loops; lists everyone below a manager (EM-R14). | |
| AccessScopeResolver | Decides whether a user may see or edit an employee on today's date (BR-EM-18, EM-10). | Used by every People query. |
| EmployeeLimitChecker | Counts employees in counted statuses; applies VC-01 (BR-EM-12). | |
| StatusApprovalRouter | Picks the approver role: Org Admin, or HR Manager if the Org Admin raised it (BR-EM-27). | |
| JobTimeline | Applies a dated change: inserts versions, supersedes old ones, cancels scheduled versions made invalid (edge case 11). | Pure logic, unit-tested. |
| PolicySetResolver | Returns the policy set in force and its source (EM-R3). | Owned by Attendance (Package 2). Until then it returns "not configured". |
| PeriodLockQuery | Says whether a date is in a locked period for an employee's payroll group and branch. | Owned by Attendance. Until then it returns "open". |
| BulkImportProcessor | Validates each row with the same rules as the screen and applies valid rows idempotently (keyed by file checksum + row). | Runs as a background task. |

---

## 6. Effective dating and history

- Effective dates are calendar dates in the employee's master-branch time zone (BR-11).
- Job versions and status periods carry effective range + recorded time; corrections supersede, never overwrite (§2.7).
- OrgSettings are versioned with a start date (G5). A setting change applies from its start date.
- Reference-list renames are versioned so history shows the old name.
- Personal/contact details hold the current value only; their history is the audit log (DQ-06).

---

## 7. Events (for the outbox, CON-02 / DEC-03)

Published when the transaction commits. Consumers are in Package 2 onward.

| Event | When | Consumer (later) |
|---|---|---|
| EmployeeCreated | Employee saved | Attendance (start calculating from joining date) |
| JobChanged (employee, from date) | New or superseded job versions | Attendance recalculates days from that date |
| StatusChanged (employee, status, from date) | Status period added | Attendance; Identity & Access |
| EmployeeExited (employee, last working day) | Exit takes effect | Identity & Access revokes sessions (BR-EM-08) |
| EnrolmentChanged | Enrolment ID mapped or changed | Punch module reprocesses unmapped punches (DEC-44) |

---

## 8. Tenant ownership and audit

- Every aggregate except Plan and built-in Role templates carries organisation id. Row-level security applies to all of them (TSD §4.1).
- The vendor reads only Organisation and Plan, plus usage counts produced for the vendor console (CL-11). Employee data needs approved support access (VC-R6).
- Audited actions: every create and change in §2, approvals and rejections, PIN issue/reset, lockouts, role and permission changes, settings changes, bulk imports, exports.

---

## 9. Cross-domain dependencies

| Needs | From | Until Package 2 exists |
|---|---|---|
| Lock status of a date | Attendance (PeriodLockQuery) | Always "open" |
| Resolved policy set | Attendance (PolicySetResolver) | "Not configured" |
| Shift/roster reference | Attendance | Field optional, empty |
| Email sending | Notifications (ZeptoMail) | Needed in Package 1 for temporary PINs and reminders |

---

## 10. Gate 2 questions — answers (22 November 2026)

| ID | Question | Answer | Applied in |
|---|---|---|---|
| DQ-01 | Probation extension | HR changes the end date with a reason, no approval | EM-04, BR-EM-31, AC-EM-12.4 |
| DQ-02 | Extra status moves | Long leave → Notice and Absconding → Notice allowed; Notice → Long leave blocked | EM-R5, SM-1, BR-EM-30 |
| DQ-03 | "No mobile" employees: web sign-in? | Mobile number is mandatory; "no mobile" removed | EM-R12, BR-EM-09, INV-07 |
| DQ-09 | "Employee profile" meaning | An employee record can exist with no sign-in at all. Portal access is per employee, default none | EM-R15, BR-EM-32, INV-16, SM-6 |
| DQ-04 | Cancelling an approved future status change | Org Admin or HR Manager, with a reason | EM-R5, SM-2, AC-EM-09.8 |
| DQ-05 | Profile change approval | No approval; edits apply at once and are audited | EM-03, BR-EM-23, AC-EM-17.1 |
| DQ-06 | Personal details: current value + audit | Yes | §2.7, §6 |
| DQ-07 | Rejoin model | Old code only: new employment period on the same record; no new code, no link | EM-05, EM-R13, BR-EM-21 |
| DQ-08 | Long leave and attendance | Decide in Package 2/3 | Package 2 |

## 11. Still open for Gate 2

None.

<!-- answered 22 November 2026; kept for reference -->
| ID | Question (answered) |
|---|---|
| DQ-09 | "3 minimum employee profiles; one is admin, another 2 may be anything." Which does this mean? (a) A new organisation can't go live until it has at least 3 user accounts, one of them an Org Admin. (b) Every organisation must always keep at least 3 users who can sign in to admin screens, one being an Org Admin. (c) Something else. |
