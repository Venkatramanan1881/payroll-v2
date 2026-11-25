# User Stories and Acceptance Criteria — Package 1: Employee Master

Phase 0 artefact (SKILL.md §4). Revision 3 · 22 November 2026 · Status: **Approved at Gate 1 on 22 November 2026**, with proposed values EMQ-24 to EMQ-32 accepted.
Revision 4 (BRD v0.12): mobile number mandatory; portal access per employee, default none (AC-EM-01.3, AC-EM-19.0).
Revision 3 (Gate 2 answers, BRD v0.11): rejoin always keeps the old code, with no rejoin link (US-EM-14); contact edits need no approval (US-EM-17); approved future status changes can be cancelled (AC-EM-09.8); probation can be extended (AC-EM-12.4).
Sources: BRD v0.10, TSD v0.4, decisions DEC-01–45, answers to EMQ-01–23, CL-13 and ACC-01 (22 November 2026).
Revision 2 changes: every EMQ answer applied; all [Q] tags resolved; new story US-AC-01 (role permissions); employee code format, custom field types, probation reminder and allowed-branch visibility are now settings.

Acceptance criteria (AC) are written Given / When / Then. Rules BR-EM-nn / BR-OR-nn are in `../BUSINESS_RULES.md`.
Tags: **[C]** confirmed (BRD v0.10 or a decision) · **[P]** proposed, needs approval at Gate 1.

## Actors

| Actor | In this package | Source |
|---|---|---|
| Vendor Admin | Creates the organisation (name, short code, plan, employee limit, first Org Admin) | VC-R1, VC-01 |
| Organisation Admin | Manages users; edits role permissions and assigns users to roles; approves employee status changes | §5, AC-01, EM-R5 |
| HR Manager | Owns org structure, employee records, settings, bulk upload; edits role permissions and assigns users to roles; may be limited to branches | §5, AC-01, AC-07 |
| Manager | Views their whole reporting chain | EM-R14 |
| Employee | Views own profile; edits contact details (no approval, EM-03); signs in with employee code + PIN | AC-05, EM-03, UM-R3 |

Not in this package: Payroll Officer (V2), Supervisor (V3). There is no Auditor role (DEC-29); read-only access is a custom role.

---

## A. Organisation structure

### US-OR-01 Manage branches [C]
As an **HR Manager**, I want to create and maintain branches, so that employees can be placed in them.
Source: OR-R1, OR-R5, BR-11.

- **AC-OR-01.1** Given I am an HR Manager, when I create a branch with name, short code, address and time zone, then it is saved and listed. The time zone defaults to Asia/Kolkata.
- **AC-OR-01.2** Given a branch short code already used in my organisation, when I save another branch with it, then the save is refused with "Branch short code already exists" (BR-OR-01).
- **AC-OR-01.3** Given a branch that has ever had an employee, when I try to delete it, then I can only deactivate it (BR-OR-02).
- **AC-OR-01.4** Given a deactivated branch, when an employee is created or transferred, then that branch is not offered.
- **AC-OR-01.5** Given a branch whose short code is already used in employee codes, when HR changes the short code, then existing employee codes do not change. Only new codes use the new short code. **[P]**
- Holiday calendar, geo-fences and devices are attached to the branch in Package 2.

### US-OR-02 Manage departments, designations, categories and employee types [C]
As an **HR Manager**, I want to maintain these lists, so that they match my organisation and sector.
Source: OR-R2, OR-R3, OR-R5, G6.

- **AC-OR-02.1** Given a new organisation created from a sector template, when HR first opens these lists, then they contain the template values. The default employee types are permanent, trainee, casual and contract. "Probation" is not a type (OR-R3).
- **AC-OR-02.2** Given any list, when HR adds, renames or deactivates an entry, then the change is saved and audited (BR-EM-17). Entries in use are deactivated, never deleted (BR-OR-02).
- **AC-OR-02.3** Given a renamed entry, when I look at an employee's history, then past versions show the entry as it was named at that time. **[P]**

### US-OR-03 Manage contractors [C]
As an **HR Manager**, I want to record contractors, so that contract workers can be linked to the contractor who supplies them.
Source: OR-R4, CW-R1, EM-R2, EM-11.

