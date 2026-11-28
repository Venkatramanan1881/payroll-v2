# API Contracts — Package 2: Attendance (v1)

Phase 4 artefact (SKILL.md §4). Revision 1 · 24 November 2026 · Status: **Approved at Gate 4 on 24 November 2026** (APQ-01 to 06 and migration 0017 accepted as proposed).
Inputs: AT-attendance stories (Gate 1), DOMAIN_MODEL-attendance / STATE_MACHINES-attendance / PERMISSION_MATRIX Package 2 (Gate 2), DATA_MODEL-attendance and migrations 0012–0016 (Gate 3), BRD v0.14, TSD v0.8.
Owning service: the same single API service (Rust, Axum). Web and mobile use the same routes. Error codes: [ERROR_CATALOG.md](../ERROR_CATALOG.md) (Package 2 codes added there, marked *P2*).

---

## 1. Conventions

**All of Package 1 §1 applies unchanged:** transport and versioning, bearer authentication, permissions read on every request, problem-details errors, `If-Match` on records with a version, `Idempotency-Key` on routes marked **Idem**, cursor lists, audit and outbox in the same transaction. Only what Package 2 adds is written here.

### 1.1 Dates and times
- A **work date** (`work_date`, `date`, `from`/`to`) is the attendance date in the **master branch's time zone on that date** (DEC-50).
- Punch times are RFC 3339 UTC. Each punch in a response also carries `local_time` and `local_zone` of the branch where it was made, **for display only** (AC-AT-20.3).
- Ranges are inclusive (`from` and `to` are both counted). Lists by date allow at most 62 days per request unless noted.

### 1.2 Scope rules added
| Scope rule | Meaning |
|---|---|
| `employee-view` / `employee-edit` | As Package 1, applied to the employee a record belongs to (punches, days, requests, roster entries) |
| `branch` | The branch (device, fence, calendar link, manual attendance batch, lock unit, waiver) is in the caller's scope |
| `self` | The caller's own employee record |
| `approver` | The caller is the current approver chosen by ApproverRouter (level 1 = reporting manager, level 2 = HR Manager in scope, fallback Org Admin), and is **not** the raiser or the person who punched (🔒 BR-07, DEC-18) |

### 1.3 Locked dates (BR-AT-18)
Two behaviours, chosen per route and stated in its row:
- **Refuse** (settings and schedules: shifts, rosters, calendars, policy versions, period settings): 409 `date_locked` with `meta.lock_unit_id` and `meta.state`. Step the unit back first.
- **Adjust** (facts and decisions: punches, adjudications, reviews, regularisation decisions, corrections): accepted. The locked day is recalculated as a shadow and the difference is posted to the next open period. The response has `"adjustment": {"posted": true, "target_period_id": …}`.

### 1.4 Recalculation is asynchronous
A command that changes a day's inputs writes the change and a `…Changed` outbox event in one transaction, then returns. Days are recalculated by the recalculation job (§12, J-AT2) within seconds. Responses carry `"recalculation": "queued"`. A client that needs the new result reads the day again; `GET …/days/{date}` returns `pending_recalculation: true` while a task is waiting.

### 1.5 Setting gates
A route whose feature is switched off by a setting returns 403 `feature_disabled` with `meta.setting` (for example `SH-12`, `RG-01`, `LA-17`, `HD-01`, `PC-01.web`). Clients read `GET /me/attendance/features` (§9) to hide those actions.

### 1.6 Settings versions (SM-AT-1)
Policy versions, shift versions and period settings share one shape and lifecycle. The stored states are `draft`, `pending_approval`, `approved`, `rejected`, `discarded`, `superseded`. The API shows `approved` as **`scheduled`** while `valid_from` is in the future and **`active`** once it is the version in force.
```json
{ "id": "uuid", "valid_from": "2026-12-01", "state": "scheduled", "values": { … }, "change_reason": "…",
  "author": {"id": "uuid", "name": "…"}, "submitted_at": null, "decided_by": null, "decided_at": null,
  "decision_reason": null, "version": 1 }
```
Commands: `POST …/versions` (create draft, **Idem**), `PATCH …/versions/{vid}` (draft only, If-Match), `POST …/versions/{vid}/submit` (→ pending when AC-04 maker-checker is on, otherwise → scheduled or active), `…/approve`, `…/reject {reason}`, `…/discard`. Approve or reject: `attendance.policy.approve`, approver ≠ author 🔒 (`cannot_decide_own_request`). `valid_from` in a locked period → `date_locked` (refuse). Values are checked by the core settings schema; each broken rule is an `invalid_setting` field error naming the setting key (AC-AT-21.5).

---

## 2. Shared shapes

