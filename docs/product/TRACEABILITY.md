# Requirements Traceability

SKILL.md §8 chain: requirement → capability → story → acceptance criteria → business rule → domain concept → state transition → DB constraint → API → implementation → test.
Phase 0 fills the first five columns. Later phases add the rest. Revision 3 · 24 November 2026: Package 2 Phase 0 rows added. Revision 2 · sources BRD v0.10, TSD v0.4.

## Package 1 — Employee Master

| BRD / decision | Capability | Story | AC | Rules | Domain → Test |
|---|---|---|---|---|---|
| OR-R1, BR-11 | CAP-OR-1 | US-OR-01 | AC-OR-01.1–5 | BR-OR-01, 02, 03 | Phase 1+ |
| OR-R2, OR-R3, OR-R5, G6 | CAP-OR-2 | US-OR-02 | AC-OR-02.1–3 | BR-OR-01, 02, 04 | |
| OR-R4, CW-R1, EM-11 | CAP-OR-3 | US-OR-03 | AC-OR-03.1–3 | BR-OR-01, BR-EM-26 | |
| OR-01–04, G9 | CAP-OR-4 | US-OR-04 | AC-OR-04.1–2 | BR-OR-04 | |
| PG-R1–R3, PG-R5, PL-01 | CAP-OR-5 | US-OR-05, US-EM-08 | AC-OR-05.1–5, AC-EM-08.1–3 | BR-OR-06, BR-EM-04, 06 | |
| AC-01, AC-06, AC-07, §5 | CAP-EM-13 | US-AC-01 | AC-AC-01.1–5 | BR-EM-29 | |
| VC-R1 | CAP-EM-2 | US-EM-02 | AC-EM-02.1 | BR-OR-05 | |
| EM-R1, EM-R2, EM-R11, EM-R12, EM-02 | CAP-EM-1 | US-EM-01 | AC-EM-01.1–9 | BR-EM-09, 13, 17, 18, 22, 28 | |
| EM-01, EM-05 | CAP-EM-2 | US-EM-02, US-EM-14 | AC-EM-02.1–7, AC-EM-14.1–4 | BR-EM-01, 21 | |
| EM-02, EM-R8 | CAP-EM-3 | US-EM-03, US-EM-04 | AC-EM-03.1–3, AC-EM-04.1–4 | BR-EM-13, 15 | |
| EM-R9, EM-06–08, AT16 | CAP-EM-4 | US-EM-05 | AC-EM-05.1–5 | BR-EM-02, 03 | |
| EM-R2, AC-02 | CAP-EM-1 | US-EM-06 | AC-EM-06.1–3 | BR-EM-11 | |
| EM-R3, PC-R3, PC-R8, G3, BR-05 | CAP-EM-14 | US-EM-07 | AC-EM-07.1–4 | BR-EM-24 | |
| EM-R5, BR-07, DEC-35 | CAP-EM-5 | US-EM-09 | AC-EM-09.1–7 | BR-EM-07, 16, 27 | |
| EM-R6, EM-09, DEC-26, G5 | CAP-EM-6 | US-EM-10, US-EM-11 | AC-EM-10.1–5, AC-EM-11.1–3 | BR-EM-05, 06, 19 | |
| EM-04 | CAP-EM-9 | US-EM-12 | AC-EM-12.1–3 | BR-EM-07 | |
| EM-R5, ACC-09 | CAP-EM-10 | US-EM-13 | AC-EM-13.1–5 | BR-EM-08, 09, 12, 22, 24 | |
| EM-R13 | CAP-EM-2 | US-EM-14 | AC-EM-14.1–4 | BR-EM-21 | |
| EM-R5 (moves, cancel), EM-04 (extension) | CAP-EM-5, CAP-EM-9 | US-EM-09, US-EM-12 | AC-EM-09.8–9, AC-EM-12.4 | BR-EM-30, 31 | |
| EM-R7 | CAP-EM-7 | US-EM-15 | AC-EM-15.1–6 | BR-EM-20 | |
| AC-05, EM-03 | CAP-EM-8 | US-EM-16, US-EM-17 | AC-EM-16.1–2, AC-EM-17.1–4 | BR-EM-09, 23 | |
| VC-01, MD-02, DEC-35 | CAP-EM-12 | US-EM-18 | AC-EM-18.1–3 | BR-EM-12 | |
| UM-R3, EM-R12, EM-R15, DEC-20, DEC-40, DQ-09 | CAP-EM-11 | US-EM-19 | AC-EM-19.0–9 | BR-EM-08, 10, 32 | |
| UM-R2, AC-07, EM-10, EM-R14, AT13, AT20, TB-01 | CAP-EM-13 | US-EM-20 | AC-EM-20.1–5 | BR-EM-18 | |
| TSD §15, EM-R11, G9, §16 | CAP-EM-1 | US-EM-21 | AC-EM-21.1–3 | BR-EM-14, 17, 28 | |
| R10, R11 | CAP-EM-15 | US-EM-22 | AC-EM-22.1–3 | BR-EM-18 | |

