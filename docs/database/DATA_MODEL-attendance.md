# Data Model — Package 2: Attendance

Phase 3 artefact (SKILL.md §4). Revision 1 · 24 November 2026 · Status: **Approved at Gate 3 on 24 November 2026** (proposals DBQ-AT-01 to 04 accepted).
Inputs: DOMAIN_MODEL-attendance.md, STATE_MACHINES-attendance.md, PERMISSION_MATRIX.md (approved at Gate 2), BUSINESS_RULES.md (BR-AT), BRD v0.14, TSD v0.8.
Column-level definitions are the migrations `backend/crates/db/migrations/0012`–`0016`. This document explains them; if the two differ, the migrations are wrong. Package 1 conventions (`DATA_MODEL.md` §2–§3, §7, §9) all still apply: `app` schema, RLS policy `tenant_isolation` on every client table, composite `(org_id, id)` foreign keys, `app_api` owns nothing, insert-only history, column-level UPDATE grants, `row_version` for optimistic locking.

---

## 1. Summary

- 36 new tables (plus monthly partitions): 11 schedules, 8 punch collection, 5 rules, 11 regularisation and periods, and the shared import pair counted under schedules.
- Two tables are **partitioned by month from day one** (DEC-48): `raw_punches` (by effective punch time) and `day_results` (by attendance date). Partitions for 2026-01 to 2028-12 are created by the migration, plus a DEFAULT partition as a safety net. `punch_keys` is hash-partitioned (16).
- Two new columns on `employee_job_versions` (the ones Package 1 reserved): `default_shift_id`, `policy_set_id`; and `job_version_allowed_shifts` (DQ-AT-03).
- 41 new permission codes; every organisation gets a default policy set (new ones by trigger, existing ones by the migration).
- All migrations are additive. Nothing in Package 1 is changed except the two nullable columns.

## 2. Tables

DB = enforced by the database. API = enforced by the Rust API (with tests).

### 2.1 Schedules (0012)

| Table | Purpose | Key rules |
|---|---|---|
| `shifts` | Shift identity: code, name, optional branch | Code unique in org DB; deactivated, never deleted (no DELETE grant) |
| `shift_versions` | Times and options per version (SM-AT-1) | One approved version per start date DB; night flag required when end ≤ start (DEC-23) DB; flexible/core/split fields present for their kind DB; no quarter options exist (DEC-24); in force on D = latest approved `valid_from` ≤ D |
| `job_version_allowed_shifts` | Shifts allowed for auto-detect (ATT-04) | Written with the job version, never changed |
| `attendance_imports`, `attendance_import_rows` | Roster uploads and punch imports (SM-AT-10) | Same file never applies a row twice DB |
| `roster_entries` | Shift or weekly off per employee-date | One current entry per employee-date (INV-AT-06) DB; replaced entries kept (`superseded_at`) |
| `holiday_calendars`, `holidays` | Calendar per year; holidays with kind | One live holiday per calendar date DB; removed holidays kept (`removed_at`) |
| `branch_calendars` | Which calendar a branch uses in a year | One per branch per year (INV-AT-07) DB |
| `optional_holiday_picks` | SM-AT-6 | One live pick per employee and holiday DB; decider ≠ raiser DB; HD-01 limit API |
| `shift_swap_requests` | SM-AT-7 | Swap needs a colleague and their shift DB; decider ≠ raiser DB |

### 2.2 Punch collection (0013)

| Table | Purpose | Key rules |
|---|---|---|
| `devices` | eSSL devices per branch | Serial unique in org (INV-AT-04) DB; only the token's SHA-256 is stored (DEC-46); `is_online` remembers the last health state so each offline spell alerts once |
| `geo_fences`, `geo_fence_versions` | Fences and their insert-only versions | Radius 20–5,000 m DB; a punch is checked against the version in force at punch time (BR-AT-09); ≤ 10 per branch API |
| `raw_punches` | Every punch as received (BR-AT-01) | **Partitioned by month.** UPDATE and DELETE refused by trigger for every role DB, and not granted. Employee may be NULL only for a device punch with an enrolment ID DB |
| `punch_keys` | Dedupe key per source (INV-AT-02) | Primary key (org, source, key) DB. Separate table because a unique index on `raw_punches` would have to include the punch time, and a retried live push can resolve to a different effective time. Keys older than 90 days are purged |
| `punch_reviews` | Flagged-punch review (SM-AT-4) | One per punch DB; decider ≠ the person who punched DB |
| `manual_punches` | HR entries and regularisation punches | Insert-only except voiding, which needs a reason DB |
| `punch_adjudications` | Exclude / include / move date / change direction | Insert-only, trigger DB; latest per punch wins |

**Unmapped device punches (DEC-44, DQ-AT-07).** A device punch keeps its enrolment ID. The employee is whoever held that ID on the punch date, from the dated `employee_job_versions.enrolment_id` (Package 1, INV-08). So mapping an ID later needs no update to any punch: the engine finds unmapped punches by `(org, enrolment_id, punched_at) WHERE employee_id IS NULL`.