- **AC-OR-03.1** Given a contractor with name, code and contact, when saved, then it can be chosen on an employee whose type is contract.
- **AC-OR-03.2** Given a contractor, when I open it, then I see the list of workers currently linked to it.
- **AC-OR-03.3** Given EM-11 = no (default), when a contract employee is saved without a contractor, then it is accepted (directly hired). Given EM-11 = yes, the save is refused until a contractor is chosen (BR-EM-26).

### US-OR-04 Organisation settings [C]
As an **HR Manager**, I want to set the financial year, leave year, week start, languages and currency, so that dates and lists behave correctly.
Source: OR-01–04, DEC-10.

- **AC-OR-04.1** Defaults: financial and leave year April–March; week starts Monday; English on; Indian rupee.
- **AC-OR-04.2** Every change records who, when, old value, new value and reason (G9).

### US-OR-05 Payroll groups (V1: name, period, membership) [C]
As an **HR Manager**, I want payroll groups with a pay period, so that attendance periods, counters and locks are defined from V1.
Source: PG-R1–R3, PG-R5, PL-01, DEC-06.

- **AC-OR-05.1** Given a new organisation, then a group "All employees, monthly" (calendar month) already exists and can be renamed (PG-R5).
- **AC-OR-05.2** Given I create a group with a name and pay frequency (daily / weekly / bi-weekly / monthly — calendar month or custom cycle such as 26th–25th), then it is saved.
- **AC-OR-05.3** Given a group, when I add employees singly, in bulk, or by rule (branch, department, category, employee type), then each employee ends up in exactly one group (BR-EM-04).
- **AC-OR-05.4** No pay rules, components or pay settings are shown in V1.
- **AC-OR-05.5** Given a group with members, when HR tries to delete it, then the delete is refused until it has no members on any date. **[P]**

### US-AC-01 Role permissions and role assignment [C]
As an **Organisation Admin** or **HR Manager**, I want to edit what each role can do and assign users to roles, so that access fits our organisation.
Source: AC-01, AC-06, AC-07, CL-13, ACC-01.

- **AC-AC-01.1** Given I am an Org Admin or HR Manager, when I change a role's view / edit / approve rights, then the change applies to everyone in that role from their next request, and is audited.
- **AC-AC-01.2** Each built-in role starts with a default permission matrix. The matrix itself is produced in Phase 1 (Domain gate).
- **AC-AC-01.3** Given AC-06 = on (default), then custom roles can be created from a copy of any role.
- **AC-AC-01.4** Given I assign a user to a role, I also set their access scope (all branches / chosen branches / chosen departments, AC-07).
- **AC-AC-01.5** An Org Admin or HR Manager cannot remove the last user holding the Org Admin role, and cannot give themself rights outside their own scope. **[P]**

---

## B. Employee record

### US-EM-01 Create an employee [C]
As an **HR Manager**, I want to create an employee with personal, contact, emergency and job details, so that the employee exists for attendance and later pay.
Source: EM-R1, EM-R2, EM-R11, EM-R12, EM-02.

- **AC-EM-01.1** Given all mandatory fields are filled (always-mandatory list plus org-chosen, BR-EM-13), when I save, then the employee is created with status Active or Probation and an employee code (US-EM-02).
- **AC-EM-01.2** Given a mandatory field is empty, when I save, then the save is refused and each missing field is named.
- **AC-EM-01.3** Mobile number is mandatory for every employee (EM-R12). A new employee is created with portal access "none" unless HR chooses otherwise (EM-R15).
- **AC-EM-01.4** Given a mobile number held by another current employee of my organisation, when I save, then the save is refused, naming that employee's code (BR-EM-09).
- **AC-EM-01.5** No identity numbers (Aadhaar, PAN or others) and no pay or bank fields appear in V1 (EM-R11, EM-R4).
- **AC-EM-01.6** Given a joining date, then no attendance result is produced for dates before it (BR-EM-22). Future joining dates are allowed.
- **AC-EM-01.7** Given the organisation is at its employee limit, then VC-01 applies (US-EM-18).
- **AC-EM-01.8** Given the HR Manager is limited to branches, then only those branches are offered as master branch (BR-EM-18).
- **AC-EM-01.9** The creation is audited (BR-EM-17).

### US-EM-02 Employee code [C]
As an **HR Manager**, I want employee codes generated by a pattern, or entered by hand, so that every employee has a unique code.
Source: EM-01, VC-R1, OR-R1.