### 2.1 Punch
```json
{ "id": "uuid", "employee": {"id": "uuid", "employee_code": "…", "full_name": "…"} | null,
  "source": "device|app|web|import|manual", "direction": "in|out|untagged",
  "punched_at": "2026-11-24T03:30:00Z", "local_time": "2026-11-24T09:00:00", "local_zone": "Asia/Kolkata",
  "source_time": "…", "received_at": "…", "branch": {"id", "short_code"}, "device": {"id", "serial"} | null,
  "enrolment_id": "1042" | null,
  "location": {"latitude": 13.0827, "longitude": 80.2707, "accuracy_m": 18.0, "fence_distance_m": 42.5} | null,
  "selfie_url": "signed, 10 min" | null,
  "flags": ["outside_fence"], "review": {"id", "state": "pending|approved|rejected"} | null,
  "adjudications": [{"kind": "exclude", "reason": "…", "by": {…}, "at": "…"}],
  "counted": true, "voided": false }
```
`source = manual` rows are manual punches (separate table), shown in the same list. `location` and `selfie_url` need `attendance.punch.view`; employees see their own.

### 2.2 Day result
```json
{ "employee_id": "uuid", "work_date": "2026-11-24", "version": 3, "state": "provisional|finalised|locked",
  "pending_recalculation": false,
  "status": "present", "shift": {"id", "code", "name", "start": "09:00", "end": "18:00", "detected": false} | null,
  "first_in": "…", "last_out": "…", "net_minutes": 482, "net_minutes_rounded": 480,
  "late_minutes": 12, "early_minutes": 0, "late_mark": true, "early_mark": false, "deduction_days": 0,
  "flags": ["missed_punch"], "branch": {"id", "short_code"}, "costed_branches": [{"branch_id", "minutes"}],
  "calculated_at": "…", "reason": "new punch",
  "explanation": { "punches_used": [...], "punches_excluded": [...], "sessions": [...], "rules": [{"setting", "value", "policy_version_id"}],
                   "approvals": [...], "waivers": [...], "overrides": [...] },
  "can": { "regularise": true, "correct": false } }
```
`explanation` is returned on the single-day route only (AC-AT-22.5). `?known_on=` returns the version current at that moment.

### 2.3 Lock unit
```json
{ "id": "uuid", "period": {"id", "payroll_group": {"id", "name"}, "from": "2026-11-01", "to": "2026-11-30", "cut_off": null},
  "branch": {"id", "short_code", "name"}, "state": "open|locked|finalised|paid",
  "history": [{"from", "to", "by", "reason", "at"}], "version": 2 }
```
A unit with no stored row is shown as `open`, `id: null`, `version: 0`. The first lock creates it.

---

## 3. Shifts and roster (`/v1/attendance`)

