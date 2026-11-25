# API Contracts — Package 1: Employee Master (v1)

Phase 4 artefact (SKILL.md §4). Status: **Approved at Gate 4, 22 November 2026.**
Inputs: user stories (Gate 1), DOMAIN_MODEL / STATE_MACHINES / PERMISSION_MATRIX (Gate 2), DATA_MODEL / DATA_DICTIONARY (Gate 3), TSD v0.7 §7–8.
Owning service: the single API service (Rust, Axum), modular monolith (TSD §3). Web (Next.js) and mobile (Flutter) use the same routes. Error codes: [ERROR_CATALOG.md](../ERROR_CATALOG.md).

---

## 1. Conventions (apply to every route)

### 1.1 Transport and versioning
- JSON over HTTPS under `/v1`. Field names `snake_case`. Unknown request fields are refused (`unknown_field`), so typos never pass silently.
- Dates `YYYY-MM-DD`, read in the employee's master-branch time zone. Timestamps RFC 3339 in UTC (`2026-11-22T09:30:00Z`).
- IDs are UUIDs. Enum values are the database values (e.g. `long_leave`, `employee_portal`).
- A breaking change means `/v2`. Adding optional fields or new routes is not breaking.
- The OpenAPI 3.1 file is generated from the Rust route definitions in Phase 5 and checked against this document in CI.

### 1.2 Authentication
- `Authorization: Bearer <access token>`. The access token is short-lived (15 minutes) and signed by the API. It carries **only** `user_id`, `org_id`, `session_id` and `kind` (admin / employee).
- **Permissions and scope are read from the database on every request**, not from the token, so a role change applies from the user's next request (AC-AC-01.1) and a revoked session stops at once.
- Every request: verify token → check the session is live → `set_config('app.org_id', …)` and `app.user_id` in a new transaction → load permissions and scope → run the operation.
- Suspended organisation (VC-R4): GET routes and exports work; every other route returns `organisation_suspended`.

### 1.3 Authorization
Each route names its **permission** (operation code from PERMISSION_MATRIX) and its **scope rule**:

| Scope rule | Meaning |
|---|---|
| `org` | Organisation-level data. Permission is enough. |
| `employee-view` | Employee must be visible to the caller: master branch in the caller's scope today; or an allowed branch in scope and EM-10 ≠ not visible; or in the caller's reporting chain (Manager); or the caller's own record (Employee). |
| `employee-edit` | Employee must be editable: master branch in scope today, or allowed branch in scope with EM-10 = view and edit. |
| `self` | The caller's own employee record only. |

- A record outside the caller's organisation **or** not visible in their scope returns **404 `not_found`**, so existence is never revealed (AC-EM-20.5).
- A visible record the caller may not change returns **403 `forbidden`**.
- A route with no declared permission and scope fails the build (TSD §7).

### 1.4 Errors (RFC 9457 problem details)
```json
{
  "type": "https://errors.payroll.app/validation_failed",
  "title": "Some fields are not valid",
  "status": 422,
  "code": "validation_failed",
  "detail": "2 fields need attention",
  "errors": [
    { "field": "mobile", "code": "mobile_in_use", "message": "Mobile number belongs to PBYCHN0007",
      "meta": { "employee_code": "PBYCHN0007" } },
    { "field": "department_id", "code": "required", "message": "Department is required" }
  ],
  "correlation_id": "6f1c…"
}
```
`code` is stable and is what clients switch on; `message` is display text. Every response carries `X-Correlation-Id`.

### 1.5 Concurrency (BR-EM-25)
- Records with `row_version` return it as `ETag: "<row_version>"` and in the body as `version`.
- Every update of such a record **requires** `If-Match: "<version>"`. Missing → 428 `precondition_required`. Different → 412 `stale_version` with the current version in `meta`.
- Each command runs in one database transaction: business rows, audit entries and outbox events commit together or not at all.

### 1.6 Idempotency
- Create-type commands marked **Idem** require header `Idempotency-Key` (8–100 characters, a new UUID per user action).
- The same key with the same body within 24 hours returns the first response (same status and body). The same key with a different body → 422 `idempotency_key_reused`. A first request still running → 409 `request_in_progress`.
- Routes that return a temporary PIN are **not replayed**: a retry with the same key returns 409 `pin_already_issued`, and HR must use PIN reset. The PIN is never stored.
- State-change commands (approve, cancel, withdraw…) are naturally idempotent: repeating one that has already happened returns the current state with 200 (STATE_MACHINES "Common to all machines").

