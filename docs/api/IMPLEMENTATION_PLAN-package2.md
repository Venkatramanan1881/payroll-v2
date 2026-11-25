# Implementation Plan — Package 2: Attendance (Phase 5)

Revision 1 · 24 November 2026. Contracts approved at Gate 4 ([package2-attendance.md](API_CONTRACTS/package2-attendance.md)). Gate 5 = every slice below done with its tests passing on the dev database.

Code layout follows Package 1: pure rules in `crates/core` (no HTTP, no SQL), routes and transactions in `crates/api` (one module per area), migrations in `crates/db`. Each slice is a vertical cut: core rules + unit tests, routes + an end-to-end test against the dev database, audit and outbox in the same transaction.

| # | Slice | Routes / jobs | Core rules (unit-tested) | Done when |
|---|---|---|---|---|
| S0 ✅ | Groundwork | Apply 0017; J1 queues one task per organisation (APQ-05); real `PeriodLockQuery` replacing the Package 1 stub; shared settings-version helper (§1.6) | Lock lookup; settings-version state view (scheduled / active) | `date_in_locked_period` reads lock units; J1 run time no longer grows with organisations |
| S1 ✅ | Shifts and roster | SH1–SH5, R1–R5, R7 | Shift validation (night flag, flex band, split); schedule resolver (roster → default shift); rotation generator | Roster built, copied, rotated; locked dates refused |
| S2 ✅ | Holidays | H1–H10 | Calendar copy; optional-pick limit | Picks approved by someone other than the employee; one calendar per branch-year |
| S3 ✅ | Policy sets | PS1–PS8 | Settings schema with defaults and the AC-AT-21.5 cross-checks; policy resolver (employee → type → category → department → branch → default) | Maker-checker path; resolved set shown on the employee |
| S4 | Devices, fences, phone binding | D1–D5, F1–F5, P3 | Fence distance; token hashing | Token shown once, rotation stops the old one |
| S5 ✅ | Punching | P1, P2, P4–P9 (+ F1–F5, P3 binding pulled from S4) | Punch time rules (DEC-21, §3.4); fence check (PC-04/14/16); flags | Offline repeat makes no duplicate; flagged punch reviewed by someone else; raw punch never changes |
| S6 ✅ | Day engine | (library only, `core::day`) | §7 steps: sessions (PR), breaks (BK), late (LA), early (EC/EA), missed punches (MP), day status and rounding (DS/RD), holidays (HD-03), unapproved absence (UA). Built in sub-slices, each with table-driven tests from the BRD examples (E1–E18, W1–W11) | Every worked example in the BRD gives the documented result |
| S7 ✅ | Recalculation | J-AT2, J-AT1, J-AT3, DY1–DY3, M2, M3, M1 | Input fingerprint; version rules | A punch updates its day within seconds; no older result overwrites a newer one |
| S8 ✅ | Regularisation and corrections | RG1–RG6, C1–C2, W1, O1 | Approver routing; window and limit checks | Two-level approval, escalation, cancellation reverse the day |
| S9 | Periods and lock | L1–L9, J-AT5, J-AT7 | Period generation; checklist; shadow recalculation and adjustments; period-end penalties | Lock, step back, correction after lock posts to the next period; payroll input file |
| S10 ✅ | Imports, notifications, reports | P10, R6, J-AT6, N1–N2, J-AT8, J-AT9, RP1–RP8, DB1 (J-AT4 moved to S4, on hold) | Import row validation | Reports respect scope and export |
| S11 | Device gateway | G1 | eSSL push parsing | **Blocked by PUN-01** (device test) |

Order: S0 → S1–S4 (independent of each other) → S5 → S6 → S7 → S8 → S9 → S10. S6 is the largest; it can start in parallel with S1–S5 because it is pure code.