| # | Route | Permission · scope | Request → Response | Errors | Idem / concurrency · audit / events |
|---|---|---|---|---|---|
| SH1 | `GET /shifts` | `attendance.roster.view` or `attendance.shift.manage` · org | `?state=&branch_id=&as_of=` → `[{id, code, name, branch, state, current: shift version values, version}]` | | |
| SH2 | `POST /shifts` | `attendance.shift.manage` | `{code, name, branch_id?, valid_from, kind, start_time, end_time, is_night, night_date_rule, required_minutes, window_before_min, window_after_min, flex_earliest?, flex_latest?, core_start?, core_end?, break_kind?, break_start?, break_end?, half_day_point, half_day_time?}` → 201 | `code_in_use` (409), `night_flag_required`, `core_outside_band`, `invalid_value` (422) | **Idem** · audit |
| SH3 | `PATCH /shifts/{id}` | `attendance.shift.manage` | `{code?, name?}` | `code_in_use` | If-Match · audit |
| SH4 | `GET/POST /shifts/{id}/versions`, `…/{vid}/submit\|approve\|reject\|discard` | `attendance.shift.manage` (approve: `attendance.policy.approve`) | §1.6 with SH2 time fields as `values` | as SH2; `date_locked` (refuse) | §1.6 · event `ScheduleChanged` on activation |
| SH5 | `POST /shifts/{id}/deactivate` · `/activate` | `attendance.shift.manage` | `{reason}` | | If-Match · audit. Deactivated shifts can't be newly assigned; existing entries stay (AC-AT-01.7) |
| R1 | `GET /roster` | `attendance.roster.view` · employee-view per row | `?from=&to=&branch_id=&department_id=&employee_id=` → `{days: [dates], rows: [{employee, entries: {"2026-11-24": {shift_id, code, is_weekly_off, source, holiday?}}}]}` | | Resolved view: roster entry, else default shift, else nothing; holidays and approved optional picks marked |
| R2 | `PUT /roster` | `attendance.roster.manage` · employee-edit per row | `{entries: [{employee_id, work_date, shift_id \| null, is_weekly_off}], reason?}` (≤ 2,000) → `{applied, unchanged, failed: [{employee_id, work_date, code}]}` | per entry: `date_locked`, `shift_inactive`, `shift_not_allowed_at_branch`, `outside_employment` | **Idem** · audit · `ScheduleChanged`. Each entry supersedes the current one; valid entries apply, failed ones are listed (AC-AT-02.2) |
| R3 | `DELETE /roster/entries` | `attendance.roster.manage` | `{employee_id, from, to, reason}` → `{removed}` | `date_locked` | Supersedes entries back to the default shift · audit |
| R4 | `POST /roster/copy` | `attendance.roster.manage` | `{from_range: {from, to}, to_start, employee_ids? \| branch_id? \| department_id?}` → as R2 | as R2 | **Idem**. Exited / not-yet-joined employees skipped (AC-AT-02.4) |
| R5 | `POST /roster/rotation` | `attendance.roster.manage` | `{employee_ids, start, weeks (1–26), pattern: [shift_id \| "weekly_off" per week-slot], days_per_slot = 7, weekly_off_days?: [1–7]}` → as R2 | as R2 | **Idem** (AC-AT-02.5) |
| R6 | `POST /roster-imports`, `GET /roster-imports[/{id}[/errors]]`, `GET /roster-imports/template` | `attendance.roster.manage` | As Package 1 B1–B5, ≤ 5 MB, ≤ 5,000 rows | as Package 1 imports | **Idem**. Same file twice: rows already applied are skipped (AC-AT-02.3) |
| R7 | `PUT /employees/{id}/shifts` | `attendance.roster.manage` · employee-edit | `{effective_from, default_shift_id, allowed_shift_ids: [], reason}` → job version | Package 1 E6 errors, `shift_inactive` | **If-Match** (employee). A dated job change (Package 1 E6), so history and scheduling work the same · `JobChanged` + `ScheduleChanged` |
| SW1 | `POST /shift-requests` | `attendance.shift_swap.request` · self | `{work_date, kind: change \| swap, shift_id?, colleague_employee_id?, reason}` → 201 | `feature_disabled` (SH-12), `date_not_future`, `shift_not_allowed`, `colleague_not_eligible` (422) | **Idem** · audit · manager and colleague told |
| SW2 | `GET /shift-requests` | requester, or `attendance.shift_swap.approve` · employee-view | `?state=&awaiting_me=true` | | |
| SW3 | `POST /shift-requests/{id}/approve` · `/reject {reason}` | `attendance.shift_swap.approve` · approver | → request | `not_pending`, `cannot_decide_own_request`, `date_locked` | If-Match · audit · approve writes both roster entries (source `swap` / `shift_change`) · `ScheduleChanged` |
| SW4 | `POST /shift-requests/{id}/withdraw` | the requester | → request | `not_pending` | If-Match · audit |
| SW5 | `GET /me/shift-options?date=` | `attendance.shift_swap.request` · self | → `{date, current: shift \| null, shifts: [{id, code, name}], colleagues: [{id, employee_code, full_name, shift}]}` | | For the request form: shifts the employee may change to, and colleagues at the same branch with a different shift where each is allowed the other's (added in build, 28 November 2026) |

## 4. Holidays and weekly offs

| # | Route | Permission · scope | Request → Response | Errors | Notes |
|---|---|---|---|---|---|
| H1 | `GET /holiday-calendars` | `attendance.roster.view` or `attendance.calendar.manage` | `?year=` → `[{id, name, year, holidays_count, branches: [{id, short_code}], version}]` | | |
| H2 | `POST /holiday-calendars` | `attendance.calendar.manage` | `{name, year, copy_from_calendar_id?}` → 201 | `name_in_use` (409) | **Idem** · audit. Copy: national dates kept, festival and optional copied for HR to adjust (AC-AT-06.6) |
| H3 | `GET /holiday-calendars/{id}` | as H1 | → `{…, holidays: [{id, date, name, kind}]}` | | Removed holidays omitted unless `?include_removed=true` |
| H4 | `POST /holiday-calendars/{id}/holidays` · `PATCH …/holidays/{hid}` · `POST …/holidays/{hid}/remove {reason}` | `attendance.calendar.manage` | `{date, name, kind}` | `date_outside_year`, `holiday_exists_on_date` (409), `date_locked` (refuse, any linked branch) | Idem / If-Match · audit · `ScheduleChanged` for linked branches |
| H5 | `PUT /branches/{id}/holiday-calendars/{year}` | `attendance.calendar.manage` · branch | `{calendar_id}` | `date_locked` (refuse if the change touches a locked date) | audit · `ScheduleChanged`. One calendar per branch per year (AC-AT-06.2) |
| H6 | `GET /me/optional-holidays` | `attendance.optional_holiday.request` · self | `?year=` → `{allowed (HD-01), used, holidays: [{id, date, name, pick: {id, state} \| null}]}` | | Shown even when HD-01 = 0 (information only, AC-AT-08.4) |
| H7 | `POST /optional-holiday-picks` | `attendance.optional_holiday.request` · self | `{holiday_id}` → 201 | `feature_disabled` (HD-01 = 0), `pick_limit_reached`, `holiday_not_optional`, `not_in_my_calendar` (422), `already_picked` (409) | **Idem** · audit · manager told |
| H8 | `GET /optional-holiday-picks` | own, or `attendance.optional_holiday.approve` · employee-view | `?state=&awaiting_me=true` | | |
| H9 | `POST /optional-holiday-picks/{id}/approve` · `/reject {reason}` | `attendance.optional_holiday.approve` · approver | → pick | `not_pending`, `cannot_decide_own_request` | If-Match · audit · `ScheduleChanged` |
| H10 | `POST /optional-holiday-picks/{id}/withdraw` (pending) · `/cancel` (approved, before the date) | the employee | → pick | `not_pending`, `date_passed` (409) | If-Match · audit |