### Phase 1–2 links (domain → state machine → permission)

| Story | Aggregate (DOMAIN_MODEL §2) | Invariants | State machine | Operation codes |
|---|---|---|---|---|
| US-OR-01–04 | Branch, reference lists, Contractor, OrgSettings | INV-15 | SM-8 | `org.*` |
| US-OR-05, US-EM-08 | PayrollGroup; Employee.JobVersion | INV-02, 10 | SM-3, SM-8 | `org.payroll_group.manage`, `employee.edit` |
| US-AC-01 | Role, User | INV-13 | — | `access.role.*` |
| US-EM-01–07 | Employee, EmployeeCodeSequence | INV-01, 02, 03, 07, 08, 09, 11, 12 | SM-3 | `employee.create`, `employee.edit`, `employee.custom_fields.manage` |
| US-EM-09, 12, 13 | StatusChangeRequest; Employee.StatusPeriod, EmploymentPeriod | INV-04, 05, 06, 10 | SM-1, SM-2, SM-5 | `employee.status.*` |
| US-EM-10, 11 | Employee.JobVersion | INV-02, 10 | SM-3 | `employee.edit`, `employee.history.view` |
| US-EM-14 | Employee.EmploymentPeriod, RejoinLink | INV-04 | SM-5 | `employee.rejoin` |
| US-EM-15 | BulkImport | as US-EM-01 | SM-7 | `employee.bulk_import` |
| US-EM-16, 17 | Employee.PersonalDetails | INV-07 | — | `employee.view`, `employee.profile.edit_own` |
| US-EM-18 | Organisation | — | SM-9 | `org.limit.set` |
| US-EM-19 | Employee.PortalAccess, User.Credential, RegisteredDevice, Session | INV-16 | SM-6 | `employee.pin.issue` |
| US-EM-20, 22 | (AccessScopeResolver, ReportingChain) | INV-15 | — | `employee.view`, `report.*` |
| US-EM-21 | AuditEntry | INV-14 | — | `audit.view` |

**Package 1 BRD items deliberately not traced (out of package):** EM-R4 pay details (V2), EM-R10 days worked by branch (Package 2), EM-07/EM-08 behaviour (Package 2, stored in Package 1).

## Phase 3 — database (approved at Gate 3, 22 November 2026)

Every DB-enforced rule and invariant is listed against its table in `../database/DATA_MODEL.md` §4 (INV-01..16, BR-OR-01..06, BR-EM-01..32). Migrations: `backend/crates/db/migrations/0001`–`0007`.

## Phase 4 — API (approved at Gate 4, 22 November 2026)

Story → route coverage: `../api/API_CONTRACTS/package1-employee-master.md` §11. Error codes: `../api/ERROR_CATALOG.md`.

## Package 2 — Attendance (approved at Gate 1, 24 November 2026)

