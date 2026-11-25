# State Machines — Package 1: Employee Master

Phase 2 artefact (SKILL.md §4). Status: **Approved at Gate 2, 22 November 2026.**
Every independent lifecycle has its own machine (SKILL.md §4 Phase 1: no shared generic "status" field). Roles are the built-in ones. A transition is allowed only if the actor holds the permission in PERMISSION_MATRIX.md **and** has the employee in scope.

Common to all machines:
- **Idempotency:** repeating a transition that has already happened (e.g. approving twice) returns the current state and changes nothing.
- **Concurrency:** each record carries a version number. A transition based on an old version is refused as a conflict (BR-EM-25).
- **Audit:** every transition writes an audit entry in the same transaction (BR-EM-17).

---

## SM-1 Employee status (StatusPeriod)

States: **Probation, Active, Notice, Long leave, Absconding, Exited.** A new employee starts in Probation or Active (AC-EM-01.1). Every move needs an approved StatusChangeRequest (SM-2).

| From \ To | Probation | Active | Notice | Long leave | Absconding | Exited |
|---|---|---|---|---|---|---|
| **Probation** | — | ✔ confirm | ✔ | ✔ | ✔ | ✔ |
| **Active** | ✘ | — | ✔ | ✔ | ✔ | ✔ |
| **Notice** | ✘ | ✔ withdrawn | — | ✘ | ✔ | ✔ |
| **Long leave** | ✘ | ✔ return | ✔ | — | ✔ | ✔ |
| **Absconding** | ✘ | ✔ returned (LM-32) | ✔ | ✘ | — | ✔ |
| **Exited** | ✘ | ✘ | ✘ | ✘ | ✘ | — (terminal) |

- **Exited is terminal** for that employment period. A rejoin opens a new period (SM-5), starting in Probation or Active.
- **Side effects:**
  - → Exited closes the employment period and emits EmployeeExited. Access is revoked from the day after the last working day (BR-EM-08), and the mobile number and enrolment ID are freed.
  - Probation → Active stops probation reminders.
  - → Absconding emits StatusChanged. Package 2 uses it to stop absconding alerts.
- **Automatic:** none. The attendance module may *suggest* Absconding (DEC-37), but a request is still needed.

## SM-2 Status change request

States: **Pending → Approved → (Scheduled →) Effective**, or **Pending → Rejected**, or **Pending → Withdrawn**, or **Scheduled → Cancelled**.

| Transition | Actor | Preconditions | Effect |
|---|---|---|---|
| raise → Pending | HR Manager or Org Admin | No other pending request for the employee. Move allowed by SM-1. Effective date not in a locked period. Exit fields filled if to = Exited. | Status unchanged |
| Pending → Approved | Org Admin; or HR Manager if the Org Admin raised it | Approver ≠ raiser (INV-06). Checks re-run at decision time. | If effective date ≤ today: StatusPeriod added, state becomes **Effective**. Otherwise **Scheduled**. |
| Pending → Rejected | Same as approve | Reason required | Nothing changes |
| Pending → Withdrawn | The raiser | — | Nothing changes |
| Scheduled → Effective | System, daily at 00:15 branch time | Date reached | StatusPeriod added; events emitted |
| Scheduled → Cancelled | Org Admin or HR Manager (in scope), with reason (DQ-04) | Before effective date | Nothing changes |

- Terminal states: Effective, Rejected, Withdrawn, Cancelled.
- Illegal: deciding a request that is not Pending; cancelling one already Effective (raise a new request instead).
- The scheduled job is safe to run twice (AT21).

## SM-3 Job version (scheduled job changes)

Job changes need no approval (BRD EM-R6). A version is **Effective** (start ≤ today), **Scheduled** (start in the future), **Superseded**, or **Cancelled**.

| Transition | Actor | Preconditions | Effect |
|---|---|---|---|
| save with past date → Effective | HR Manager | Date in open period (INV-10). Date ≥ joining date. | Supersedes the versions it replaces; emits JobChanged(from date) |
| save with today/future date → Effective / Scheduled | HR Manager | — | Scheduled versions show as "scheduled" |
| Scheduled → Effective | System (date reached) | — | Emits JobChanged |
| Scheduled → Cancelled | HR Manager, with reason | Before start | Version kept, marked Cancelled |
| any → Superseded | System, as part of a later back-dated save | — | Version kept, marked Superseded with recorded time |
| Scheduled → Cancelled (automatic) | System | An exit or transfer effective earlier makes it invalid (edge case 11) | HR is notified |

## SM-4 Profile change request

**Removed (DQ-05).** With EM-03 on, an employee's contact edit applies at once in one step. The mobile number uniqueness check (BR-EM-09) still runs, and the change is audited. There is no request lifecycle.

## SM-5 Employment period

**Open → Closed.** Opened when the employee is created or rejoined; closed when an exit becomes Effective. At most one Open period (INV-04). A Closed period is never reopened; a rejoin opens a new one on the same employee, keeping the employee code (DQ-07). A rejoin is refused while a status request is still pending. The probation end date can be extended while the period is Open (DQ-01). If an exit is reversed by mistake, the fix is a new correction with an audit reason, handled by an Org Admin.

## SM-6 Employee sign-in credential

States: **Not issued, Temporary, Active, Locked, Expired, Revoked.**

| From | Event | To | Actor |
|---|---|---|---|
| Not issued | HR issues temporary PIN | Temporary | HR Manager |
| Temporary | Employee signs in and sets own PIN | Active | Employee |
| Temporary | 7 days pass | Expired | System |
| Expired | HR reissues | Temporary | HR Manager |
| Active | 5 wrong PINs or codes | Locked (15 min) | System |
| Locked | 15 minutes pass | Active | System |
| Active / Locked | HR resets PIN | Temporary (all sessions revoked) | HR Manager |
| any | Exit becomes Effective | Revoked | System |
| Revoked | Rejoin, HR issues PIN | Temporary | HR Manager |

- "Not issued" is the permanent state for employees with portal access "none" (default, EM-R15). Removing portal access from any state goes to **Revoked** (all sessions end). Granting it again goes to Temporary.
- Plan without mobile app (MD-02): web sign-in only.

## SM-7 Bulk import

**Uploaded → Validating → Applying → Completed** (with an applied count and an error count) or **→ Failed** (file unreadable, wrong template).

- The same file (same checksum) uploaded again is recognised, and rows already applied are skipped (AC-EM-15.5).
- Completed is terminal. The error report stays downloadable for 30 days (TSD §11 export retention).

## SM-8 Reference item, branch, contractor, payroll group

**Active ⇄ Inactive.** Deactivating something that's in use doesn't change existing employees. It only removes the item from new choices. Never deleted once used (BR-OR-02). A payroll group can be deleted only if it never had members.

## SM-9 Organisation (Platform, minimal)

**Active ⇄ Suspended.** While suspended, users can only view and export (VC-R4). Owned by the vendor console. Here it only matters for refusing People writes.