**Weekly offs** are part of the policy set (`WO` settings: fixed days, alternating weeks, or "from roster"). A branch or employee that needs a different pattern gets a different policy set through an assignment (§6). No separate route (Gate 4 question APQ-03).

## 5. Devices, enrolment, fences, punches

### 5.1 Devices and enrolment
| # | Route | Permission · scope | Request → Response | Errors | Notes |
|---|---|---|---|---|---|
| D1 | `GET /devices` | `attendance.device.manage` · branch | `?branch_id=&state=&online=` → `[{id, serial, make, model, branch, state, is_online, last_contact_at, version}]` | | AC-AT-10.3 |
| D2 | `POST /devices` | `attendance.device.manage` · branch | `{serial, make, model, branch_id}` → 201 `{device, token, push_url}` | `serial_in_use` (409) | **Idem, never replayed**: a retry returns 409 `token_already_issued` (rotate instead). **Token shown once**, only its hash stored, never logged · audit (without token) |
| D3 | `POST /devices/{id}/rotate-token` | as D2 | → `{token, push_url}` | `device_inactive` (409) | Old token stops at once (AC-AT-10.2) · If-Match · audit |
| D4 | `PATCH /devices/{id}` · `POST /devices/{id}/deactivate {reason}` | as D2 | `{make?, model?, branch_id?}` | | If-Match · audit. Deactivated: token refused, device kept (AC-AT-10.5) |
| D5 | `GET /enrolment/unmapped` | `attendance.enrolment.manage` · branch of the device | → `[{enrolment_id, device: {id, serial, branch}, punch_count, first_at, last_at}]` | | AC-AT-12.3 |
| D6 | Map an ID | — | Use Package 1 E6 (`enrolment_id` is a dated job field). Unmapped punches from the version's `valid_from` are counted automatically and their days queued | | No update to any punch (DQ-AT-07). Enrolment status: Package 1 E11 |

### 5.2 Geo-fences
| # | Route | Permission · scope | Request → Response | Errors | Notes |
|---|---|---|---|---|---|
| F1 | `GET /branches/{id}/fences` | `attendance.fence.manage` · branch | → `[{id, name, state, latitude, longitude, radius_m, valid_from, version}]` | | |
| F2 | `POST /branches/{id}/fences` | as F1 | `{name, latitude, longitude, radius_m = 200}` → 201 (Draft) | `fence_limit_reached` (409, 10 per branch), `invalid_value` (radius 20–5,000) | **Idem** · audit |
| F3 | `PATCH /fences/{id}` | as F1 | `{name?, latitude?, longitude?, radius_m?}` | | Active fence: new version from now (AC-AT-13.3) · If-Match · audit |
| F4 | `POST /fences/{id}/test` | as F1 | `{latitude, longitude, accuracy_m}` → `{distance_m, inside}` | | Read-only (AC-AT-13.2) |
| F5 | `POST /fences/{id}/activate` · `/deactivate` | as F1 | → fence | | If-Match · audit |

### 5.3 Device gateway (eSSL push) — **blocked by PUN-01**
| # | Route | Auth | Behaviour |
|---|---|---|---|
| G1 | `/{device-protocol path}` (eSSL push, exact paths fixed after the device test) | Device serial + token in the push URL; `device_for_push(serial, sha256(token))` | Unknown or mismatched token: 401, logged, nothing stored (AC-AT-11.2). Per punch: dedupe on `(source=device, idempotency_key = serial + enrolment ID + device time)`; time rule DEC-21; unmapped kept; `devices.last_contact_at` updated. Response in the device's own format. Rate limit per device. Not under `/v1` and not JSON |

### 5.4 Punching (employee)
| # | Route | Permission · scope | Request → Response | Errors | Notes |
|---|---|---|---|---|---|
| P1 | `POST /me/punches` | `attendance.punch.self` · self | multipart: `meta` JSON `{direction: in \| out, client_time, uptime_time?, queued: bool, client_key, location?: {latitude, longitude, accuracy_m}, device: {id, platform}}` + `selfie` (JPEG ≤ 1 MB, when PC-02 requires it) → 201 `{punch (§2.1), flags, review_opened: bool, adjustment?}` | `feature_disabled` (PC-01 app/web), `device_not_bound` (403), `location_required` (422), `outside_fence` / `poor_accuracy` (422, only when set to block), `ip_not_allowed` (403), `selfie_required` (422) | Idempotent by `client_key` (offline queue key, AC-AT-14.6): a repeat returns the stored punch with 200. Source `app` with a device id, else `web`; web takes IP as SEC-12 says. Time rules §3.4. **Adjust** behaviour on locked dates. Event `PunchRecorded` |
| P2 | `GET /me/punches/today` | `attendance.punch.self` · self | → `{punches: [...], next_direction, shift, features}` | | Today's status (AC-AT-41.1) |
| P3 | `POST /employees/{id}/device-binding/reset` | `attendance.device_binding.reset` · employee-edit | `{reason}` → 204 | `no_binding` (409) | Next app sign-in binds the new phone (AC-AT-14.2) · audit |

