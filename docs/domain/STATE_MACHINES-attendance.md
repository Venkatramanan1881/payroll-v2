# State Machines — Package 2: Attendance

Phase 2 artefact (SKILL.md §4). Revision 1 · 24 November 2026 · Status: **Approved at Gate 2 on 24 November 2026.**
Model: `DOMAIN_MODEL-attendance.md`. Package 1 machines (SM-1 to SM-7) in `STATE_MACHINES.md` are unchanged. Roles and permission codes: `PERMISSION_MATRIX.md` (Package 2 section).

Common to all machines (same as Package 1):
- **Idempotency:** repeating a transition that has already happened returns the current state and changes nothing.
- **Concurrency:** each record carries a version number; a transition based on an old version is refused as a conflict.
- **Audit:** every transition writes an audit entry in the same transaction (BR-AT-23).
- **Recalculation:** any transition that changes a day's inputs queues that day in the same transaction (outbox).
- **Lock:** a transition that would change a locked date either is refused (settings and schedules) or posts an Adjustment (punches, decisions, corrections) — BR-AT-18.

Raw punches, manual punches, adjudications, day overrides and adjustments have **no lifecycle**: they are append-only records (ATQ-01). Day *statuses* (present, half day, absent…) are values on a result version, not states.

---

## SM-AT-1 Settings version (PolicySetVersion; also ShiftVersion and PeriodSettings)

States: **Draft → Pending approval → Scheduled → Active → Superseded**; **Pending approval → Rejected → (edit) Draft**; **Draft / Scheduled → Discarded**.

| Transition | Actor | Preconditions | Effect |
|---|---|---|---|
| create → Draft | HR Manager or Org Admin (`attendance.policy.manage`) | Values pass validation (BR-AT-14 etc.) | Nothing changes results |
| Draft → Pending approval | Same | Maker-checker on (AC-04) | Approver told |
| Draft → Scheduled | Same | Maker-checker **off**. Effective date not in a locked period | — |
| Pending → Scheduled | Org Admin (`attendance.policy.approve`) | Approver ≠ author 🔒. Effective date still not locked | — |
| Pending → Rejected | Org Admin | Reason | Author told |
| Scheduled → Active | System, at the effective date | — | Previous version → Superseded; PolicyChanged; affected open days recalculated |
| Scheduled → Discarded | Author or Org Admin | Not yet Active | — |

Effective today or earlier (back-dated into open periods, ATT-19): Scheduled → Active at once.

## SM-AT-2 Device

States: **Active ⇄ (online / offline, derived)**, **Active → Inactive** (terminal).

| Transition | Actor | Preconditions | Effect |
|---|---|---|---|
| register → Active | HR Manager (`attendance.device.manage`) | Serial unused in the organisation | Token shown once |
| rotate token | HR Manager | Active | Old token rejected at once; new one shown once |
| online → offline | System | No contact for PC-10 minutes | DeviceOffline, one alert |
| offline → online | System | Contact received | DeviceOnline |
| Active → Inactive | HR Manager | — | Token rejected; device kept for history |

## SM-AT-3 Geo-fence

States: **Draft → Active → Inactive**. Editing an Active fence adds a version (valid from now); the state stays Active.

| Transition | Actor | Preconditions | Effect |
|---|---|---|---|
| create → Draft | HR Manager (`attendance.fence.manage`) | Radius 20–5,000 m; ≤ 10 fences per branch | Can be tested, not used for punches |
| Draft → Active | HR Manager | — | Used from now |
| Active → Inactive | HR Manager | — | Not used for punches from now; old punches keep their check |

## SM-AT-4 Flagged punch review

States: **Pending → Approved**, **Pending → Rejected**. Both final (DQ-AT-04).

| Transition | Actor | Preconditions | Effect |
|---|---|---|---|
| open → Pending | System | Punch stored with a flag needing a decision | Punch counts provisionally (DEC-18); NT-08 |
| Pending → Approved | Anyone with `attendance.punch.review` and the employee in scope | Actor ≠ the person who punched 🔒 | Flag cleared |
| Pending → Rejected | Same | Reason | Exclude adjudication; day recalculated (or Adjustment if locked) |

## SM-AT-5 Regularisation request

States: **Pending (level 1) → Pending (level 2) → Approved**, **Pending → Escalated → …**, **→ Rejected**, **→ Rejected (auto)**, **→ Withdrawn**, **Approved → Cancelled**.

