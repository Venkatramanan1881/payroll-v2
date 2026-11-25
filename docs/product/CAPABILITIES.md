# Product Capability Map — Build Order

Phase 0 artefact (SKILL.md §4). Revision 3 (24 November 2026: Package 2 section updated). Sources: BRD v0.13, TSD v0.7, `../Payroll-App-Issue-Resolutions-v0.2.md` (DEC-01–51).
Status: Draft for Approval Gate 1.

The build goes module by module. Each module passes through every phase (stories → domain → state machines → permissions → DB → API → backend → web → mobile) before the next one starts.

---

## Package 1 — Employee Master (this package)

Includes the organisation structure it cannot work without.

| Cap ID | Capability | BRD source | Release |
|---|---|---|---|
| CAP-OR-1 | Branches (address, time zone, holiday calendar link) | OR-R1 | V1 |
| CAP-OR-2 | Departments, designations, categories, employee types (extendable lists) | OR-R2, OR-R3, OR-R5 | V1 |
| CAP-OR-3 | Contractors and the workers they supply | OR-R4, CW-R1 | V1 |
| CAP-OR-4 | Organisation settings: financial/leave year, week start, languages, currency | OR-01–04 | V1 |
| CAP-OR-5 | Payroll groups: name, pay period, membership (no pay rules) | DEC-06, PG-R1–R3, PL-01 | V1 |
| CAP-EM-1 | Employee record: personal, contact, emergency, job details | EM-R1, EM-R2 | V1 |
| CAP-EM-2 | Employee code (automatic per branch / manual) and rejoin | EM-01, EM-05 | V1 |
| CAP-EM-3 | Mandatory and custom fields | EM-02, EM-R8 | V1 |
| CAP-EM-4 | Master branch and allowed branches | EM-R9, EM-06–09 | V1 |
| CAP-EM-5 | Employment status lifecycle with approval | EM-R5, DEC-35 | V1 |
| CAP-EM-6 | Dated history and transfers | EM-R6, EM-09, DEC-26 | V1 |
| CAP-EM-7 | Bulk create/update by spreadsheet | EM-R7 | V1 |
| CAP-EM-8 | Employee self-service: view profile, edit contact details (no approval) | EM-03 | V1 |
| CAP-EM-9 | Probation confirmation reminder | EM-04 | V1 |
| CAP-EM-10 | Exit (minimal V1 record) and access revocation | EM-R5, ACC-09 | V1 |
| CAP-EM-11 | Employee sign-in account (employee code + PIN; email code optional) | UM-R3, DEC-40, DEC-30 | V1 |
| CAP-EM-12 | Employee limit per plan | VC-01, MD-02, DEC-35 | V1 |
| CAP-EM-13 | Employee list, search and access scope | UM-R2, AC-07, AT13 | V1 |
| CAP-EM-14 | Attendance links on the employee: enrolment ID and status, shift/roster, policy set | EM-R3, PC-R3, PC-R8 | V1 (fields here; behaviour in Package 2) |
| CAP-EM-15 | Reports: employee list and headcount (R10); joiners and leavers (R11) | R10, R11 | V1 |
| CAP-AC-1 | Role permissions and role assignment (Org Admin and HR Manager) | AC-01, AC-06, AC-07, CL-13 | V1 |

**Not in Package 1:** pay details, bank details and CTC (V2 Payroll, EM-R4); identity numbers (EM-R11); onboarding/offboarding checklists and documents (V3); "days worked by branch" (EM-R10, needs Package 2).

**Depends on (minimum slice, built alongside):** organisation creation by the vendor (VC-R1), users, roles and access scope (C03). Only what Employee Master needs is built now.

---

## Package 2 — Attendance (approved at Gate 1, 24 November 2026)

Stories: `USER_STORIES/AT-attendance.md` (US-AT-01–44). Rules: BR-AT-01–23. Questions: ATQ-05–20.

| Order | Sub-module | BRD source |
|---|---|---|
| 2a | Shifts and roster | C05, SH-R1–R4, SH-01–12 |
| 2b | Holidays and weekly offs | C07, HD-R1–R3, HD-01–05 |
| 2c | Punch collection: eSSL gateway, app and web punch, geo-fence, offline, import | C04, PC-R1–R15, PC-01–17 |
| 2d | Attendance rules engine: reading, breaks, late, early, missed punch, day status, rounding | C06 |
| 2e | Regularisation and flagged-punch approval | RG-01–11, DEC-18 |
| 2f | Attendance period and lock (per payroll group × branch) | C12, PL-01–07 |
| 2g | Payroll input export, attendance reports, notifications, dashboard widgets | MD-06, R1–R5, R13, R23, R26, NT-01–10, DB-R3/R5/R7 |

Leave, permissions and on-duty (C08, C09) feed the engine as overlays. The engine leaves a defined input slot for them, and they come as Package 3.

**Blockers before Phase 0:** ATT-08, CL-17 and CL-08 decided 24 November 2026 (DEC-50, DEC-49, DEC-47), with CL-07 (DEC-46), CL-14 (DEC-48) and SEC-07 (DEC-51). **PUN-01 still open:** only the eSSL gateway (US-AT-11) waits for the device test.