### 5.5 Punch administration
| # | Route | Permission · scope | Request → Response | Errors | Notes |
|---|---|---|---|---|---|
| P4 | `GET /punches` | `attendance.punch.view` · employee-view per row | `?from=&to=&employee_id=&branch_id=&source=&flag=&unmapped=true&review=pending` → list of Punch | | Cursor list, newest first |
| P5 | `GET /punch-reviews` | `attendance.punch.review` · employee-view | `?state=pending&branch_id=&awaiting_me=true` → `[{id, punch, flags, opened_at}]` | | Excludes the caller's own punches |
| P6 | `POST /punch-reviews/{id}/approve` · `/reject {reason}` | `attendance.punch.review` · employee-view; caller ≠ the person who punched | → review | `cannot_decide_own_request` (403), `not_pending` (409) | If-Match · audit. Reject writes an `exclude` adjudication; **Adjust** on locked dates · employee told (NT-08) |
| P7 | `POST /punches/{id}/adjudications` | `attendance.punch.adjudicate` · employee-edit | `{kind: exclude \| include \| move_date \| change_direction, new_work_date?, new_direction?, reason}` → 201 | `invalid_adjudication` (422: include without a prior exclude, same date, same direction) | **Idem** · audit · **Adjust** on locked dates · `PunchRecorded` for both dates when moved |
| P8 | `POST /manual-attendance` | `attendance.punch.manual` · branch | `{branch_id, work_date, reason, entries: [{employee_id, in_time?, out_time?}]}` → `{created, failed: [...]}` | per entry: `outside_employment`, `invalid_value` | **Idem** · audit per entry (PUN-08). `GET /manual-attendance/sheet?branch_id=&work_date=` lists the employees expected (AC-AT-18.1) |
| P9 | `POST /manual-punches/{id}/void` | `attendance.punch.manual` · employee-edit | `{reason}` | `already_voided` | If-Match · audit · **Adjust** |
| P10 | `POST /punch-imports` (multipart CSV) · `POST /punch-imports/json` (`{rows: […] ≤ 5,000}`) · `GET /punch-imports[/{id}[/errors]]` | `attendance.punch.import` | Rows `{external_id, punch_time (with offset), employee_code \| (enrolment_id + device_serial), direction?, latitude?, longitude?}` → 202 `{import_id}` | `feature_disabled` (PC-01 import), file errors as Package 1 | **Idem**. Repeated `external_id` ignored (AC-AT-17.2). Processed in the background (J-AT6) |

## 6. Policy sets

| # | Route | Permission · scope | Request → Response | Errors | Notes |
|---|---|---|---|---|---|
| PS1 | `GET /policy-sets` | `attendance.policy.manage` or `attendance.day.view` | → `[{id, name, is_default, state, active_version: {id, valid_from}, assignments_count, version}]` | | |
| PS2 | `POST /policy-sets` | `attendance.policy.manage` | `{name, copy_from_policy_set_id?, valid_from, values?}` → 201 set + first version (draft) | `name_in_use` | **Idem** · audit |
| PS3 | `PATCH /policy-sets/{id}` · `POST …/deactivate` | as PS2 | `{name}` | `cannot_deactivate_default`, `policy_set_in_use` (409) | If-Match · audit |
| PS4 | `GET/POST /policy-sets/{id}/versions`, `…/{vid}`, `…/{vid}/submit\|approve\|reject\|discard` | §1.6 | `values` = settings document; `GET …/{vid}?with_defaults=true` fills defaults | `invalid_setting`, `date_locked` | §1.6 · `PolicyChanged` on activation |
| PS5 | `GET /policy-settings/schema` | `attendance.policy.manage` | → JSON Schema of the settings document, with defaults, labels and which settings are hidden by decisions | | Clients build the form from it |
| PS6 | `GET /policy-assignments` · `POST /policy-assignments` · `POST /policy-assignments/{id}/end {valid_to, reason}` | `attendance.policy.manage` | `{policy_set_id, level: branch \| department \| category \| employee_type, target_id, valid_from, reason}` | `assignment_overlaps` (409), `date_locked` | **Idem** / If-Match · audit · `PolicyChanged` |
| PS7 | `PUT /employees/{id}/policy-set` | `attendance.policy.manage` · employee-edit | `{effective_from, policy_set_id \| null, reason}` | Package 1 E6 errors | A dated job change (as R7) · `PolicyChanged` |
| PS8 | `GET /employees/{id}/policy-set` | `attendance.day.view` · employee-view | `?as_of=` → `{policy_set, version_id, source: employee \| employee_type \| category \| department \| branch \| default}` | | Package 1 E2 now returns this in `policy_set` (AC-AT-21.2) |