- **AC-EM-02.1** Given EM-01 = automatic (default), when an employee is created at branch CHN in organisation PBY, then the code is organisation short code + branch short code + a 4-digit running number for that branch: PBYCHN0001, then PBYCHN0002.
- **AC-EM-02.2** Given EM-01 = custom pattern, when HR chooses the parts (organisation short code, branch short code, running number, fixed text), their order, a separator, the number of digits and the starting number, then new codes follow the pattern. A preview shows the next code before saving. Existing codes don't change.
- **AC-EM-02.3** Given EM-01 = manual, when I enter a code used by another person in my organisation, then the save is refused (BR-EM-01).
- **AC-EM-02.4** Running numbers are never reused, even after an exit or a failed save.
- **AC-EM-02.5** Given an employee transfers to another branch, then the code does not change.
- **AC-EM-02.6** Given the running number would pass the digit count (after 9999), then one more digit is used (PBYCHN10000), and HR is warned when 90% of the range has been used. **[P]**
- **AC-EM-02.7** The same code may exist in another organisation.

### US-EM-03 Mandatory fields [C]
As an **HR Manager**, I want to choose which fields are mandatory, so that we capture what our organisation needs.
Source: EM-02.

- **AC-EM-03.1** Given the field list, when HR marks fields mandatory, then creates and edits enforce them, including bulk upload.
- **AC-EM-03.2** These are always mandatory and cannot be made optional: name, employee code, mobile number, master branch, joining date, employee type, category, department, payroll group, status (BR-EM-13).
- **AC-EM-03.3** Given a field made mandatory after employees exist, then existing records are not blocked but are listed as "incomplete" for HR. **[P]**

### US-EM-04 Custom fields [C]
As an **HR Manager**, I want to add our own fields to the employee record, so that we can hold information the standard form lacks.
Source: EM-R8.

- **AC-EM-04.1** Given I add a custom field with a name, type and mandatory flag, then it appears on the employee form, in bulk upload and in R10. Types: text, number, date, yes/no, dropdown (one choice), multiple choice.
- **AC-EM-04.2** For dropdown and multiple choice, HR maintains the options. An option in use can be deactivated, not deleted.
- **AC-EM-04.3** Custom fields are not used by any attendance calculation in V1 (BR-EM-15).
- **AC-EM-04.4** Given a custom field with data, when deleted, then it is hidden and its values are kept. **[P]**

### US-EM-05 Master and allowed branches [C]
As an **HR Manager**, I want to set an employee's master branch and the other branches they may punch at, so that multi-branch staff are handled.
Source: EM-R9, EM-06, EM-07, EM-08, AT16.

- **AC-EM-05.1** Every employee has exactly one master branch on any date (BR-EM-02).
- **AC-EM-05.2** Given EM-06 = master branch only (default), then the allowed-branch field is hidden.
- **AC-EM-05.3** Given EM-06 = chosen branches, when I add allowed branches, then the master branch cannot also be listed and only active branches are offered.
- **AC-EM-05.4** Given EM-06 = all branches, then every active branch is allowed, including ones added later. **[P]**
- **AC-EM-05.5** EM-07 (punch at a branch not allowed) and EM-08 (costing) are stored here and applied in Package 2.

### US-EM-06 Reporting manager [C]
As an **HR Manager**, I want to set each employee's reporting manager, so that requests reach the right approver.
Source: EM-R2, AC-02.

- **AC-EM-06.1** The reporting manager must be a current employee of the same organisation (BR-EM-11).
- **AC-EM-06.2** An employee cannot be their own manager, and no chain may loop (A → B → C → A) (BR-EM-11).
- **AC-EM-06.3** Given a manager's exit is approved, then HR sees a list of reportees still pointing to them, to reassign. **[P]**

### US-EM-07 Attendance details on the employee [C]
As an **HR Manager**, I want to record the device enrolment ID, shift or roster and policy set, so that attendance can be calculated.
Source: EM-R3, PC-R3, PC-R8, G3, BR-05.

- **AC-EM-07.1** An enrolment ID belongs to one current employee in the organisation. A duplicate is refused, naming the holder (BR-EM-24).
- **AC-EM-07.2** Each employee shows an enrolment status: not enrolled / enrolled / re-enrolment needed (PC-R8). New employees start "not enrolled".
- **AC-EM-07.3** The policy set field shows the set in force today and where it comes from (organisation, branch, department, category or shift). HR may set an employee-level policy set, which then wins (BR-05).
- **AC-EM-07.4** Shift/roster assignment and its behaviour are delivered in Package 2. In Package 1 the field exists and is optional.