### 1.7 Lists
- `?limit=` (1–200, default 50) and `?cursor=` (opaque). Response: `{ "items": [...], "next_cursor": "…" | null }`. No total count by default; `?include_total=true` adds `total` where noted.
- `?sort=field` or `?sort=-field` from the route's allowed list. Filters are query parameters; repeat a parameter for "any of" (`?status=active&status=probation`).

### 1.8 Audit and events
- Every command writes `audit_log` with `action` = the route's permission code, old and new values, reason when the route takes one, and the request's correlation id. **Never** PINs, passwords, one-time codes or tokens (AC-EM-21.3).
- Events go to `outbox_events` in the same transaction (DOMAIN_MODEL §7).

### 1.9 Schema additions found in this phase (migration 0008, for approval with this gate)

| Addition | Needed by |
|---|---|
| `custom_field_definitions.inactive_options` | Deactivate a dropdown option in use (AC-EM-04.2) |
| `employment_periods.probation_reminder_sent_for` | Send each probation reminder once; resend after an extension (AC-EM-12.1, 12.3, 12.4) |
| `user_credentials.otp_hash`, `otp_expires_at` | Email one-time code sign-in (AC-EM-19.5) |
| `idempotency_keys` table | §1.6 |
| `export_jobs` table | Background exports (AC-EM-22.3) |

### 1.10 Notes on behaviour
- **JobChanged** is emitted when a dated change is *saved* (with its start date), including future dates. Nothing is emitted when a scheduled date arrives: Attendance reads the version in force for each day it calculates. This replaces "Scheduled → Effective emits JobChanged" in SM-3. The outcome is the same and no daily job is needed.
- **Admin invitation** uses the admin's `user_credentials` row: state `temporary` holds the hash of the invitation token, valid 7 days. No extra table is needed.

---

## 2. Shared resource shapes

### 2.1 Employee (as returned by `GET /employees/{id}`)
```json
{
  "id": "uuid", "employee_code": "PBYCHN0001", "version": 4,
  "as_of": "2026-11-22",
  "personal": {
    "full_name": "Meena R", "date_of_birth": "1994-03-02", "gender": "female", "photo_url": "https://… (signed, 10 min)",
    "mobile": "+919876543210", "email": "meena@example.com", "address": "…",
    "emergency_contact_name": "…", "emergency_contact_phone": "+91…"
  },
  "employment": { "period_id": "uuid", "joining_date": "2026-04-01", "probation_end_date": "2026-09-30",
                  "last_working_day": null, "exit_reason": null, "exit_remarks": null },
  "status": { "value": "active", "since": "2026-10-01" },
  "pending_status_request": { "id": "uuid", "to_status": "notice", "effective_date": "2026-12-01" } ,
  "job": {
    "version_id": "uuid", "valid_from": "2026-11-16", "valid_to": null,
    "master_branch": { "id": "uuid", "short_code": "CHN", "name": "Chennai" },
    "allowed_branches": [ { "id": "uuid", "short_code": "BLR", "name": "Bengaluru" } ],
    "department": { "id": "uuid", "code": "OPS", "name": "Operations" },
    "designation": null,
    "category": { "id": "uuid", "code": "SKL", "name": "Skilled" },
    "employee_type": { "id": "uuid", "code": "PERM", "name": "Permanent" },
    "contractor": null,
    "reporting_manager": { "id": "uuid", "employee_code": "PBYCHN0002", "full_name": "…" },
    "payroll_group": { "id": "uuid", "name": "All employees" },
    "enrolment_id": "1042"
  },
  "policy_set": { "state": "not_configured" },
  "enrolment_status": "enrolled",
  "portal_access": "employee_portal",
  "sign_in": { "credential_state": "active", "locked_until": null, "devices": 1 },
  "custom_fields": { "<field_id>": "value" },
  "incomplete_fields": [],
  "scheduled_changes_count": 1,
  "can": { "edit": true, "raise_status": true, "issue_pin": true }
}
```
- Names of lists, branches and contractors are shown **as named on `as_of`** (AC-OR-02.3).
- `can` tells the client which actions to offer. The server still checks every action.
- `sign_in` and `can` are omitted for callers without `employee.pin.issue` / edit rights. An Employee reading their own record (`/me/profile`) gets `personal`, `employment`, `status`, `job` and `custom_fields` only.