## 7. Days and recalculation

| # | Route | Permission · scope | Request → Response | Notes |
|---|---|---|---|---|
| DY1 | `GET /days` | `attendance.day.view` · employee-view per row | `?from=&to=&employee_id=&branch_id=&department_id=&status=&flag=&late=true&state=` → list of Day result without `explanation` | Register views (R2) use this |
| DY2 | `GET /employees/{id}/days/{date}` | `attendance.day.view` · employee-view | `?known_on=` → Day result with `explanation`, `punches`, `requests`, `versions: [{version, reason, calculated_at}]` | AC-AT-22.5 |
| DY3 | `POST /employees/{id}/days/recalculate` | `attendance.day.correct` · employee-edit | `{from, to, reason}` → 202 `{queued}` | Manual retry; audited. Locked dates run as shadow |

## 8. Regularisation, corrections, waivers, outages

| # | Route | Permission · scope | Request → Response | Errors | Notes |
|---|---|---|---|---|---|
| RG1 | `POST /regularisations` | `attendance.regularisation.request` · self, or employee-edit when `employee_id` ≠ self (on behalf, REL-09) | `{employee_id?, work_date, kind, requested: {in_time?, out_time?, punch_id?, direction?}, reason, attachment?}` → 201 | `feature_disabled` (RG-01 / type off), `window_passed` (RG-04), `date_locked` (refuse: requests are raised before lock), `limit_reached` (RG-08 = block), `attachment_required`, `request_already_pending` (409, same employee-date-kind) | **Idem** · audit. Over the limit with RG-08 = HR only → `over_limit: true`, routed to HR. RG-06 types → approved at once (system actor) |
| RG2 | `GET /regularisations` | own, or `attendance.regularisation.approve` · employee-view | `?state=&employee_id=&from=&to=&awaiting_me=true` | | |
| RG3 | `GET /regularisations/{id}` | as RG2 | → `{…, decisions: [{level, action, actor, reason, at}], current_approver}` | | |
| RG4 | `POST /regularisations/{id}/approve {note?}` · `/reject {reason}` | `attendance.regularisation.approve` · approver | → request | `cannot_decide_own_request`, `wrong_approver` (403), `not_pending` | If-Match · audit. Last level approve writes manual punches / adjudications; **Adjust** if the date has since locked |
| RG5 | `POST /regularisations/{id}/withdraw` | the raiser | | `not_pending` | If-Match · audit |
| RG6 | `POST /regularisations/{id}/cancel {reason}` | `attendance.regularisation.cancel` · employee-edit | | `not_approved` (409), `date_locked` (refuse) | If-Match · audit. Voids its manual punches, adds reverse adjudications |
| C1 | `POST /employees/{id}/days/{date}/overrides` | `attendance.day.correct` · employee-edit | `{fields: {status?, net_minutes?, late_mark?, early_mark?, deduction_days?}, reason}` → 201 | `feature_disabled` (RG-10), `invalid_value` | **Idem** · audit · **Adjust** on locked dates (DEC-34: any past month, warning beyond PL-07 in `meta.warning`) |
| C2 | `POST /day-overrides/{id}/void {reason}` | as C1 | | `already_voided` | If-Match · audit · **Adjust** |
| W1 | `GET/POST /waivers`, `POST /waivers/{id}/withdraw {reason}` | `attendance.waiver.manage` · branch | `{kind: late \| early, work_date, branch_id?, shift_id?, reason}` | `feature_disabled` (LA-17 / EC-16), `target_required` | **Idem** / If-Match · audit · `PolicyChanged`. **Adjust** on locked dates |
| O1 | `POST /outage-regularisations` | `attendance.outage.regularise` · branch | `{branch_id, from, to, reason}` → 202 `{affected_employees, queued}` | `feature_disabled` (RG-11), `range_too_long` (> 31 days) | **Idem** · audit. Fills missing punches from shift times as manual punches (origin `outage_fill`); **Adjust** on locked dates |

## 9. Periods, lock, adjustments, payroll input