### US-EM-08 Payroll group membership [C]
As an **HR Manager**, I want each employee in one payroll group, so that their period is defined.
Source: PG-R3, PG-R5, DEC-06.

- **AC-EM-08.1** On every date, an employee is in exactly one payroll group (BR-EM-04).
- **AC-EM-08.2** A move between groups is dated. The old membership ends the day before the new one starts.
- **AC-EM-08.3** A move into a date that falls in a locked period is refused unless that period is first unlocked (BR-EM-06).

---

## C. Lifecycle and history

### US-EM-09 Change employment status with approval [C]
As an **HR Manager**, I want to change an employee's status with an approval, so that status changes are controlled.
Source: EM-R5, BR-07, DEC-35.

- **AC-EM-09.1** Statuses: Active, Probation, Notice, Long leave, Absconding, Exited.
- **AC-EM-09.2** Given HR raises a change with an effective date and reason, then the change is "pending" and the current status is unchanged until it is approved.
- **AC-EM-09.3** Given a change is already pending for that employee, then a second one is refused (BR-EM-07).
- **AC-EM-09.4** The Organisation Admin approves. If the Organisation Admin raised the change, an HR Manager approves. Nobody approves their own request (BR-EM-27).
- **AC-EM-09.5** Given approval, then the new status takes effect from the effective date (past in an open period, today, or future) and is recorded in history.
- **AC-EM-09.6** Given rejection, then nothing changes and the reason is recorded.
- **AC-EM-09.7** Absconding may be flagged by the attendance module (LM-29, DEC-37). It becomes the employee's status only after this approval.
- **AC-EM-09.8** Given an approved change with a future effective date, when the Organisation Admin or an HR Manager cancels it with a reason before that date, then it never takes effect, and the cancellation is audited.
- **AC-EM-09.9** Only the moves listed in EM-R5 are offered. Any other move (e.g. Notice → Long leave) is refused.

### US-EM-10 Dated changes and transfers [C]
As an **HR Manager**, I want job changes recorded with an effective date, so that past, present and future dates use the right values.
Source: EM-R6, EM-09, DEC-26, G5.

- **AC-EM-10.1** Changes to master branch, allowed branches, department, designation, category, employee type, reporting manager, shift/roster, policy set, payroll group and status are saved as dated versions. Earlier versions are never overwritten (BR-EM-05).
- **AC-EM-10.2** Given a future effective date, then the change is scheduled, the current values stay until that date, and HR can cancel the scheduled change before it starts.
- **AC-EM-10.3** Given a past effective date in an open period, then it is accepted and the affected days are recalculated (BR-03). In a locked period, it is refused unless the period is unlocked (BR-EM-06).
- **AC-EM-10.4** Given a change of master branch, then HR is prompted to review shift, policy set and payroll group at the same time (EM-09).
- **AC-EM-10.5** Given a transfer effective 16 November, then days up to 15 November use the old branch (including its holiday calendar) and days from 16 November use the new one (DEC-26, BR-EM-19).

### US-EM-11 View history [C]
As an **HR Manager**, I want to see an employee's full dated history, so that I can explain any past value.
Source: EM-R6, §16.

- **AC-EM-11.1** History lists every version with effective date, changed fields (old → new), who, when and why.
- **AC-EM-11.2** Given a date, when I ask "as on date", then I see the employee exactly as they were on that date.
- **AC-EM-11.3** Scheduled (future) changes are shown separately as "scheduled".

### US-EM-12 Probation confirmation reminder [C]
As an **HR Manager**, I want a reminder before an employee's probation ends, so that confirmation isn't missed.
Source: EM-04.

- **AC-EM-12.1** Given an employee in status Probation, when probation end (joining date + probation length, default 6 months) is N days away (EM-04, default 15, changeable in settings), then HR is notified.
- **AC-EM-12.2** Confirmation is a status change (US-EM-09) from Probation to Active.
- **AC-EM-12.3** Given HR changes N, then reminders not yet sent use the new N.
- **AC-EM-12.4** Given an employee in Probation, when HR sets a later probation end date with a reason, then the end date changes with no approval, the reminder moves with it, and the change is audited (EM-04).