### 2.2 Job fields (used in create, job changes, rejoin)
```json
{ "master_branch_id": "uuid", "allowed_branch_ids": ["uuid"], "department_id": "uuid", "designation_id": "uuid|null",
  "category_id": "uuid", "employee_type_id": "uuid", "contractor_id": "uuid|null",
  "reporting_manager_id": "uuid|null", "payroll_group_id": "uuid", "enrolment_id": "string|null" }
```
Validation: active items only (AC-OR-01.4); master ∉ allowed and allowed obeys EM-06 (BR-EM-03); EM-11 contractor rule (BR-EM-26); manager is a current employee, not self, no loop (BR-EM-11); enrolment ID free on the dates (BR-EM-24).

### 2.3 Status change request
```json
{ "id": "uuid", "employee": { "id": "uuid", "employee_code": "…", "full_name": "…" },
  "from_status": "active", "to_status": "exited", "effective_date": "2026-12-31", "reason": "…",
  "exit": { "last_working_day": "2026-12-31", "exit_reason_id": "uuid", "remarks": "…" },
  "state": "pending", "approver_role": "org_admin",
  "raised_by": { "id": "uuid", "name": "…" }, "raised_at": "…",
  "decided_by": null, "decided_at": null, "decision_reason": null,
  "cancelled_by": null, "cancelled_at": null, "cancel_reason": null, "version": 1 }
```

---

## 3. Sign-in and sessions (`/v1/auth`) — no bearer token unless noted

| # | Route | Purpose | Request → Response | Errors | Notes |
|---|---|---|---|---|---|
| A1 | `POST /auth/employee/sign-in` | Employee sign-in (AC-EM-19.1) | `{org_short_code, employee_code, pin, device:{id, platform}}` → `{access_token, refresh_token, expires_in, must_change_pin, new_device}` | `invalid_credentials` (401, same for every cause), `account_locked` (423, `meta.locked_until`), `pin_expired` (401) | Wrong code or PIN adds to failed attempts; 5 → locked 15 min (AC-EM-19.6). Unknown org/employee/no portal access return the same `invalid_credentials`. New device is registered and shown to HR (AC-EM-19.7). Rate-limited per IP and per org+code. Web without app plan: allowed (MD-02); app sign-in on a plan without the mobile app → `mobile_app_not_in_plan` (403) |
| A2 | `POST /auth/employee/otp/request` | Email a one-time code (AC-EM-19.5) | `{org_short_code, employee_code}` → 202 `{}` always | — | Code: 6 digits, valid 10 min, only the hash is stored. Sent only if the employee has portal access and an email. Always 202, so nothing is revealed |
| A3 | `POST /auth/employee/otp/verify` | Sign in with the code | `{org_short_code, employee_code, code, device}` → same as A1 | as A1 | Wrong codes count toward the same lockout |
| A4 | `POST /auth/employee/pin` 🔑 | Set own PIN; required after a temporary PIN (AC-EM-19.3) | `{current_pin, new_pin}` → 204 | `invalid_credentials`, `pin_too_weak` (422: not 6 digits, or all-same / sequential) | While `must_change_pin` is true, every other route returns 403 `pin_change_required` |
| A5 | `POST /auth/admin/sign-in` | Admin step 1 | `{email, password}` → `{mfa_token, mfa_setup_required}` | `invalid_credentials`, `account_locked` | |
| A6 | `POST /auth/admin/mfa` | Admin step 2 (authenticator code) | `{mfa_token, totp_code}` → tokens | `invalid_credentials` | |
| A7 | `POST /auth/admin/accept-invite` | First password + authenticator setup | `{invite_token, password}` → `{mfa_setup: {secret, otpauth_uri}, mfa_token}` | `invite_invalid_or_expired` (410), `password_too_weak` | Invite valid 7 days |
| A8 | `POST /auth/refresh` | New access token | `{refresh_token}` → `{access_token, refresh_token, expires_in}` | `session_revoked` (401) | Refresh token rotates every use; reusing an old one revokes the session |
| A9 | `POST /auth/sign-out` 🔑 | End this session | → 204 | | |
| A10 | `GET /me/access` 🔑 | My permissions and scope, for the UI | → `{user, org:{name, short_code, includes_mobile_app, state}, permissions:[…], scope:{kind, branch_ids, department_ids}, employee_id}` | | |

🔑 = bearer token required.

---

## 4. Organisation structure (`/v1/org`)