| # | Route | Permission · scope | Request → Response | Errors | Notes |
|---|---|---|---|---|---|
| L1 | `GET /periods` | `attendance.period.view` | `?payroll_group_id=&from=&to=` → `[{id, payroll_group, from, to, cut_off, units: {open, locked, finalised, paid}}]` | | Periods are generated 3 ahead by J-AT5 (AC-AT-36.1) |
| L2 | `GET/POST /payroll-groups/{id}/period-settings/versions…` | `attendance.policy.manage` | §1.6 with PL-02–07 values | `invalid_setting`, `date_locked` | |
| L3 | `GET /periods/{id}/units` | `attendance.period.view` · branch | → `[Lock unit]` | | |
| L4 | `GET /periods/{id}/units/{branch_id}/checklist` | `attendance.period.view` · branch | → `{pending_regularisations, pending_reviews, missed_punches_unresolved, days_not_finalised, can_lock, rule: PL-05}` with counts and the first 50 items each | | AC-AT-37.1 |
| L5 | `POST /periods/{id}/lock` | `attendance.period.lock` · branch for each unit | `{branch_ids?: [] (all in scope when omitted), pending_as?: absent \| present (only when PL-05 allows)}` → `{locked: [unit], failed: [{branch_id, code, checklist}]}` | per unit: `checklist_not_clear`, `period_not_ended`, `stale_version` | **Idem** · audit per unit · `PeriodStateChanged`. Units that fail stay Open (DEC-49). Then period-end rules and the payroll input build run (J-AT7) |
| L6 | `POST /lock-units/{id}/finalise` · `/mark-paid` | `attendance.period.lock` · branch | → unit | `wrong_state` (409) | If-Match · audit |
| L7 | `POST /lock-units/{id}/step-back {reason}` | `attendance.period.step_back` · branch (Org Admin only, fixed) | → unit | `wrong_state` (open), `reason_required` | If-Match · audit · one step (DB-enforced). Back to Open recalculates the unit's days |
| L8 | `GET /adjustments` | `attendance.adjustment.view` · employee-view (employees: own) | `?employee_id=&target_period_id=&original_from=&original_to=&kind=` → `[{id, employee, original_date, original_period, target_period, kind, fields: {old, new, diff}, cause, created_at}]` | | R23 |
| L9 | `POST /periods/{id}/payroll-input` | `attendance.payroll_input.export` · branch | `{branch_ids?, format: xlsx \| csv}` → 202 `{export_id}` | `period_not_locked` (409) | Uses Package 1 exports (P4 to fetch) · audit. Columns: AC-AT-40.1–2, no overtime |

## 10. Self-service and notifications

| # | Route | Permission · scope | Request → Response | Notes |
|---|---|---|---|---|
| M1 | `GET /me/attendance/features` | any employee | → `{punch: {app, web, location_required, selfie_required}, regularisation: {enabled, kinds, window_days, limit, used}, optional_holidays: {allowed, used}, shift_requests: bool}` | Drives what the app shows (§1.5) |
| M2 | `GET /me/roster` | `attendance.roster.view` · self | `?from=&to=` (default this and next week) → entries with shift times, weekly offs, holidays | AC-AT-04.1 |
| M3 | `GET /me/days` · `GET /me/days/{date}` | `attendance.day.view` · self | as DY1 / DY2 for self | AC-AT-41.1 |
| N1 | `GET /notification-settings` · `PUT /notification-settings` | `notification.settings.manage` | `{rows: [{event, channel: email \| in_app, recipient: employee \| manager \| hr \| org_admin, enabled}]}` | audit. No row = on |
| N2 | `GET /me/notifications` · `POST /me/notifications/{id}/read` · `POST /me/notifications/read-all` | any signed-in user | `?unread=true` → list | In-app channel |

## 11. Reports and dashboard

| # | Route | Permission · scope | Notes |
|---|---|---|---|
| RP1 | `GET /reports/r1` | `report.r1` · employee-view | `?date=&branch_id=` daily attendance |
| RP2 | `GET /reports/r2` | `report.r2` · employee-view | `?period_id= \| from&to` monthly register (employee × day grid of status codes) |
| RP3 | `GET /reports/r3` | `report.r3` | late and early: marks, minutes, penalties |
| RP4 | `GET /reports/r4` | `report.r4` | missed punches and regularisations |
| RP5 | `GET /reports/r5` | `report.r5` | exceptions: flagged punches, fence, late sync, device status |
| RP6 | `GET /reports/r23` | `report.r23` | adjustments: HR corrections and automatic separately |
| RP7 | `GET /reports/r26` | `report.r26` | settings history and assignments |
| RP8 | `POST /exports` | `report.export` + the report's permission | Package 1 P3, `report_code` now also `r1–r5, r23, r26, payroll_input` |
| DB1 | `GET /dashboard/attendance` | `dashboard.attendance` · scope | HR/Org Admin: today by branch (present, absent, late, on leave = 0 until Package 3), missed punches, pending approvals, devices offline. Manager: team and pending. Employee: M3. Cached 15 min per caller scope, `?refresh=true` bypasses (TEC-04) |

R10 (days by branch) and R13 (payroll input summary) come from DY1 grouping and L9; they use the same report pattern and are listed in §14.

## 12. Internal jobs (`/internal`, Cloud Tasks with OIDC only)