### 2.3 Rules (0014)

| Table | Purpose | Key rules |
|---|---|---|
| `policy_sets` | Named policy sets | One default per org DB (AC-AT-21.1) |
| `policy_set_versions` | All attendance settings of a version as one JSON document, validated by the core library (same pattern as `org_settings_versions`) | One approved version per start date DB; approved content frozen by trigger DB; approver ≠ author DB (§3.23) |
| `policy_assignments` | Set per branch, department, category or employee type, dated | No overlapping active assignment for the same target DB (INV-AT-05). The employee override is `employee_job_versions.policy_set_id` |
| `waivers` | Bulk late/early waivers (SM-AT-11) | Branch or shift required DB; withdrawal needs a reason DB |
| `day_results` | One row per calculated version | **Partitioned by month.** Exactly one current row per employee-date DB (INV-AT-09); version numbers unique DB; deduction only 0, 0.5 or 1 day (DEC-24) DB. API may change only `is_current` and `finalised_at` |

### 2.4 Regularisation and periods (0015)

| Table | Purpose | Key rules |
|---|---|---|
| `regularisation_requests` | SM-AT-5 | Reason required DB; cancellation needs a reason DB |
| `regularisation_decisions` | Each approve / reject / escalate / automatic step | Insert-only DB; the raiser can't decide DB (trigger) and API |
| `day_overrides` | HR direct correction (RG-10) | Insert-only except voiding with reason DB |
| `period_settings_versions` | PL-02–07 per payroll group | Same lifecycle and freeze as policy versions DB |
| `attendance_periods` | Periods per payroll group | No overlap DB (INV-AT-12); no gaps API |
| `lock_units` | Period × branch (DEC-49), SM-AT-9 | One per period and branch DB; state moves one step at a time DB (trigger); no row = Open |
| `lock_unit_transitions` | History of every move | Insert-only DB; one step DB; a step back needs an actor and reason DB |
| `period_results` | Period-end counters and penalties, versioned | One current per employee and period DB |
| `adjustments` | Differences posted to the next open period (BR-AT-18) | Insert-only DB; target ≠ original period DB; non-zero DB |
| `notification_settings` | Event × channel × recipient on/off | No row = on |
| `in_app_notifications` | Per-user messages | API may only mark read |

### 2.5 Seed and functions (0013, 0016)

| Object | What | Who can run it |
|---|---|---|
| `device_for_push(serial, token_hash)` | Finds the organisation and device for a push before the organisation is known (TSD §9.1); active devices of active organisations only; returns ids only | `app_api` (gateway) |
| `add_month_partitions(parent, from, months)` | Creates monthly partitions with RLS | Migration role only (operations, before 2028-12) |
| `add_default_policy_set(org)` + trigger on `organisations` | Default policy set for every organisation | Trigger (inside `provision_organisation`) |
| 41 permission codes; built-in role defaults | PERMISSION_MATRIX Package 2 | Seed; also added to every organisation's built-in role copies. Custom roles untouched |

## 3. Effective dating

| Data | Model | In force on D |
|---|---|---|
| Shift times | `shift_versions` | Latest approved `valid_from` ≤ D |
| Policy settings | `policy_set_versions` | Same |
| Period settings | `period_settings_versions` | Same |
| Assignments | `policy_assignments.valid` (daterange) | Active row containing D |
| Default shift, allowed shifts, override | `employee_job_versions` (Package 1 timeline) | Active job version containing D |
| Roster | `roster_entries` | Current row for (employee, D) |
| Fences | `geo_fence_versions` | Latest `valid_from` ≤ punch time |
| Day result | `day_results` | `is_current`; "as known on K" = latest version with `calculated_at` ≤ K |

Dates are in the master branch's time zone on that date (DEC-50); timestamps are UTC.

## 4. How the main flows write

| Flow | Rows written in one transaction (all audited unless noted) |
|---|---|
| Device push (gateway) | `device_for_push` → set org → per punch: `punch_keys` insert (conflict ⇒ skip) → `raw_punches` → `punch_reviews` if flagged → outbox `PunchRecorded`; `devices.last_contact_at`. Raw punches are not copied to `audit_log` (they are their own record) |
| App / web punch | Same path, key from the app's offline queue id |
| Reject flagged punch | `punch_reviews` decided → `punch_adjudications` (exclude) → outbox |
| Recalculate a day | Lock (employee, date) → compare fingerprint → old current row `is_current = false` → new version inserted → outbox `DayResultChanged`. If the date is locked: no write to `day_results`; `adjustments` instead (shadow) |
| Approve regularisation | `regularisation_decisions` → request state → `manual_punches` / `punch_adjudications` with `regularisation_id` → outbox |
| Cancel regularisation | Request `cancelled` → its manual punches voided; reverse adjudications inserted |
| Lock a unit | `lock_units` state (one step) + `lock_unit_transitions` → period-end results → payroll input export job |
| Step back | Same, by the Org Admin only, with reason |