| # | Route | Permission · scope | Request → Response | Errors | Idem / concurrency · audit / events |
|---|---|---|---|---|---|
| O1 | `GET /org/branches` | `employee.view` or `org.branch.manage` · org | `?state=&q=` → list of `{id, short_code, name, address, time_zone, state, version}` | | |
| O2 | `POST /org/branches` | `org.branch.manage` · org | `{short_code, name, address?, time_zone = "Asia/Kolkata"}` → 201 branch | `short_code_in_use` (409), `invalid_short_code`, `unknown_time_zone` (422) | Idem · audit |
| O3 | `PATCH /org/branches/{id}` | `org.branch.manage` | any of `{short_code, name, address, time_zone}` → branch | as O2 | If-Match · audit. Short code change never changes existing employee codes (AC-OR-01.5) |
| O4 | `POST /org/branches/{id}/deactivate` · `/activate` | `org.branch.manage` | `{reason}` → branch | `branch_has_current_employees` is **not** an error: deactivation is allowed; the response lists `employees_with_this_branch` so HR can move them (edge case 9) | If-Match · audit |
| O5 | `GET /org/lists/{list}` | `employee.view` or `org.lists.manage` | `list` ∈ `departments, designations, categories, employee-types, exit-reasons`; `?state=` → items `{id, code, name, state, version}` | | |
| O6 | `POST /org/lists/{list}` | `org.lists.manage` | `{code, name}` → 201 | `code_in_use` (409) | Idem · audit |
| O7 | `PATCH /org/lists/{list}/{id}` | `org.lists.manage` | `{code?, name?}` | `code_in_use` | If-Match · audit; rename kept in name history |
| O8 | `POST /org/lists/{list}/{id}/deactivate` · `/activate` | `org.lists.manage` | `{reason}` | | If-Match · audit |
| O9 | `POST /org/lists/apply-template` | `org.lists.manage` | `{sector: factory \| supermarket \| restaurant_bakery \| office_services}` → counts added | `lists_not_empty` (409) | Only while all four job lists are empty (DBQ-04). Idem · audit |
| O10 | `GET/POST /org/contractors`, `PATCH /org/contractors/{id}`, `POST …/deactivate` | view: `employee.view` · write: `org.contractor.manage` | `{code, name, contact_name?, contact_phone?, contact_email?}` | `code_in_use` | As lists |
| O11 | `GET /org/contractors/{id}/workers` | `employee.view` · employee-view | `?as_of=` → employees linked today (AC-OR-03.2) | | Filtered by the caller's scope |
| O12 | `GET /org/payroll-groups` | `employee.view` or `org.payroll_group.manage` | → `{id, name, pay_frequency, month_start_day, week_start_day, cycle_anchor, is_default, state, member_count_today, version}` | | |
| O13 | `POST /org/payroll-groups` · `PATCH …/{id}` | `org.payroll_group.manage` | `{name, pay_frequency, month_start_day?, week_start_day?, cycle_anchor?}` | `name_in_use`, `invalid_cycle` (422) | Idem / If-Match · audit |
| O14 | `DELETE /org/payroll-groups/{id}` | `org.payroll_group.manage` | → 204 | `group_has_members` (409, ever had members, AC-OR-05.5), `cannot_delete_default` (409) | If-Match · audit |
| O15 | `POST /org/payroll-groups/{id}/assign` | `org.payroll_group.manage` + `employee.edit` · employee-edit for each | `{effective_from, employee_ids? , rule?: {branch_ids, department_ids, category_ids, employee_type_ids}, reason}` → `{changed: n, skipped: [{employee_id, code}]}` | `date_in_locked_period`, `not_found` | Idem. One dated job change per employee (as E6). Employees outside scope are skipped, not failed. Events `JobChanged` per employee |
| O16 | `GET /org/settings` | `employee.view` (read) | `?as_of=` → `{groups: {OR: {...values, valid_from}, EM: {...}}}` with defaults filled in | | |
| O17 | `PUT /org/settings/{group}` | `org.settings.edit` | `{valid_from, values, reason}` → 201 version | `invalid_setting` (422, field = setting key) | Validated by the core schema; settings history = versions (AC-OR-04.2). Idem · audit |
| O18 | `GET /org/settings/employee-code/preview` | `org.settings.edit` | `?pattern=…&branch_id=` → `{next_code}` | `invalid_pattern` | AC-EM-02.2 preview; reads, never consumes a number |
| O19 | `GET/POST /org/custom-fields`, `PATCH …/{id}`, `POST …/{id}/deactivate` | view: `employee.view` · write: `employee.custom_fields.manage` | `{label, field_type, options?, is_mandatory}`; PATCH may set `inactive_options` | `label_in_use`, `options_required`, `cannot_change_type_with_values` (409) | A deactivated field is hidden; values kept (AC-EM-04.4). If-Match · audit |