### US-EM-13 Exit an employee (V1 record) [C]
As an **HR Manager**, I want to record an employee's exit, so that they stop appearing as current and lose access.
Source: EM-R5, ACC-09.

- **AC-EM-13.1** Exit is a status change to Exited (US-EM-09). It records the last working day, a reason from an editable list (resigned, terminated, absconded, retired, contract ended, other) and remarks.
- **AC-EM-13.2** From the day after the last working day, the employee cannot sign in, and every session and refresh token is revoked (BR-EM-08).
- **AC-EM-13.3** Punches after the last working day are stored but not counted (BR-01, BR-EM-22).
- **AC-EM-13.4** On exit, the employee's mobile number and enrolment ID are freed for reuse (BR-EM-09, BR-EM-24).
- **AC-EM-13.5** Exited employees stay in history and reports, and no longer count toward the employee limit (BR-EM-12).

### US-EM-14 Rejoin [C]
As an **HR Manager**, I want to rejoin a former employee, so that their history stays on one record.
Source: EM-05, EM-R13.

- **AC-EM-14.1** Given an exited employee, when HR rejoins them with a new joining date, then they keep their old employee code, and a new employment period starts on the same record.
- **AC-EM-14.2** Given the employee's old mobile number or enrolment ID now belongs to someone else, then HR must enter new ones before the rejoin is saved.
- **AC-EM-14.3** The rejoin is treated like a new joiner for the employee limit, mandatory fields and status (Probation or Active).
- **AC-EM-14.4** The earlier employment period's history is not changed (BR-EM-21).

---

## D. Bulk and self-service

### US-EM-15 Bulk create and update by spreadsheet [C]
As an **HR Manager**, I want to upload employees by spreadsheet, so that I can load many at once.
Source: EM-R7.

- **AC-EM-15.1** HR downloads a template containing the standard fields, the org's mandatory flags and custom fields.
- **AC-EM-15.2** Every row is validated with the same rules as the screen.
- **AC-EM-15.3** Valid rows are applied. Invalid rows are not applied and are listed in an error report with row number, field and reason (BR-EM-20).
- **AC-EM-15.4** An update row (existing employee code) must carry an effective date for dated fields.
- **AC-EM-15.5** Uploading the same file twice creates no duplicate employees.
- **AC-EM-15.6** A file of 2,000 rows is validated and applied within 2 minutes. **[P]**

### US-EM-16 Employee views own profile [C]
As an **Employee**, I want to see my profile in the app and on the web, so that I know what HR holds about me.
Source: AC-05, N6.

- **AC-EM-16.1** The employee sees personal, contact, emergency and job details.
- **AC-EM-16.2** The employee cannot see another employee's profile.

### US-EM-17 Employee edits own contact details [C]
As an **Employee**, I want to update my contact and emergency details, so that HR has them right.
Source: EM-03.

- **AC-EM-17.1** Given EM-03 = on (default), then my edit applies at once, without HR approval, and is audited (BR-EM-23).
- **AC-EM-17.2** HR can see every self-service change in the employee's history (who, when, old → new).
- **AC-EM-17.3** Given EM-03 = off, then the fields are read-only for me.
- **AC-EM-17.4** A change of mobile number is checked against the one-mobile-one-identity rule (BR-EM-09).

---

## E. Access and limits

### US-EM-18 Employee limit [C]
As a **Vendor Admin**, I want the plan's employee limit enforced, so that organisations stay within their plan.
Source: VC-01, MD-02, DEC-35.

- **AC-EM-18.1** Active, Probation, Notice, Long leave and Absconding employees count. Exited employees don't (BR-EM-12).
- **AC-EM-18.2** Given VC-01 = allow and alert (default), when the limit is passed, then the employee is created and the vendor is alerted.
- **AC-EM-18.3** Given VC-01 = block, then the create (screen or bulk) is refused, naming the limit.

### US-EM-19 Employee sign-in account [C]
As an **HR Manager**, I want to give an employee app/web access, so that they can punch and see their records.
Source: UM-R3, EM-R12, MD-02, DEC-20, DEC-40.