## 5. Privileges

- **No DELETE** on any Package 2 table except `punch_keys` (90-day purge). Removed/withdrawn/voided/superseded markers instead.
- **Never updated by anyone** (trigger): `raw_punches`, `punch_adjudications`, `regularisation_decisions`, `lock_unit_transitions`, `adjustments`.
- **Column-level UPDATE only** elsewhere (see each migration's grant list). Examples: `day_results (is_current, finalised_at)`; `manual_punches` and `day_overrides` (void fields only); `roster_entries` (supersede fields only).

## 6. Volume, partitions and indexes

| Query | Index |
|---|---|
| An employee's punches for a date range (engine) | `raw_punches (org_id, employee_id, punched_at)` + partition pruning |
| Unmapped punches of an enrolment ID | partial `(org_id, enrolment_id, punched_at) WHERE employee_id IS NULL` |
| Flagged punches queue | partial `(org_id, punched_at) WHERE flags <> '{}'`; `punch_reviews (org_id, punched_at) WHERE pending` |
| Current day result | partial unique `(employee_id, work_date) WHERE is_current` |
| Branch-day dashboard, registers | `day_results (org_id, work_date, branch_id) WHERE is_current` |
| Days to finalise (hourly job) | `day_results (org_id, work_date) WHERE is_current AND finalised_at IS NULL` |
| Lock check for a date | `lock_units (org_id, branch_id) WHERE state <> 'open'` + `attendance_periods (org_id, payroll_group_id, lower(period))` |
| Pending regularisations | partial `(org_id, state, created_at) WHERE pending/escalated` |

**Size D (1,000,000 employees, DEC-13):** about 104M raw punches and 26M day-result rows (plus versions, §3.25) a month. Monthly partitions keep each index small and let old months be detached and archived without touching current ones. Every query filters by organisation and a date range so the planner prunes partitions. **Operations task:** run `add_month_partitions` for 2029 onward before December 2028 (a Gate 3 action, DBQ-AT-01).

## 7. Migrations

| # | File | Contents |
|---|---|---|
| 0012 | attendance_schedules | Settings-version enum, shifts and versions, job-version shift columns, attendance imports, rosters, holiday calendars, picks, swaps |
| 0013 | attendance_punches | Devices, fences, raw punches (partitioned), punch keys (hash), reviews, manual punches, adjudications, `device_for_push`, `add_month_partitions` |
| 0014 | attendance_rules | Policy sets and versions, assignments, job-version policy column, waivers, day results (partitioned), freeze trigger |
| 0015 | attendance_periods | Regularisation and decisions, day overrides, period settings, periods, lock units and transitions, period results, adjustments, notifications |
| 0016 | attendance_seed | Permissions, built-in role defaults, default policy set (trigger + backfill) |

Each has a `.down.sql` (development databases only; 0016 down deletes policy sets and attendance permissions).

## 8. Database review (SKILL.md Phase 3 checklist)

| Check | Result |
|---|---|
| Every client table has `org_id` and RLS | Yes, including every partition (`add_month_partitions` enables RLS on each), so `client_tables_without_rls()` stays empty |
| Cross-organisation references impossible | All FKs between client tables are composite `(org_id, …)` |
| Raw facts immutable | `raw_punches` and adjudications: trigger refuses UPDATE/DELETE for every role (BR-AT-01) |
| History never overwritten | Versions, superseded markers, voiding, insert-only decisions and transitions |
| Every DB-enforceable invariant enforced | INV-AT-01, 02, 04, 05, 06, 07, 09, 11 (requests and reviews), 12, 13 DB. API-only: INV-AT-03 (gateway), 08, 10, 14, 15, 16, 17 — each needs a backend test |
| Indexes for every listed query | §6 |
| Volume plan | Monthly partitions from day one (DEC-48) |
| Reversible | Down files for each migration |
| No secrets stored in clear | Device tokens: SHA-256 only. Selfies: file store references only |
| Money | None in Package 2 (fixed-amount penalties are counts until V2) |

## 9. Gate 3 questions

**Proposed [P]**

| ID | Question | Proposed |
|---|---|---|
| DBQ-AT-01 | Partitions exist to 2028-12. Who adds later ones? | An operations runbook step each year (and an alert when the next 6 months aren't there); the API never runs DDL |
| DBQ-AT-02 | Dedupe keys kept 90 days | Yes; a retry more than 90 days after the first receipt would make a duplicate, which is accepted |
| DBQ-AT-03 | Policy settings as one JSON document per version, validated in the core library | Yes, same as org settings; typed columns only where reports need them (day results) |
| DBQ-AT-04 | `roster_entries` is not partitioned | Accept for now: only rostered staff have rows; revisit at size C |

## 10. Not yet verified

The migrations have not been run yet. They need to run on the Supabase dev database (as 0001–0011 were) and pass the CI checks of `DATA_MODEL.md` §9: up, down, up; `client_tables_without_rls()` empty; isolation test; constraint tests. **That needs your approval**, because it changes the dev database schema.