---

## 5. Employees (`/v1/employees`)

### 5.1 Read

| # | Route | Permission · scope | Request → Response | Notes |
|---|---|---|---|---|
| E1 | `GET /employees` | `employee.view` · rows filtered to the caller's view scope | Filters: `branch_id, department_id, category_id, employee_type_id, status, payroll_group_id, contractor_id, reporting_manager_id, incomplete=true, q` (name, code or mobile). `as_of` (default today). Sort: `employee_code, full_name, joining_date`. `include_total` allowed → `{items:[{id, employee_code, full_name, mobile, status, master_branch, department, designation, payroll_group, portal_access}], next_cursor, total?}` | AC-EM-20.1–4. `incomplete=true` lists records missing a field made mandatory later (AC-EM-03.3). `reporting_manager_id` lists reportees to reassign (AC-EM-06.3) |
| E2 | `GET /employees/{id}` | `employee.view` · employee-view | `?as_of=` → Employee (§2.1) | AC-EM-11.2 "as on date". `?known_on=` (datetime) shows the record as it was recorded at that time |
| E3 | `GET /employees/{id}/history` | `employee.history.view` · employee-view | → `{job_versions:[{version_id, valid_from, valid_to, state, changes:[{field, old, new}], reason, recorded_by, recorded_at, superseded_at}], status:[{status, effective_from, request_id}], periods:[…], personal_changes:[{field, old, new, by, at}]}` | AC-EM-11.1, AC-EM-17.2. Scheduled versions carry `state: "scheduled"` (AC-EM-11.3) |
| E4 | `GET /employees/{id}/devices` | `employee.pin.issue` · employee-view | → `[{id, platform, first_seen_at, last_seen_at}]` | AC-EM-19.7 |

### 5.2 Create and change

| # | Route | Permission · scope | Request → Response | Errors | Idem / concurrency · audit / events |
|---|---|---|---|---|---|
| E5 | `POST /employees` | `employee.create` · master branch must be in the caller's edit scope | `{employee_code? (manual pattern only), personal:{full_name, mobile, …}, joining_date, probation_end_date?, initial_status: active \| probation, job: Job fields, custom_fields?, portal_access = "none"}` → 201 Employee (+ `temporary_pin` once if portal access was granted, see E13) | `validation_failed` (422, every missing field named, AC-EM-01.2), `mobile_in_use` (409, names the holder), `employee_code_in_use` (409), `enrolment_id_in_use` (409), `employee_limit_reached` (409, VC-01 = block, names the limit), `manager_loop`, `contractor_required` (422) | **Idem** · audit · events `EmployeeCreated`. Code from the sequence (skips codes already taken, edge case 16); a failed save still uses up the number (AC-EM-02.4). Over the limit with allow-and-alert: created, vendor alerted, response header `X-Employee-Limit: exceeded` |
| E6 | `POST /employees/{id}/job-changes` | `employee.edit` · employee-edit | `{effective_from, changes: {any Job fields}, reason}` → `{versions:[…], scheduled: bool, prompts:["review_payroll_group", "review_shift"]}` | `date_in_locked_period` (409), `date_before_joining` (422), `no_change` (422), `stale_version` (412), the job-field errors | **If-Match** (employee version) · audit · event `JobChanged(effective_from)`. Past date ⇒ supersedes from that date (BR-EM-05). Master branch change returns `prompts` (AC-EM-10.4). A later exit cancels scheduled versions after the last working day automatically (edge case 11) |
| E7 | `POST /employees/{id}/job-changes/{version_id}/cancel` | `employee.edit` · employee-edit | `{reason}` → 200 | `not_scheduled` (409: already effective) | Idempotent · audit |
| E8 | `PATCH /employees/{id}/personal` | `employee.edit` · employee-edit | any of personal fields → Employee | `mobile_in_use`, `validation_failed` | **If-Match** · audit (old → new) |
| E9 | `PATCH /employees/{id}/probation` | `employee.probation.extend` · employee-edit | `{probation_end_date, reason}` | `not_in_probation` (409), `date_before_joining` | If-Match · audit. Reminder is re-armed (AC-EM-12.4) |
| E10 | `PUT /employees/{id}/custom-fields` | `employee.edit` · employee-edit | `{ "<field_id>": value \| null }` | `invalid_value` (422, per field), `option_inactive` | If-Match · audit |
| E11 | `PATCH /employees/{id}/enrolment-status` | `employee.edit` · employee-edit | `{enrolment_status}` | | If-Match · audit. The enrolment **ID** is a job field (E6) · event `EnrolmentChanged` when the ID changes |
| E12 | `POST /employees/{id}/rejoin` | `employee.rejoin` · new master branch in edit scope | `{joining_date, probation_end_date?, initial_status, job: Job fields, mobile?, reason}` → Employee | `not_exited` (409), `status_request_pending` (409, edge case 7), `mobile_in_use` / `enrolment_id_in_use` (409: enter new ones, AC-EM-14.2), `employee_limit_reached`, `date_before_last_exit` (422) | **Idem** · If-Match · audit · events `EmployeeCreated`, `StatusChanged`. Keeps the code; new period (AC-EM-14.1) |