| # | Route | When | Does |
|---|---|---|---|
| J-AT1 | `POST /internal/jobs/finalise {org_id}` | Hourly | Finalise branch-dates whose latest window end + 1 h has passed (§3.26) |
| J-AT2 | `POST /internal/jobs/recalculate {org_id, employee_id, from, to, reason}` | From outbox events | Per (employee, date) lock; skip when the fingerprint is unchanged; shadow on locked dates. Max 50 tasks per second per organisation (TEC-03) |
| J-AT3 | `POST /internal/jobs/attendance-sweep {org_id}` | Nightly | Requeue days whose inputs changed after their result; RG-07 escalations and auto-decisions; UA-01 absent-pending → UA-02 |
| J-AT4 | `POST /internal/jobs/device-health` | Every 5 min | Online/offline from PC-10; one alert per offline spell |
| J-AT5 | `POST /internal/jobs/periods {org_id}` | Daily | Generate periods 3 ahead; PL-04 automatic lock; pre-lock reminders (NT-09) |
| J-AT6 | `POST /internal/jobs/punch-import {org_id, import_id}` · `roster-import` | On upload | As Package 1 J5 |
| J-AT7 | `POST /internal/jobs/period-end {org_id, lock_unit_id}` | After a lock | Period-end rules, results frozen, payroll input built |
| J-AT8 | `POST /internal/jobs/attendance-reminders {org_id}` | Every 15 min | MP-04 missed-punch reminders, manager digest |
| J-AT9 | `POST /internal/jobs/attendance-cleanup` | Daily | Delete `punch_keys` > 90 days; selfie files > 13 months (DEC-51) |

J1 fan-out (Package 1) queues the per-organisation jobs. It already runs one organisation at a time in a single request, which a test run showed takes about 22 minutes for two passes over 185 organisations. Before attendance jobs are added, J1 must queue one Cloud Task per organisation instead (Gate 4 question APQ-05).

## 13. Schema additions found in this phase (migration 0017, for approval with this gate)

| Addition | Needed by |
|---|---|
| `user_devices.bound_at`, `unbound_at`, `unbound_by`, `unbind_reason`; partial unique "one bound phone per user"; UPDATE grant on the unbind columns | Phone binding (DEC-20, P1, P3). Package 1 records devices but not which one is bound |

Nothing else: export jobs, idempotency keys, outbox and audit from Package 1 are reused.

## 14. Gate 4 questions

| ID | Question | Proposed |
|---|---|---|
| APQ-01 | Recalculation is asynchronous (§1.4), so a punch response doesn't contain the new day result | Yes. A synchronous path would hold a database lock per request; the client re-reads the day |
| APQ-02 | Device gateway paths and response format | Fixed after the PUN-01 device test; everything else is built first |
| APQ-03 | Weekly-off patterns live in the policy set (no separate branch/employee route) | Yes. A different pattern = a different policy set via assignment |
| APQ-04 | Selfie upload limit 1 MB JPEG, stored in the file store, link signed for 10 minutes | Yes |
| APQ-05 | J1 changes to one Cloud Task per organisation before the attendance jobs go live | Yes, in Phase 5 (also fixes the 22-minute run) |
| APQ-06 | Reports R10 and R13 | R10 = DY1 grouped by costed branch; R13 = the payroll input file of L9 |

## 15. Coverage: stories → routes

| Story | Routes | Story | Routes |
|---|---|---|---|
| US-AT-01 | SH1–SH5 | US-AT-23 | §1.4, J-AT2, J-AT3 |
| US-AT-02 | R1–R7 | US-AT-24–31 | Engine (Phase 5 core library); results via DY1–DY2; PS4 settings |
| US-AT-03 | R7 (allowed shifts), engine | US-AT-32 | RG1–RG6, J-AT3 |
| US-AT-04 | M2 | US-AT-33 | C1–C2 |
| US-AT-05 | SW1–SW4 | US-AT-34 | W1 |
| US-AT-06 | H1–H5 | US-AT-35 | O1 |
| US-AT-07 | PS4 (WO settings), PS6 | US-AT-36 | L1–L2, J-AT5 |
| US-AT-08 | H6–H10 | US-AT-37 | L3–L5, J-AT5, J-AT7 |
| US-AT-09 | PS4 (HD settings), engine | US-AT-38 | L6–L7 |
| US-AT-10 | D1–D4, J-AT4 | US-AT-39 | §1.3, L8 |
| US-AT-11 | G1 (blocked) | US-AT-40 | L9 |
| US-AT-12 | D5–D6, Package 1 E6/E11 | US-AT-41 | M1, M3, P2 |
| US-AT-13 | F1–F5 | US-AT-42 | N1–N2, J-AT8 |
| US-AT-14, 15 | P1–P3 | US-AT-43 | RP1–RP8 |
| US-AT-16 | P5–P6 | US-AT-44 | DB1 |
| US-AT-17 | P10 | | |
| US-AT-18 | P8–P9 | | |
| US-AT-19 | P7 | | |
| US-AT-20 | P1 (fence of allowed branches), engine | | |
| US-AT-21 | PS1–PS8 | | |
| US-AT-22 | DY1–DY3, J-AT1 | | |