| BRD / decision | Sub-module | Story | AC | Rules |
|---|---|---|---|---|
| SH-R1, SH-01–07, SH-11, DEC-22, DEC-23, ATT-05 | 2a | US-AT-01 | AC-AT-01.1–7 | BR-AT-18 |
| SH-R2, SH-R3, SH-09 | 2a | US-AT-02 | AC-AT-02.1–6 | BR-AT-18 |
| SH-08, ATT-04 | 2a | US-AT-03 | AC-AT-03.1–3 | |
| SH-R4 | 2a | US-AT-04 | AC-AT-04.1–2 | |
| SH-12 | 2a | US-AT-05 | AC-AT-05.1–3 | BR-AT-15 |
| HD-R1, SH-10, DEC-26, DEC-38 | 2b | US-AT-06 | AC-AT-06.1–6 | BR-AT-07, 18 |
| HD-R2, SH-09, PC-06 | 2b | US-AT-07 | AC-AT-07.1–3 | |
| HD-R3, HD-01, LV-06 | 2b | US-AT-08 | AC-AT-08.1–4 | BR-AT-15 |
| HD-03–05, §3.12 | 2b | US-AT-09 | AC-AT-09.1–3 | |
| PC-R2, PC-R10, PC-10, DEC-46 | 2c | US-AT-10 | AC-AT-10.1–5 | BR-AT-03 |
| PC-R1, PC-R6, PC-R7, PC-07, PC-08, DEC-21, DEC-44 | 2c | US-AT-11 (blocked, PUN-01) | AC-AT-11.1–7 | BR-AT-01–04 |
| PC-R3, PC-R8, DEC-44, DEC-45 | 2c | US-AT-12 | AC-AT-12.1–4 | BR-AT-04, 05, BR-EM-24 |
| PC-R14, PC-03, PC-16 | 2c | US-AT-13 | AC-AT-13.1–4 | BR-AT-09 |
| PC-R4, R5, R13, R15, PC-02, 04, 14, 17, DEC-04, 18, 19, 20, 51 | 2c | US-AT-14 | AC-AT-14.1–9 | BR-AT-02, 05, 08, 09, 22 |
| PC-R11, PC-R12, PC-05, 12, 13, 15 | 2c | US-AT-15 | AC-AT-15.1–4 | BR-AT-08, 09 |
| DEC-18, CON-06 | 2e | US-AT-16 | AC-AT-16.1–4 | BR-AT-01, 08, 15 |
| PC-01, REL-10 | 2c | US-AT-17 | AC-AT-17.1–4 | BR-AT-02 |
| PC-R9, PC-11, PUN-08 | 2c | US-AT-18 | AC-AT-18.1–3 | BR-AT-01, 23 |
| CON-06, ATQ-01 | 2e | US-AT-19 | AC-AT-19.1–4 | BR-AT-01, 18 |
| E17, EM-07, DEC-50 | 2c | US-AT-20 | AC-AT-20.1–3 | BR-AT-06 |
| BR-05, EM-R3, AC-04, ATT-19 | 2d | US-AT-21 | AC-AT-21.1–6 | BR-AT-14, 18, 21 |
| ATT-09, BR-08, DM-02, DM-04, TEC-02 | 2d | US-AT-22 | AC-AT-22.1–6 | BR-AT-06, 10, BR-EM-22 |
| PC-R7, CON-02, TEC-03 | 2d | US-AT-23 | AC-AT-23.1–4 | BR-AT-10, 18 |
| PR-01–06, DEC-41, ATT-02 | 2d | US-AT-24 | AC-AT-24.1–6 | |
| BK-01–09 | 2d | US-AT-25 | AC-AT-25.1–3 | |
| LA-01–23, DEC-24, PUN-09 | 2d | US-AT-26 | AC-AT-26.1–11 | BR-AT-12, 13, 14 |
| EC-01–17, EA-01–02 | 2d | US-AT-27 | AC-AT-27.1–4 | BR-AT-12, 14, 20 |
| MP-01–05 | 2d | US-AT-28 | AC-AT-28.1–4 | |
| DS-01–09, RD-01–04 | 2d | US-AT-29 | AC-AT-29.1–4 | BR-AT-11, 12 |
| DEC-25 (UA-01–05) | 2d | US-AT-30 | AC-AT-30.1–4 | |
| LA-06–12, EC-05–11, BR-04, BR-10 | 2d | US-AT-31 | AC-AT-31.1–4 | BR-AT-13, 16 |
| RG-01–09, RG-12, REL-09 | 2e | US-AT-32 | AC-AT-32.1–7 | BR-AT-15 |
| RG-10, PUN-08, DEC-34 | 2e | US-AT-33 | AC-AT-33.1–2 | BR-AT-01, 18 |
| LA-17, EC-16 | 2e | US-AT-34 | AC-AT-34.1–2 | BR-AT-23 |
| RG-11 | 2e | US-AT-35 | AC-AT-35.1 | BR-AT-01 |
| PL-R1, PL-01–03 | 2f | US-AT-36 | AC-AT-36.1–2 | |
| PL-R2, R3, PL-04, 05, DEC-49 | 2f | US-AT-37 | AC-AT-37.1–6 | BR-AT-16, 17 |
| PL-R5, PL-06, DEC-07 | 2f | US-AT-38 | AC-AT-38.1–2 | BR-AT-19 |
| PL-R4, PL-07, DEC-34 | 2f | US-AT-39 | AC-AT-39.1–3 | BR-AT-18 |
| MD-06, R13, LV-10, DEC-47 | 2g | US-AT-40 | AC-AT-40.1–3 | BR-AT-20 |
| DB-R7, BR-08 | 2g | US-AT-41 | AC-AT-41.1–2 | |
| NT-01–10 | 2g | US-AT-42 | AC-AT-42.1–2 | |
| R1–R5, R10, R13, R23, R26 | 2g | US-AT-43 | AC-AT-43.1–2 | |
| DB-R3, DB-R5, TEC-04 | 2g | US-AT-44 | AC-AT-44.1–2 | |
| DEC-48 (monthly partitions) | all | Phase 3 database design | — | — |