### 5.3 Status changes (`/v1/status-requests`)

| # | Route | Permission · scope | Request → Response | Errors | Notes |
|---|---|---|---|---|---|
| S1 | `POST /employees/{id}/status-requests` | `employee.status.raise` · employee-edit | `{to_status, effective_date, reason, exit?: {last_working_day, exit_reason_id, remarks}}` → 201 Status change request | `request_already_pending` (409), `transition_not_allowed` (422, SM-1), `date_in_locked_period` (409), `exit_details_required` (422) | **Idem** · audit. `approver_role` = org_admin, or hr_manager if the raiser is an Org Admin (BR-EM-27) |
| S2 | `GET /status-requests` | `employee.status.raise` or `employee.status.approve` · employee-view | `?state=pending&state=scheduled&awaiting_me=true&employee_id=` → list | | `awaiting_me` = pending, my approver role, not raised by me |
| S3 | `GET /status-requests/{id}` | as S2 | → request | | |
| S4 | `POST /status-requests/{id}/approve` | `employee.status.approve` · employee-view; caller's role = `approver_role`; caller ≠ raiser | `{note?}` → request (`effective` or `scheduled`) | `cannot_decide_own_request` (403), `wrong_approver_role` (403), `not_pending` (409), `transition_not_allowed` / `date_in_locked_period` (checks re-run) | If-Match · audit. If effective now: status row added; event `StatusChanged`; for exit: `EmployeeExited`, sessions revoked, mobile and enrolment ID freed, period closed |
| S5 | `POST /status-requests/{id}/reject` | as S4 | `{reason}` (required) | as S4 | If-Match · audit |
| S6 | `POST /status-requests/{id}/withdraw` | the raiser | → request | `not_pending` | If-Match · audit |
| S7 | `POST /status-requests/{id}/cancel` | `employee.status.cancel` · employee-edit | `{reason}` (required) | `not_scheduled` (409: already effective) | If-Match · audit (AC-EM-09.8) |

### 5.4 Portal access and PINs

| # | Route | Permission · scope | Request → Response | Errors | Notes |
|---|---|---|---|---|---|
| E13 | `POST /employees/{id}/portal-access` | `employee.pin.issue` · employee-edit | `{send_email: bool}` → 201 `{temporary_pin, expires_at, emailed}` | `already_has_access` (409), `employee_not_current` (409) | Creates or reactivates the user; credential `temporary`. **PIN shown once**, never stored or logged (AC-EM-19.2). Email sent after commit; email failure doesn't undo the grant (`emailed: false`). Idem key required but never replayed (§1.6) · audit (without the PIN) |
| E14 | `DELETE /employees/{id}/portal-access` | `employee.pin.issue` · employee-edit | `{reason}` → 204 | `no_access` (409) | User and credential revoked; all sessions revoked at once (AC-EM-19.0a) · audit |
| E15 | `POST /employees/{id}/pin-reset` | `employee.pin.issue` · employee-edit | `{send_email}` → `{temporary_pin, expires_at, emailed}` | `no_access` | Old PIN stops; all sessions revoked; lock cleared (AC-EM-19.4) · audit |
| E16 | `POST /employees/{id}/unlock` | `employee.pin.issue` · employee-edit | → 204 | | Clears a lockout early · audit |