| Transition | Actor | Preconditions | Effect |
|---|---|---|---|
| raise → Pending | Employee (own) or HR on behalf (`attendance.regularisation.request`) | Type enabled; date within RG-04 days and not locked; under RG-03 limit or RG-08 = HR only (routes to HR) | Approver told. RG-06 type → Approved at once |
| Pending → Pending (level 2) | Level-1 approver | RG-05 = two levels | HR told |
| Pending → Approved | Approver at the last level (`attendance.regularisation.approve`) | Approver ≠ raiser 🔒; date still not locked | Manual punches / adjudications written; day recalculated |
| Pending → Rejected | Approver | Reason | — |
| Pending → Escalated | System | No action within RG-07 days, RG-07 = escalate | Next approver (HR, then Org Admin) |
| Pending → Approved / Rejected (auto) | System | RG-07 = auto-approve / auto-reject | As above, actor "system" |
| Pending / Escalated → Withdrawn | Raiser | Before a decision | — |
| Approved → Cancelled | HR Manager (`attendance.regularisation.cancel`) | Reason; date not locked | Its manual punches voided, adjudications reversed; day recalculated |

## SM-AT-6 Optional holiday pick

States: **Pending → Approved / Rejected / Withdrawn**; **Approved → Cancelled**.

| Transition | Actor | Preconditions | Effect |
|---|---|---|---|
| pick → Pending | Employee | Within HD-01 for the year; holiday is optional and in the branch calendar | Manager told |
| Pending → Approved / Rejected | Manager (`attendance.optional_holiday.approve`) | Approver ≠ employee 🔒 | Approved: the date is a holiday for that employee; recalculated |
| Pending → Withdrawn | Employee | — | — |
| Approved → Cancelled | Employee | Before the date | Working day again; pick freed |

## SM-AT-7 Shift change or swap request

States: **Pending → Approved / Rejected / Withdrawn**. Only when SH-12 is on.

| Transition | Actor | Preconditions | Effect |
|---|---|---|---|
| raise → Pending | Employee | Future date; shift(s) allowed for both people (swap) | Manager told; colleague told (swap) |
| Pending → Approved | Requester's manager | Approver ≠ requester 🔒; dates still open | Roster entries written for both sides |
| Pending → Rejected / Withdrawn | Manager / requester | — | — |

## SM-AT-8 Day result

States: **Provisional → Finalised → Locked**, with **Finalised → Provisional** when an input changes before lock.

| Transition | Actor | Preconditions | Effect |
|---|---|---|---|
| calculate → Provisional | System | Date within employment (BR-EM-22) | New version |
| Provisional → Finalised | System (hourly Finaliser) | Branch-local time past latest window end + 1 h (§3.26) and no open review, request or missed-punch flag | — |
| Finalised → Provisional | System | An input changed (late punch, decision, settings) | New version |
| → Locked | System | Its lock unit leaves Open | Further changes → Adjustments (shadow) |
| Locked → Provisional / Finalised | System | Its lock unit steps back to Open | Recalculated |

## SM-AT-9 Lock unit (period × branch)

States: **Open → Attendance locked → Finalised → Marked as paid**; each can step back one state.

| Transition | Actor | Preconditions | Effect |
|---|---|---|---|
| Open → Attendance locked | HR Manager with the branch in scope (`attendance.period.lock`), or system (PL-04 automatic) | PL-05 checklist rule; period end has passed | Period-end rules run; results frozen; payroll input produced; NT-09 |
| Locked → Finalised | HR Manager (`attendance.period.lock`) | — | (V2: final pay values) |
| Finalised → Marked as paid | HR Manager | — | — |
| any → one step back | Org Admin (`attendance.period.step_back`) 🔒 | Reason (DQ-AT-02) | Back to Open: affected days recalculated, and Adjustments already posted from this period's shadows stay (a new shadow difference corrects them) |

"Lock organisation" / "lock branch" = the same transition on every matching unit, in one request; units that fail their checklist are listed and stay Open (DEC-49).

## SM-AT-10 Imports (roster upload, punch import)

Same as Package 1 SM-7 (bulk import): **Received → Processing → Completed** (with row outcomes) / **Failed**.

## SM-AT-11 Waiver

States: **Active → Withdrawn**. Both transitions by HR Manager (`attendance.waiver.manage`); each recalculates the affected days.