- **AC-EM-19.0** Given an employee with portal access "none" (default), then no sign-in details are issued, no user account exists, and any sign-in attempt with their code is refused. HR keeps managing their record (EM-R15).
- **AC-EM-19.0a** Given HR grants portal access, then a temporary PIN is issued (AC-EM-19.2). Given HR removes it, then the PIN stops working and all sessions end at once.
- **AC-EM-19.1** Given an employee with portal access, then they sign in with organisation short code (remembered by the app after first use), employee code and PIN.
- **AC-EM-19.2** HR issues a temporary PIN. It's shown once on screen, can be printed, is emailed if the employee has an email address, and expires after 7 days.
- **AC-EM-19.3** On first sign-in, the employee must replace the temporary PIN with their own.
- **AC-EM-19.4** HR can reset a PIN. The old PIN stops working, all sessions are revoked, and the reset is audited.
- **AC-EM-19.5** Where the employee has an email address, they may sign in with a one-time code sent by email (ZeptoMail).
- **AC-EM-19.6** After 5 wrong PINs or codes, the account locks for 15 minutes. **[P]**
- **AC-EM-19.7** A sign-in from a new phone registers that phone and is shown to HR (device binding, DEC-20).
- **AC-EM-19.8** If the org's plan has no mobile app (MD-02), portal access means web only.
- **AC-EM-19.9** PINs are stored only as a hash (BR-EM-10).

### US-EM-20 Employee list, search and scope [C]
As an **HR Manager** or **Manager**, I want to search the employee list within my access scope, so that I find people quickly and see no one else.
Source: UM-R2, AC-07, EM-10, EM-R14, AT13, AT20.

- **AC-EM-20.1** The list filters by branch, department, category, type, status, payroll group and contractor, searches by name, code and mobile, and pages on the server.
- **AC-EM-20.2** An HR Manager limited to branches sees and edits employees whose master branch is in scope today (BR-EM-18).
- **AC-EM-20.3** For employees whose *allowed* branch is in that HR Manager's scope, EM-10 decides: not visible / view only (default) / view and edit.
- **AC-EM-20.4** A Manager sees their whole reporting chain below them, read-only (EM-R14).
- **AC-EM-20.5** No request, export or file link can return an employee of another organisation (AT20, TB-01).

### US-EM-21 Data protection and audit [C]
As the **organisation**, we want employee data protected and every change traceable.
Source: TSD §15, EM-R11, G9, §16, N9.

- **AC-EM-21.1** V1 holds no identity numbers and no bank details (EM-R11, EM-R4). When bank details arrive in V2, they are encrypted per field and masked (BR-EM-14).
- **AC-EM-21.2** Every create, change, approval and status change records actor, organisation, time, old and new values, and reason where required (BR-EM-17).
- **AC-EM-21.3** Passwords, PINs, one-time codes and tokens are never written to logs.

### US-EM-22 Reports [C]
As an **HR Manager**, I want employee reports, so that I can see headcount and movement.
Source: R10, R11, §14.

- **AC-EM-22.1** R10: employee list and headcount by branch, department, category and type, as on a chosen date. (Days worked by branch arrives with Package 2.)
- **AC-EM-22.2** R11: joiners and leavers for a period. (Onboarding/offboarding status is V3.)
- **AC-EM-22.3** Reports respect the user's scope and export to spreadsheet. Large exports run in the background (TSD §7).

---

## Edge and negative cases (to become tests)

1. Transfer effective on the same day as joining.
2. Two pending status changes for one employee → the second is refused.
3. A status change back-dated into a locked period → refused until unlocked.
4. A reporting-manager change that would create a loop three levels deep.
5. Bulk upload containing the same new employee twice in one file.
6. Bulk upload update for an employee outside the uploader's scope → row refused.
7. Rejoin while the earlier record still has a pending status change → refused until it's decided.
8. A mobile number held by an exited employee → allowed (freed on exit).
9. A branch deactivated while employees still have it as an allowed branch.
10. An Org Admin trying to approve a status change they raised → routed to an HR Manager.
11. A future-dated transfer, followed by an exit dated before the transfer → the transfer is cancelled automatically and HR is told.
12. Employee limit reached in the middle of a bulk upload with VC-01 = block.
13. Two HR users saving the same employee at once → the second gets a conflict, not a silent overwrite (BR-EM-25).
14. A custom field renamed after values exist.
15. A joining date changed after attendance already exists for earlier days.
16. A custom code pattern that would create a code already used → the next free number is used.
17. A branch short code changed → existing codes unchanged, new codes use the new short code.
18. A temporary PIN used after 7 days → refused; HR must reissue.
19. The last Org Admin's role removed → refused.
