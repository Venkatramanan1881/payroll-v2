# Error Catalogue — API v1

Phase 4 artefact. Status: **Approved at Gate 4, 22 November 2026.**
Every error response uses the shape in [API_CONTRACTS §1.4](API_CONTRACTS/package1-employee-master.md#14-errors-rfc-9457-problem-details). Clients switch on `code`, never on `message`. Codes are stable: a code is never renamed or reused for another meaning. A field-level error appears inside `errors[]` with the field name; the top-level `code` is then `validation_failed`, or the field code itself when only one rule broke.

## General

| Code | HTTP | Meaning |
|---|---|---|
| `validation_failed` | 422 | One or more fields are invalid; see `errors[]` |
| `required` | 422 (field) | A mandatory field is missing (BR-EM-13, org-chosen mandatory fields) |
| `unknown_field` | 422 (field) | The request has a field the route doesn't accept |
| `invalid_value` | 422 (field) | Wrong type or format for the field |
| `not_found` | 404 | No such record, **or** it belongs to another organisation, **or** it's outside the caller's scope |
| `forbidden` | 403 | The record is visible, but the caller lacks the permission for this action |
| `organisation_suspended` | 403 | Organisation suspended: read and export only (VC-R4) |
| `precondition_required` | 428 | `If-Match` header missing on an update |
| `stale_version` | 412 | Someone else changed the record; `meta.current_version` given (BR-EM-25) |
| `idempotency_key_reused` | 422 | Same `Idempotency-Key` with a different request body |
| `request_in_progress` | 409 | Same `Idempotency-Key` while the first request is still running |
| `rate_limited` | 429 | Too many requests; `Retry-After` header given |
| `internal_error` | 500 | Unexpected failure; nothing was changed; quote `correlation_id` |

## Sign-in and access

| Code | HTTP | Meaning |
|---|---|---|
| `invalid_credentials` | 401 | Wrong organisation, code, PIN, password or one-time code. Always the same code, so nothing is revealed |
| `account_locked` | 423 | 5 wrong attempts; `meta.locked_until` (AC-EM-19.6) |
| `pin_expired` | 401 | Temporary PIN older than 7 days; HR must reissue (edge case 18) |
| `pin_change_required` | 403 | Signed in with a temporary PIN; set your own first (AC-EM-19.3) |
| `pin_too_weak` | 422 | Not 6 digits, or all-same / sequential digits |
| `password_too_weak` | 422 | Admin password doesn't meet the policy |
| `invite_invalid_or_expired` | 410 | Admin invitation used or older than 7 days |
| `session_revoked` | 401 | Refresh token revoked, expired or reused |
| `unauthenticated` | 401 | Access token missing, invalid or expired; the client refreshes it (A8) and retries. Added in Phase 5 |
| `mobile_app_not_in_plan` | 403 | App sign-in on a plan without the mobile app (MD-02) |
| `self_edit_disabled` | 403 | EM-03 is off: contact details are read-only for employees |
| `already_has_access` | 409 | Portal access already granted |
| `no_access` | 409 | Employee has no portal access |
| `employee_not_current` | 409 | Exited employee: portal access can't be granted |
| `pin_already_issued` | 409 | Retry of a PIN-issuing request; use PIN reset |

## Organisation structure

| Code | HTTP | Meaning |
|---|---|---|
| `short_code_in_use` | 409 | Branch short code already used in the organisation (BR-OR-01) |
| `invalid_short_code` | 422 | Not 2–6 capital letters or digits |
| `unknown_time_zone` | 422 | Time zone not recognised (BR-OR-03) |
| `code_in_use` | 409 | List or contractor code already used (BR-OR-01) |
| `name_in_use` | 409 | Payroll group or role name already used |
| `label_in_use` | 409 | Custom field label already used |
| `lists_not_empty` | 409 | Sector template can only be applied to empty lists |
| `invalid_cycle` | 422 | Pay-cycle fields don't match the pay frequency (DBQ-02) |
| `group_has_members` | 409 | Payroll group has had members; can't be deleted (AC-OR-05.5) |
| `cannot_delete_default` | 409 | The default payroll group can't be deleted |
| `invalid_setting` | 422 (field) | Setting value not allowed by the settings schema |
| `invalid_pattern` | 422 | Employee-code pattern can't produce codes |
| `options_required` | 422 | Dropdown and multiple-choice fields need options |
| `option_inactive` | 422 (field) | Chosen option has been deactivated |
| `cannot_change_type_with_values` | 409 | A custom field with values can't change type |

## Employees

| Code | HTTP | Meaning |
|---|---|---|
| `employee_code_in_use` | 409 | Code used by another person in the organisation (BR-EM-01) |
| `mobile_in_use` | 409 | Mobile belongs to a current employee; `meta.employee_code` names them (BR-EM-09) |
| `enrolment_id_in_use` | 409 | Enrolment ID held by another employee on those dates; `meta.employee_code` (BR-EM-24) |
| `employee_limit_reached` | 409 | VC-01 = block and the plan limit is reached; `meta.limit` (BR-EM-12) |
| `manager_loop` | 422 | Reporting manager would be the employee or create a loop (BR-EM-11) |
| `contractor_required` | 422 | EM-11 = yes and a contract employee has no contractor (BR-EM-26) |
| `date_in_locked_period` | 409 | Effective date falls in a locked period (BR-EM-06) |
| `date_before_joining` | 422 | Date earlier than the joining date |
| `date_before_last_exit` | 422 | Rejoin date not after the previous last working day |
| `no_change` | 422 | The job change changes nothing |
| `not_scheduled` | 409 | The change has already taken effect; raise a new change instead |
| `not_in_probation` | 409 | Probation end date can only change while in Probation |
| `not_exited` | 409 | Only an exited employee can rejoin |
| `status_request_pending` | 409 | Decide the pending status change first (edge case 7) |

## Status changes

| Code | HTTP | Meaning |
|---|---|---|
| `request_already_pending` | 409 | One pending change per employee (BR-EM-07) |
| `transition_not_allowed` | 422 | Move not allowed by SM-1 / EM-R5 |
| `exit_details_required` | 422 | A change to Exited needs last working day and exit reason |
| `cannot_decide_own_request` | 403 | Nobody approves their own request (BR-EM-16) |
| `wrong_approver_role` | 403 | This request is for the Org Admin, or for an HR Manager if the Org Admin raised it (BR-EM-27) |
| `not_pending` | 409 | The request has already been decided or withdrawn |

## Roles and users

| Code | HTTP | Meaning |
|---|---|---|
| `fixed_permission` | 422 | That permission can't be granted or removed |
| `exceeds_own_rights` | 403 | You can't grant rights or scope wider than your own (BR-EM-29) |
| `last_org_admin` | 409 | The organisation must keep at least one Org Admin (BR-EM-29) |
| `custom_roles_disabled` | 403 | AC-06 is off |
| `email_in_use` | 409 | Admin email already used on the platform (DBQ-01) |

## Bulk import

| Code | HTTP | Meaning |
|---|---|---|
| `file_unreadable` | 422 | Not a readable .xlsx or .csv |
| `wrong_template` | 422 | Columns don't match the template |
| `file_too_large` | 413 | Over 5 MB or 5,000 rows |
| `link_expired` | 410 | A file link past its time limit, or an import error report older than 30 days. Added in Phase 5 |

Row-level errors in the import error report use the same codes as the screen.

---

## Package 2: Attendance (*P2*, approved at Gate 4, 24 November 2026)

Reused from Package 1 with the same meaning: `code_in_use`, `name_in_use`, `not_pending`, `cannot_decide_own_request`, `request_already_pending`, `invalid_setting`, `stale_version`, file and import codes.

### General attendance

| Code | HTTP | Meaning |
|---|---|---|
| `feature_disabled` | 403 | The setting in `meta.setting` switches this action off (SH-12, RG-01, HD-01, LA-17, PC-01…) |
| `date_locked` | 409 | The date is in a locked lock unit; step it back first. `meta.lock_unit_id`, `meta.state` (BR-AT-18) |
| `outside_employment` | 422 | Date before joining or after the last working day (BR-EM-22) |
| `wrong_approver` | 403 | You are not the current approver of this request |
| `already_voided` | 409 | The record has already been voided |
| `reason_required` | 422 | This action needs a reason |

### Shifts, rosters, holidays

| Code | HTTP | Meaning |
|---|---|---|
| `night_flag_required` | 422 | The shift ends at or before it starts; tick the night flag (DEC-23) |
| `core_outside_band` | 422 | Core hours must fall inside the flexible band |
| `shift_inactive` | 422 | The shift is deactivated |
| `shift_not_allowed_at_branch` | 422 | The shift belongs to another branch |
| `shift_not_allowed` | 422 | The shift isn't allowed for this employee |
| `date_not_future` | 422 | Shift requests are for future dates only |
| `colleague_not_eligible` | 422 | The colleague isn't at the same branch that day, or the shifts aren't allowed for both |
| `date_outside_year` | 422 | Holiday date isn't in the calendar's year |
| `holiday_exists_on_date` | 409 | The calendar already has a holiday on that date |
| `pick_limit_reached` | 422 | All optional holidays allowed this year (HD-01) are used |
| `holiday_not_optional` | 422 | Only optional holidays can be picked |
| `not_in_my_calendar` | 422 | The holiday isn't in your branch's calendar |
| `already_picked` | 409 | You have already picked this holiday |
| `date_passed` | 409 | The date has passed; it can no longer be cancelled |

### Devices, fences, punches

| Code | HTTP | Meaning |
|---|---|---|
| `serial_in_use` | 409 | A device with this serial is already registered (INV-AT-04) |
| `token_already_issued` | 409 | Retry of a device registration; rotate the token instead |
| `device_inactive` | 409 | The device is deactivated |
| `fence_limit_reached` | 409 | A branch can have at most 10 fences |
| `device_not_bound` | 403 | Punches are accepted only from the phone bound to you; ask HR to reset it (DEC-20) |
| `no_binding` | 409 | The employee has no bound phone |
| `location_required` | 422 | Location is required to punch (PC-02, PC-13) |
| `outside_fence` | 422 | You are outside every allowed fence and the organisation blocks such punches (PC-04) |
| `poor_accuracy` | 422 | Location accuracy is worse than allowed and the organisation blocks such punches (PC-14) |
| `ip_not_allowed` | 403 | Web punches are allowed only from the organisation's network (PC-05) |
| `selfie_required` | 422 | A selfie is required (PC-02) |
| `invalid_adjudication` | 422 | The adjudication changes nothing or has no earlier exclusion to undo |

### Rules, regularisation, periods

| Code | HTTP | Meaning |
|---|---|---|
| `cannot_deactivate_default` | 409 | The default policy set can't be deactivated |
| `policy_set_in_use` | 409 | The policy set has active assignments or employees |
| `assignment_overlaps` | 409 | Another active assignment covers the same target and dates (INV-AT-05) |
| `window_passed` | 422 | The date is older than the regularisation window (RG-04) |
| `limit_reached` | 422 | Regularisation limit for the period reached and RG-08 = block |
| `attachment_required` | 422 | This request type needs an attachment |
| `not_approved` | 409 | Only an approved request can be cancelled |
| `target_required` | 422 | A waiver needs a branch or a shift |
| `range_too_long` | 422 | The date range is longer than allowed |
| `checklist_not_clear` | 409 | Pending items block the lock (PL-05); `meta` has the checklist |
| `period_not_ended` | 409 | The period hasn't ended yet |
| `wrong_state` | 409 | The lock unit isn't in a state this action applies to |
| `period_not_locked` | 409 | Payroll input needs a locked period |