### 5.5 Bulk import (`/v1/employee-imports`)

| # | Route | Permission · scope | Request → Response | Errors | Notes |
|---|---|---|---|---|---|
| B1 | `GET /employee-imports/template` | `employee.bulk_import` | → `.xlsx` | | Standard fields, the org's mandatory flags, custom fields (AC-EM-15.1) |
| B2 | `POST /employee-imports` | `employee.bulk_import` | multipart `file` (.xlsx or .csv, ≤ 5 MB, ≤ 5,000 rows) → 202 `{import_id, state, duplicate_of?}` | `file_unreadable`, `wrong_template` (422), `file_too_large` (413) | **Idem**. Same checksum as an earlier import: accepted, rows already applied are skipped (AC-EM-15.5). Processed in the background |
| B3 | `GET /employee-imports/{id}` | `employee.bulk_import` | → `{state, total_rows, applied_count, error_count, failure_reason}` | | |
| B4 | `GET /employee-imports/{id}/errors` | `employee.bulk_import` | → signed file link (`.xlsx`: row, field, code, message) valid 10 min | | Kept 30 days |
| B5 | `GET /employee-imports` | `employee.bulk_import` | list, newest first | | |

Row rules: same validation as E5/E6/E8; an update row needs `effective_from` for job fields (AC-EM-15.4); a row for an employee outside the uploader's scope fails as `not_found` (edge case 6); VC-01 = block stops new-employee rows once the limit is reached, and later rows fail with `employee_limit_reached` (edge case 12).

---

## 6. Self-service (`/v1/me`) — Employee

| # | Route | Permission · scope | Request → Response | Errors | Notes |
|---|---|---|---|---|---|
| M1 | `GET /me/profile` | `employee.view` · self | → personal, employment, status, job, custom fields | | AC-EM-16.1 |
| M2 | `PATCH /me/contact` | `employee.profile.edit_own` · self, and EM-03 = on | any of `{mobile, email, address, emergency_contact_name, emergency_contact_phone}` | `self_edit_disabled` (403, EM-03 off), `mobile_in_use` (409) | If-Match · applied at once · audit (AC-EM-17.1–4) |

---

## 7. Roles and users (`/v1/access`)

| # | Route | Permission · scope | Request → Response | Errors | Notes |
|---|---|---|---|---|---|
| R1 | `GET /access/permissions` | `access.role.edit` | → catalogue `{code, description, is_fixed}` | | |
| R2 | `GET /access/roles`, `GET /access/roles/{id}` | `access.role.edit` or `access.role.assign` | → `{id, name, builtin_key, state, permissions:[codes], user_count, version}` | | |
| R3 | `POST /access/roles` | `access.role.edit` | `{name, copy_from_role_id?, permissions?}` → 201 | `name_in_use`, `custom_roles_disabled` (403, AC-06 = off) | Idem · audit |
| R4 | `PUT /access/roles/{id}/permissions` | `access.role.edit` | `{permissions:[codes]}` (full set) | `fixed_permission` (422), `exceeds_own_rights` (403) | If-Match · audit with added and removed codes. Applies from each holder's next request (AC-AC-01.1) |
| R5 | `GET /access/users` | `access.role.assign` | `?kind=admin` → users with roles and scopes | | |
| R6 | `POST /access/users` | `access.role.assign` | `{email, display_name, roles:[{role_id, scope_kind, branch_ids?, department_ids?}]}` → 201 (invitation emailed) | `email_in_use` (409, platform-wide, DBQ-01), `exceeds_own_rights` | Idem · audit |
| R7 | `PUT /access/users/{id}/roles` | `access.role.assign` | `{roles:[…]}` (full set) | `last_org_admin` (409), `exceeds_own_rights` (403) | If-Match · audit (AC-AC-01.4–5, edge case 19) |
| R8 | `POST /access/users/{id}/suspend` · `/reactivate` | `access.role.assign` | `{reason}` | `last_org_admin` | Suspend revokes sessions · audit |

---

## 8. Reports, exports, audit

| # | Route | Permission · scope | Request → Response | Notes |
|---|---|---|---|---|
| P1 | `GET /reports/r10` | `report.r10` · employee-view | `?as_of=&group_by=branch\|department\|category\|employee_type` → `{rows:[{group, count}], total}` plus paged employee list via E1 filters | AC-EM-22.1 |
| P2 | `GET /reports/r11` | `report.r11` · employee-view | `?from=&to=` → `{joiners:[…], leavers:[{…, last_working_day, exit_reason}]}` | AC-EM-22.2; rejoins count as joiners |
| P3 | `POST /exports` | `report.export` + the report's permission | `{report_code: r10 \| r11 \| employees, parameters}` → 200 file link (≤ 2,000 rows) or 202 `{export_id}` | Idem · audit (exports are audited, SKILL §6). Same scope as the report |
| P4 | `GET /exports/{id}` | the requester | → `{state, download_url?, expires_at}` | File kept 30 days |
| P5 | `GET /audit` | `audit.view` · employee-view for employee targets | `?target_type=&target_id=&actor_id=&from=&to=&action=` → entries | Read-only |

---

## 9. Vendor console (`/v1/vendor`) — vendor accounts only

| # | Route | Request → Response | Notes |
|---|---|---|---|
| V1 | `POST /vendor/organisations` | `{short_code, name, plan_id, employee_limit, limit_behaviour, admin_email, admin_name}` → 201 `{org_id}` | Calls `provision_organisation()`; invitation email to the first Org Admin. Idem |
| V2 | `GET /vendor/organisations` | → list with plan, limit, current employee count, state | Counts only, no employee data (TSD §4.3) |
| V3 | `PATCH /vendor/organisations/{id}` | `{employee_limit?, limit_behaviour?, plan_id?}` | If-Match · audit |
| V4 | `POST /vendor/organisations/{id}/suspend` · `/reactivate` | `{reason}` | SM-9 |

Vendor sign-in (email, password, authenticator) mirrors A5–A6 under `/vendor/auth`.

---

## 10. Internal job routes (`/internal`) — Cloud Tasks only

Called only by Cloud Tasks / Cloud Scheduler with a Google-signed OIDC token for the jobs service account; refused from anywhere else. Each call carries one `org_id` (TSD §4.2) and is safe to run twice.

| # | Route | When | Does |
|---|---|---|---|
| J1 | `POST /internal/jobs/fan-out` | Scheduler, every 15 min | For each `active_org_ids()`, queue J2–J3 for organisations whose branch local time just passed 00:15 |
| J2 | `POST /internal/jobs/status-due {org_id}` | Daily 00:15 branch time | Scheduled status requests with date ≤ today → effective (as S4) |
| J3 | `POST /internal/jobs/probation-reminders {org_id}` | Daily | Open periods in Probation with `probation_end_date − EM-04 days ≤ today` and `probation_reminder_sent_for ≠ probation_end_date` → notify HR, set `probation_reminder_sent_for` |
| J4 | `POST /internal/outbox/dispatch` | Every minute | Deliver pending events (role `app_outbox`) |
| J5 | `POST /internal/jobs/bulk-import {org_id, import_id}` | On upload | Validate and apply rows (SM-7) |
| J6 | `POST /internal/jobs/export {org_id, export_id}` | On request | Build the file |
| J7 | `POST /internal/jobs/cleanup` | Daily | Delete idempotency keys > 24 h; expire exports and import error files > 30 days |

---

## 11. Coverage: stories → routes

| Story | Routes |
|---|---|
| US-OR-01 | O1–O4 |
| US-OR-02 | O5–O9 |
| US-OR-03 | O10–O11 |
| US-OR-04 | O16–O17 |
| US-OR-05 | O12–O15 |
| US-AC-01 | R1–R8, A10 |
| US-EM-01 | E5 |
| US-EM-02 | E5, O18 |
| US-EM-03 | O17 (EM-02), E1 `incomplete` |
| US-EM-04 | O19, E10 |
| US-EM-05, 06, 07, 08 | E5, E6, E11, O15 |
| US-EM-09 | S1–S7 |
| US-EM-10 | E6, E7 |
| US-EM-11 | E2, E3 |
| US-EM-12 | E9, J3 |
| US-EM-13 | S1 (to Exited), S4, J2 |
| US-EM-14 | E12 |
| US-EM-15 | B1–B5, J5 |
| US-EM-16, 17 | M1, M2 |
| US-EM-18 | E5, E12, B2 (VC-01) |
| US-EM-19 | A1–A4, A8–A9, E4, E13–E16 |
| US-EM-20 | E1, §1.3 |
| US-EM-21 | §1.8, P5 |
| US-EM-22 | P1–P4 |
| Vendor (VC-R1, VC-01, SM-9) | V1–V4 |
