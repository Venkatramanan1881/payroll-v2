# Open Issues Register — Payroll App
Version 0.1 · 21 November 2026
Sources reviewed: BRD v0.8 (draft), TSD v0.2 (draft), `Skills/SKILL.md` (payroll-saas-engineering)
Phase: Phase 0 — Product and requirements analysis (pre Approval Gate 1)

---

## 0. How to read this

| Field | Meaning |
|---|---|
| **Type** | **Conflict** (two sources disagree) · **Gap** (behaviour not defined) · **Ambiguity** (defined but readable two ways) · **Risk** · **Decision** (known open decision) |
| **Sev** | **Blocker** (stops Gate 1 / design) · **High** (wrong pay or rework if guessed) · **Medium** · **Low** |
| **Owner** | Product (Venkat / BA) · Tech (architect) · Legal (labour-law / privacy consultant) · Vendor ops |

Nothing in this register is a decision. Proposed resolutions are recommendations only and need approval before they become requirements (SKILL §4 Phase 0: "Never convert an assumption into a requirement without approval").

---

## 1. Top 12 — resolve before anything else

| # | ID | One line |
|---|---|---|
| 1 | GOV-01 | BRD and TSD are both "Draft, not yet approved" and there is no sign-off owner (Q14). Gate 1 cannot pass. |
| 2 | CON-01 | Mobile stack: Skill says React Native + Expo; BRD §22.1 and TSD say Flutter ("decided"). |
| 3 | CON-03 | Offline punches: Skill says offline sync is a later, separately approved phase; BRD PC-R5/N4 and TSD §9.3 put it in V1. |
| 4 | ATT-01 | Default reading mode "paired" is undefined and, for untagged devices, collapses into odd/even parity — which the Skill prohibits. |
| 5 | PER-01 | PL-06 allows unlocking a locked period; BR-06 says a locked period never changes. |
| 6 | REL-01 | V1 has attendance periods and "payroll period" counters, but pay periods are defined per payroll group, which arrives in V2. |
| 7 | ATT-10 | Overlapping "absent with no leave" settings: DS-10, LM-28, LV-04 and LA-12 can each decide the same day differently. |
| 8 | ATT-12 | BR-04 "larger applies once" cannot compare unlike penalties (half day vs ₹ amount vs minutes). |
| 9 | PAY-01 | Rounding granularity of a "line" is undefined; P6/P7 totals change depending on it (₹3,461.54 vs ₹3,461.52). |
| 10 | PAY-03 | Monthly salary minus (deduction days × per-day rate) can go negative or pay for zero days depending on the divisor. |
| 11 | SEC-01 | Device gateway authenticates eSSL devices by serial number only — a guessable, non-secret value. |
| 12 | CR-01 | Mechanism for client-specific payroll rules (code, plug-in, DSL?) is undefined and drives the whole V2 architecture. |

---

## 2. Governance and process (GOV)

| ID | Type | Sev | Issue | Source | Proposed resolution / question | Owner |
|---|---|---|---|---|---|---|
| GOV-01 | Gap | Blocker | Both upstream documents are drafts; Q14 "who signs off this BRD?" is open. Skill precedence (§12) ranks *approved* BRD/TSD above skill defaults, so today no conflict can be settled by document authority. | BRD header, Q14; TSD header; SKILL §12 | Name approvers for BRD and TSD; record approval date and version. | Product |
| GOV-02 | Gap | High | Two decision numbering schemes: TSD §20 uses D1–D8, change log cites D-12 and D-14. No decision log / ADR register exists in the repo. | TSD change log, §20 | Create `docs/architecture/ADR/` (or a single decision log) and migrate D-12, D-14 and the §22.1 stack decisions into it. | Tech |
| GOV-03 | Gap | High | Phase 0 artefacts required by the Skill do not exist: user stories, story-level acceptance criteria, business-rules catalogue, permission matrix, glossary, traceability matrix. BRD AT1–AT21 are system-level only. | SKILL §4 Phase 0, §8 | Produce these next, keyed to BRD IDs (PC-R5, LA-05 …). | Product + BA |
| GOV-04 | Gap | Medium | Success measures in §2 are "proposed (to confirm)". "95% of employee-days finalised with no manual action" has no definition of "finalised" or "manual action". | BRD §2 | Define, and tie to a day lifecycle (see DM-03). | Product |
| GOV-05 | Gap | Medium | All BRD defaults are "proposals for review" (§7). No list of which defaults are confirmed. | BRD §7 | Add a "confirmed" flag to each setting in the settings catalogue. | Product |

---

## 3. Conflicts between the Skill and BRD/TSD (CON)

The Skill (§3) says: *"If the repository or approved TSD contradicts this section, stop and report the conflict. Do not silently choose one."* These are those conflicts.

| ID | Type | Sev | Issue | Source | Proposed resolution / question | Owner |
|---|---|---|---|---|---|---|
| CON-01 | Conflict | Blocker | **Mobile framework.** Skill: React Native with Expo. BRD §22.1 and TSD §2/§13: Flutter, marked "decided". | SKILL §3, §4 Ph7; BRD §22.1; TSD §2, §13 | Pick one; record ADR; update the other document. (TSD is newer and more specific; if Flutter stands, the Skill must be edited.) | Tech |
| CON-02 | Conflict | High | **Async messaging.** Skill: Google Pub/Sub with transactional outbox and idempotent consumers. TSD: Cloud Tasks + Cloud Scheduler, no outbox. TSD flow "raw punch saved → task queued" is a dual write: if enqueue fails after commit, the recalculation is silently lost. | SKILL §3 Arch rules; TSD §3.2, §10 | Either adopt an outbox table drained to Cloud Tasks, or add a sweeper that re-queues days whose inputs changed after their last result. Record which. | Tech |
| CON-03 | Conflict | Blocker | **Offline punches.** Skill §4 Ph10: offline sync is "a later, explicitly scoped capability. Do not build it by default." BRD PC-R5, N4 and TSD §9.3/§13 put it in V1. | SKILL Ph10; BRD PC-R5, N4; TSD §9.3 | Confirm V1 scope. If V1, the Skill Ph10 checklist (conflict rules, clock drift, max offline age, local encryption, expiry) becomes a V1 design input. | Product + Tech |
| CON-04 | Ambiguity | Medium | **Cache.** TSD §4.2 says "caches … are keyed by organisation" but no cache is chosen. Skill: Redis only for a concrete requirement. | TSD §4.2; SKILL §3 | State whether any shared cache exists in V1. If not, delete the line. | Tech |
| CON-05 | Ambiguity | Medium | **Service boundaries.** Skill wants domain-oriented services with explicit aggregate ownership. TSD: one API service, one gateway, jobs, one DB, one Rust workspace. Not a contradiction, but aggregate ownership is not defined anywhere. | SKILL §3, Ph1; TSD §3.1, §5.1 | Phase 1 domain model to name owning module per aggregate even inside a modular monolith. | Tech |
| CON-06 | Ambiguity | Medium | **Raw punch corrections.** BR-01: "A correction is a new punch marked manual." Skill §5.1: corrections and adjudications are separate auditable records. A new manual punch can add evidence but cannot neutralise a *wrong* punch (RG-02 "wrong punch") without deleting or ignoring the original. | BRD BR-01, RG-02; SKILL §5.1 | Add a "punch adjudication" record (exclude / reassign date / reassign direction) alongside manual punches. | Product + Tech |
| CON-07 | Conflict | Low | Skill lists Supabase as "managed database/platform" incl. Auth; TSD §8/§17 deliberately excludes Supabase Auth, storage, realtime, edge functions. Consistent with Skill's "explicit decision" rule but the Skill text is stale. | SKILL §3; TSD §8, §17 | Update Skill text after ADR. | Tech |

---

## 4. Inconsistencies between BRD and TSD (DOC)

| ID | Type | Sev | Issue | Source | Proposed resolution / question | Owner |
|---|---|---|---|---|---|---|
| DOC-01 | Conflict | Medium | BRD §22 points to "TSD v0.1"; current TSD is v0.2. | BRD §22 | Update reference. | Product |
| DOC-02 | Conflict | Medium | BRD Q16 (sign-in SMS provider) and Q15 still open in BRD; TSD records Q16 decided (D-12, Firebase) and D4 decided. BRD not updated. | BRD Q15, Q16; TSD §8, §20 | Close Q16 in BRD with reference to D-12. | Product |
| DOC-03 | Conflict | High | **Cost estimate uses the rejected option.** TSD §19.2 still prices sign-in SMS via an Indian provider, while D-12 chose Firebase. Restated (same method, 25% contingency): A ≈ **$104** (was $95), B ≈ **$801** (was $476), C ≈ **$3,109** (was $1,379, ≈ ₹5.9/employee instead of ₹2.6). | TSD §19.2, §19.3 | Re-issue §19 with Firebase pricing, or revisit D-12. | Tech |
| DOC-04 | Gap | High | **D-12 rationale missing.** TSD §8 says the rejected Indian provider is "about 20 times lower" in price; no reason is given for choosing the dearer option (DLT registration effort? deliverability? time to market?). At size C it is larger than all other costs combined. | TSD §8, §19.3 | Record the rationale and a revisit trigger (e.g. at 10,000 employees). | Product + Tech |
| DOC-05 | Conflict | Medium | **Point-in-time recovery start.** BRD TB-08: PITR "once live payroll data is held". TSD §15: "from the Growth size". TSD D8: undecided. V1 attendance totals already drive client pay via export (MD-06). | BRD TB-08; TSD §15, D8 | Decide: PITR from first live client is the safer reading. | Product + Tech |
| DOC-06 | Ambiguity | Medium | **Languages without code change.** BRD N7: new languages "without code changes". TSD §12: translation files (usually a deploy). | BRD N7; TSD §12 | Clarify whether "no code change" means "no code change, but a deploy". | Product |
| DOC-07 | Gap | Medium | **Second factor for admins.** BRD UM-R3 "optional second step", AC-08 on/off (default on). TSD §8/§15 reads as mandatory. Vendor users not mentioned. | BRD UM-R3, AC-08; TSD §8, §15 | Decide whether an org may turn MFA off; make it mandatory for vendor users. | Product |
| DOC-08 | Gap | Medium | **Capacity range.** BRD N2: hundreds of orgs, up to 2,000 employees each (proposed 500 orgs in 2 years ⇒ up to ~1M employees). TSD sizes stop at 50,000 employees. | BRD N2; TSD §18 | Add a size D or state the expected average org size. | Product + Tech |
| DOC-09 | Gap | Low | TSD §16 lists dev/test/live, but only test and live have separate projects; dev isolation and data source unspecified. | TSD §16; BRD TB-09 | State dev data source (synthetic only). | Tech |

---

## 5. Release scope and module dependencies (REL)

| ID | Type | Sev | Issue | Source | Proposed resolution / question | Owner |
|---|---|---|---|---|---|---|
| REL-01 | Gap | Blocker | **What is a period in V1?** C12 delivers attendance periods in V1, but PL-01 pay period is "set per payroll group", and payroll groups are V2. LA-18 default reset is "payroll period"; PM-08 offers "payroll period". | BRD C12, PL-01, LA-18, PM-08, §21 | Define a V1 "attendance period group" (org or branch level), or pull payroll-group *membership* (not pay) into V1. | Product |
| REL-02 | Gap | High | **Overtime identification in V1.** "Overtime hours are identified by attendance from V1" but the settings that define identification (OT-02 basis, OT-03 minimum, OT-06 rounding, OT-07 pre-shift) belong to the V3 module. | BRD M01, §10.1, §21 note 2 | List which OT settings are active in V1 for identification only. | Product |
| REL-03 | Gap | Medium | **HW-01 = "overtime"** (V1 comp-off module) refers to overtime handling that exists only in V3. | BRD HW-01 | State V1 behaviour (store hours only?). | Product |
| REL-04 | Gap | Medium | **Daily allowance referenced before it exists.** OD-09 (V1 setting) "daily allowance earned on an on-duty day"; PG-06 (V2). DA is V3. | BRD OD-09, PG-06, §21 note 3 | Hide or defer these settings until V3. | Product |
| REL-05 | Gap | High | **Advances/loans with Payroll off.** C10 is a common module (V2); MD-04 does not make it depend on Payroll; MD-06/AL-R8 export recoveries when Payroll is off. But AL-08 recovery cap is "% of net pay", which does not exist without Payroll. | BRD C10, MD-04, MD-06, AL-08, AL-R8 | Define cap basis when Payroll is off, or make advances depend on Payroll. | Product |
| REL-06 | Decision | Medium | Q12: overtime pay in V2 only as a manually entered amount vs moving OT/DA to V2. | BRD Q12, §21 note 2 | Decide. | Product |
| REL-07 | Decision | Medium | Q6 reimbursements in V2; Q2 leave encashment in V2; Q5 plan contents. | BRD Q2, Q5, Q6 | Decide. | Product |
| REL-08 | Gap | Medium | **Roles ahead of features.** Supervisor's main duty (daily cash payouts) is V3; Payroll Officer duties are V2; VC-R3/VC-R8 show "last pay run", "failed pay runs" in V1. | BRD §5, VC-R3, VC-R8 | Mark per-role V1 scope. | Product |
| REL-09 | Gap | Medium | **Employees without a smartphone** use the device only, but have no channel to apply for leave, view balances or regularise. HR-on-behalf requests are not specified. | BRD §3, LM-R3, RG | Add "HR raises request on behalf of employee" (with audit), or a kiosk mode (PC-01 lists "kiosk" with no requirements). | Product |
| REL-10 | Gap | Medium | PC-01 lists punch sources "kiosk" and "file or API import" with no requirements (format, auth, idempotency, who may import). | BRD PC-01 | Specify or remove from V1. | Product |
| REL-11 | Gap | Medium | **Work-from-home** appears in PC-09, PC-17, LV-03 but there is no WFH request type, limit or approval flow. | BRD PC-09, PC-17, LV-03 | Add WFH as a request type (or as an on-duty sub-type). | Product |

---

## 6. Punch intake and evidence (PUN)

| ID | Type | Sev | Issue | Source | Proposed resolution / question | Owner |
|---|---|---|---|---|---|---|
| PUN-01 | Decision | Blocker | Q15 / D1: can the eSSL models push over HTTPS, and their real contact rate. TSD says test before building the gateway. Cost model also depends on it (TSD §19.3). | BRD Q15, TB-04; TSD §9.1, D1 | Run the device test; list exact models in use. | Tech |
| PUN-02 | Gap | High | **Unknown enrolment IDs.** Punches whose enrolment ID has no employee mapping yet — stored? where? reprocessed when mapping is added? | BRD PC-R3, PC-R8; TSD §9.1 | Store as unmapped raw punches; reprocess on mapping. Confirm. | Product + Tech |
| PUN-03 | Gap | High | **Who approves flagged punches?** E18 says outside-fence punches are "not counted until the manager approves"; EM-07 "flag for approval". AC-02 approver list doesn't include flagged punches; no state flow, timeout or escalation. | BRD PC-04, EM-07, E18, AC-02 | Add flagged-punch approval as a request type with RG-07-style escalation. | Product |
| PUN-04 | Ambiguity | Medium | Is a "record and flag" punch counted provisionally or excluded until approved? E18 says excluded; PC-06/PC-07/PC-14 flags are silent. | BRD PC-04, PC-06, PC-07, PC-14 | Define per flag type: counts / excluded until approved. | Product |
| PUN-05 | Gap | High | **Offline punch rules** (if CON-03 keeps it in V1): maximum offline age; behaviour when the synced punch falls in a locked period; app clock-mismatch tolerance (PC-08 is device-only); selfie offline; geo-fence evaluated against fence version at punch time. | BRD PC-R5, N4, PC-07, PC-08; TSD §9.3 | Specify. | Product + Tech |
| PUN-06 | Ambiguity | Medium | **Selfie / face punch.** Is the selfie evidence only, or is a face match / liveness check required? BR-02 bans AI only in the calculation path. | BRD PC-R4, PC-02, BR-02 | Decide; if matching is required, it is a new component. | Product |
| PUN-07 | Gap | Medium | **Buddy punching / app trust.** No device binding, rooted-device or mock-location detection for the employee app. Skill Ph7 requires device registration and trust. Long-lived phone sessions (TSD §8) amplify this. | SKILL Ph7; TSD §8, §9.2 | Add device binding and mock-location flag requirements. | Product + Tech |
| PUN-08 | Ambiguity | Medium | PC-R9 "manual attendance" by HR during device failure — are these manual punches, or a day-status override? RG-10 "direct correction by HR" has the same ambiguity. | BRD PC-R9, RG-10, RG-11 | Define both as records distinct from raw punches (see CON-06). | Product |
| PUN-09 | Gap | Medium | "Hold check-in until the manager approves" (LA-05 slab action) cannot block a biometric device punch. Operational meaning undefined. | BRD LA-05 | Clarify: punch stored but day held pending approval? | Product |
| PUN-10 | Gap | Low | PC-08 device clock mismatch tolerance has no action (flag? correct with receipt time?). | BRD PC-08 | Define action. | Product |
| PUN-11 | Gap | Low | Enrolment template sync (pushing an employee's fingerprint to an allowed branch's device) — in scope or manual? | BRD PC-R8, EM-R9 | State. | Product |

---

## 7. Shifts and attendance engine (ATT)

| ID | Type | Sev | Issue | Source | Proposed resolution / question | Owner |
|---|---|---|---|---|---|---|
| ATT-01 | Conflict | Blocker | **"Paired" reading mode** (PR-01 default) is undefined. With untagged device punches it reduces to alternating in/out — punch parity — which the Skill forbids (§5.1, §11). Mixed sources (device + app with explicit direction) on one day also undefined. | BRD PR-01, MP-01; SKILL §5.1, §11 | Specify pairing using shift window, PR-04 minimum gap, PR-05 max session and source direction tags. Approve before engine design. | Product + Tech |
| ATT-02 | Gap | High | **Overlapping shift windows.** SH-07 default 3h before / 6h after; for night shift 22:00–06:00 followed by a day shift 09:00, windows overlap. No tie-break rule for punch-to-date assignment. | BRD SH-07, SH-06; SKILL §5.2 | Define precedence (nearest shift start? rostered shift wins?). | Product |
| ATT-03 | Gap | High | **Open shift** (hours only, no times) has no start/end, so SH-07 window and SH-06 attendance date cannot apply. | BRD SH-01, SH-07 | Define date assignment for open shifts (e.g. calendar day in branch time zone, or first-punch anchoring). | Product |
| ATT-04 | Gap | Medium | **SH-08 auto-detect shift** algorithm (nearest start? within window? tie?) undefined. | BRD SH-08 | Specify. | Product |
| ATT-05 | Gap | Medium | **Split shifts**: late/early per segment or for the day? Half-day point? Break rules between segments? | BRD SH-R1, SH-11, BK | Specify. | Product |
| ATT-06 | Gap | Medium | **Weekly-hours staff** (SH-03 per week) — DS rules are daily; no weekly status rule. | BRD SH-03, DS-01 | Specify weekly evaluation. | Product |
| ATT-07 | Ambiguity | Medium | SH-R1 lists "night shifts" as a type; SH-01 does not (has "roster"). Is night an attribute of any shift? SP-06 "shift marked as night" implies so. | BRD SH-R1, SH-01, SP-06 | Make "night" a shift flag. | Product |
| ATT-08 | Gap | High | **Time zone when punching at another branch.** OR-R1 gives each branch a time zone; EM-R9 lets employees punch at allowed branches. Attendance date computed in which zone — punch branch, master branch, or shift's branch? | BRD OR-R1, EM-R9, BR-11; TSD §5.3 | Decide (master branch zone is simplest). | Product |
| ATT-09 | Gap | High | **Single canonical calculation order.** Three orders exist: §9.6 daily process (overlays at step 7, after late calc at step 6), §12 late-check order (overlays first), Skill §5.2 (11 steps). | BRD §9.6, §12; SKILL §5.2 | Publish one ordered algorithm as a business rule. | Product + Tech |
| ATT-10 | Conflict | Blocker | **Absent-without-leave handled four times.** DS-10 (LOP / auto-debit / pending N days), LM-28 (LOP / auto-debit), LV-04 (debit order), LA-12 (penalty recovery order). Which wins, and do DS-10 and LM-28 duplicate each other? | BRD DS-10, LM-28, LV-04, LA-12, EC-11 | Merge into one setting family with one debit order. | Product |
| ATT-11 | Conflict | Medium | DS-06 quarter-day statuses default **off**, but LA-05/EC-04 slab actions and LA-09/EC-08 penalty sizes offer "quarter day". | BRD DS-06, LA-05, LA-09 | Validate on save, or allow quarter-day cuts independent of DS-06. | Product |
| ATT-12 | Gap | Blocker | **BR-04 "where two rules apply, the larger applies once"** — no way defined to compare a half-day cut with a ₹ fixed amount (LA-09) or actual minutes. PD note only "warns". | BRD BR-04, LA-09, PD note, K4 | Define a comparison basis (convert to ₹ at per-day/per-minute rate?) or block conflicting combinations. | Product |
| ATT-13 | Ambiguity | Medium | **Slab boundaries.** Default late slab 11–120 assumes grace 10. If grace becomes 15, minutes 11–15 fall in both. Inclusive/exclusive edges undefined. | BRD LA-03, LA-05 | Slabs measured on minutes beyond grace, or validated against grace. | Product |
| ATT-14 | Ambiguity | Medium | **MP-03 "cut-off"** — which one? PL-02 attendance cut-off, J1 nightly finalisation, or RG-04 regularisation window? | BRD MP-03, PL-02, RG-04; TSD J1 | Define. | Product |
| ATT-15 | Conflict | Low | Overtime rounding defined twice: RD-04 "applied to overtime" and OT-06. | BRD RD-04, OT-06 | One wins. | Product |
| ATT-16 | Ambiguity | Medium | RD rounding "applied to totals only" — daily net-hours total, or period total? | BRD RD-01–04, BR-11 | Specify. | Product |
| ATT-17 | Gap | Medium | LA-13/LA-14 compensation "same week" and DS-05 "make up within the week" require multi-day re-evaluation; when does the week close relative to period lock and BR-10? | BRD LA-14, DS-05, BR-10 | Specify evaluation timing. | Product |
| ATT-18 | Ambiguity | Low | E10 over-stay minutes counted as late — does grace apply to them? | BRD PM-19, E10 | Specify. | Product |
| ATT-19 | Gap | Medium | **Policy effective dates in the past.** G5 allows new versions with a start date; can it be back-dated into an open period? into a locked one? Triggers bulk recalculation (TSD §10). | BRD G5, BR-03, BR-06; TSD §5.4 | Back-dating allowed only within open periods; confirm. | Product |
| ATT-20 | Gap | Medium | **Mid-period transfers** (branch, shift, policy set, holiday calendar) — counter continuity (late marks, permission count) and which holiday calendar applies to which days. | BRD EM-09, LA-18 | Specify. | Product |
| ATT-21 | Gap | Low | LA-12 default "casual leave, then LOP" assumes a leave type the org may not have. | BRD LA-12 | Template-driven default. | Product |

---

## 8. Leave, permissions, on-duty, holidays, comp-off (LV)

| ID | Type | Sev | Issue | Source | Proposed resolution / question | Owner |
|---|---|---|---|---|---|---|
| LV-01 | Ambiguity | Medium | **Sandwich with two leave types** (LM-20 "same leave type"): which type when the two sides differ, or one side is uninformed absence? Half-day leave adjacent? | BRD LM-17–LM-20 | Specify. | Product |
| LV-02 | Gap | Medium | Do holidays/weekly offs *inside* a leave stretch count as leave days for LM-25 excess-informed counts and LM-10 max consecutive? | BRD LM-10, LM-25 | Specify. | Product |
| LV-03 | Decision | Medium | Q9 absconding: 7 days default; working or calendar days. LM-31 "hold pay" in V1 has no payroll — export a hold flag? | BRD Q9, LM-29–32 | Decide; define V1 effect. | Product |
| LV-04 | Gap | Medium | HD-03 "presence on the working day before and after" — what if those days are leave, on-duty or another holiday? Interplay with sandwich. | BRD HD-03, LM-16 | Specify. | Product |
| LV-05 | Gap | Low | HD-02 "move holiday to next working day" when the roster already assigned that day. | BRD HD-02 | Specify. | Product |
| LV-06 | Gap | Medium | Optional holidays (HD-R3): selection window, approval, cancellation — no flow defined. | BRD HD-R3, HD-01 | Add request flow. | Product |
| LV-07 | Gap | Medium | LV-01 "auto-cancel leave when full hours are worked" — balance restored? Partial hours? Notification? | BRD LV-01 | Specify. | Product |
| LV-08 | Gap | Medium | Leave/permission requests that span a period lock or are approved after lock (LM-23 reversal of uninformed cut after lock). | BRD LM-11, LM-23, PL-R4 | Route through PL-R4 adjustment; confirm. | Product |
| LV-09 | Gap | Low | CO-R4 "employee chooses on the day between pay and comp-off" — how (app prompt? request?). In V1 "pay" option has no effect. | BRD CO-R4, HW-01 | Specify. | Product |
| LV-10 | Gap | Low | Unpaid permission (PM-11) and PM-18 "unpaid minutes" — what V1 output carries them into the payroll export? | BRD PM-11, PM-18, MD-06 | Add to export spec. | Product |
| LV-11 | Gap | Medium | Leave year vs financial year (OR-01) — per org only, or per leave type? Effect on mid-year opening balances (OB-R4). | BRD OR-01, OB-R4 | Specify. | Product |

---

## 9. Attendance period and lock (PER)

| ID | Type | Sev | Issue | Source | Proposed resolution / question | Owner |
|---|---|---|---|---|---|---|
| PER-01 | Conflict | Blocker | **Unlock vs never changes.** PL-06 allows unlock with Org Admin approval; BR-06 says a locked period never changes; AT8 requires locked periods to stay unchanged. | BRD PL-06, BR-06, AT8 | Either drop unlock, or redefine BR-06 as "changes only through the audited unlock workflow". | Product |
| PER-02 | Gap | High | **Recomputing period-level penalties after lock.** A correction to a locked day can change count-based penalties (LA-06) or excess-absence slabs (LM-25) of that locked period. How is "the difference" computed? | BRD PL-R4, BR-06, BR-10 | Recompute the locked period as a shadow run and post the delta; confirm. | Product + Tech |
| PER-03 | Gap | Medium | PL-07 allows corrections for 3 past periods. What happens to a correction older than that (e.g. late-synced offline punch)? | BRD PL-07, PC-07 | Reject with message / HR-only override. | Product |
| PER-04 | Gap | Medium | PL-03 "assumed present after cut-off" + PL-05 "treat pending as present/absent" create automatic adjustments next period — must be visible as such on reports (R23). | BRD PL-03, PL-05, R23 | Confirm labelling. | Product |
| PER-05 | Gap | Medium | Lock granularity: per org, per branch, per (V2) payroll group? PL-R3 says "for the employees in it". | BRD PL-R3 | Tie to REL-01 answer. | Product |

---

## 10. Payroll and money — V2 (PAY)

| ID | Type | Sev | Issue | Source | Proposed resolution / question | Owner |
|---|---|---|---|---|---|---|
| PAY-01 | Ambiguity | Blocker | **What is a "line"?** BR-15 rounds once per line. P6 shows 2 days as one line ₹1,153.85; two per-day lines would give ₹1,153.84. P7 total is ₹3,461.54 as 3 lines but ₹3,461.52 as 6 per-day lines. | BRD BR-15, P6, P7, P16 | Define line granularity per component (per reason, per period). | Product |
| PAY-02 | Decision | High | Q17 rounding mode (half up vs half even); Q18 which of bank/cash absorbs the paise and whether cash is whole rupees. | BRD Q17, Q18, BR-15 | Decide. | Product |
| PAY-03 | Gap | Blocker | **Monthly salary edge cases.** Salary − deduction days × (salary ÷ divisor): divisor 26 with 27 working days all absent ⇒ 27 × 576.92 = ₹15,576.92 deducted from ₹15,000 (negative). Divisor 30 in February, absent all 28 days ⇒ ₹1,000 paid for zero days. | BRD §10.3.1, PY-05, BR-12 | Add floor/cap rule (deduction ≤ salary; full-absence = 0). | Product |
| PAY-04 | Gap | High | **Mid-period joiners, leavers and salary revisions** — no proration rule for monthly salary (only LM-04 for leave, PM-24 for permissions). | BRD PY-R5, PY-05 | Specify proration basis (same divisor? calendar days?). | Product |
| PAY-05 | Gap | High | **Per-minute rate for non-fixed shifts.** "Per-day rate ÷ shift hours ÷ 60" — shift hours for flexible, open, split, weekly-hours staff? For daily/hourly wage types? | BRD §10.3.1, PD-05, OT-11 | Use required hours (SH-03); confirm. | Product |
| PAY-06 | Gap | High | **Monthly rules for non-monthly pay.** LM-25/26 count "in the month"; SP-09 per week; weekly or bi-weekly pay periods straddle months. Which run carries a month-based cut? | BRD LM-25, LM-26, PL-01, AL-09 | Specify attribution rule. | Product |
| PAY-07 | Ambiguity | Medium | **Deduction order vs recovery cap.** PY-04 order, AL-08 cap (% of net pay — net before or after loans?), PY-R11 floor. Circular when cap is % of a net that the recovery itself reduces. | BRD PY-04, AL-08, PY-R11, P11 | Define cap base as net before recoveries (P11 implies this). | Product |
| PAY-08 | Gap | Medium | PY-06 nearest ₹10 rounding — is the rounding difference shown as a payslip line? carried forward? | BRD PY-06, AT11 | Specify. | Product |
| PAY-09 | Gap | Medium | Held pay (PY-R9, LM-31) — released into which run, how shown, journal treatment. | BRD PY-R9, LM-31, AE | Specify. | Product |
| PAY-10 | Gap | Low | "CTC" used (P1, PY-02, SP-07) but never defined. | BRD P1, PY-02 | Add to definitions. | Product |
| PAY-11 | Gap | Medium | PY-08 "daily pay at check-out" contradicts K6 / DA-06 (next morning so deductions are known) and J13. | BRD PY-08, K6, DA-06; TSD J13 | Remove the at-check-out option, or define corrections. | Product |
| PAY-12 | Gap | Medium | Final settlement in V2 without V3 offboarding — what starts it, leave encashment (Q2), notice shortfall formula. | BRD PY-R10, §21 note 4, Q2 | Specify V2 flow. | Product |
| PAY-13 | Decision | Low | Q3 bank list formats; Q7 / AE-R4 Tally version and import format. | BRD Q3, Q7, AE-R4 | Decide. | Product |
| PAY-14 | Decision | Medium | Q1 / A6 daily allowance earning logic (DA-02–04) still to finalise. | BRD Q1, A6 | Decide before V3. | Product |
| CR-01 | Gap | Blocker | **Client-specific rule mechanism.** TSD §4.4: rules "registered per organisation and loaded only for that organisation" — implies vendor-written code in the shared binary. Not stated: compiled-in Rust, WASM plug-in or rule DSL; how a release for one client is deployed without risk to others (CR-R2); how versioned rules stay reproducible for old runs (BR-08). | BRD CR-R1–R7, O5, G10, Q8; TSD §4.4 | ADR needed before V2 design. Answer Q8 (2–3 real client calculations) first. | Tech + Product |
| CR-02 | Gap | Medium | CR-R4 test run "against last closed period" — what if the org has no closed period (new client)? | BRD CR-R4 | Allow synthetic or opening-balance-based test. | Product |

---

## 11. Roles, access and approvals (ACC)

| ID | Type | Sev | Issue | Source | Proposed resolution / question | Owner |
|---|---|---|---|---|---|---|
| ACC-01 | Gap | High | **No permission matrix.** AC-01 makes everything configurable but gives no default per role and no non-configurable limits (e.g. who can unlock, change payroll results, view bank details). Skill requires operation/transition-level permissions. | BRD §5, AC-01; SKILL §6 | Produce default permission matrix + list of non-delegable rights. | Product |
| ACC-02 | Gap | High | **BR-07 self-approval fallback.** When the requester is the only eligible approver (HR Manager's own leave, manager with no manager), who approves? | BRD BR-07, AC-02 | Define fallback chain. | Product |
| ACC-03 | Gap | Medium | **Escalation chain end.** RG-07/LM-13/PM-17 "escalate" — to whom, and after the top of the chain? | BRD RG-07, LM-13, PM-17, OD-05 | Specify. | Product |
| ACC-04 | Gap | Medium | AC-03 delegation: period, scope, can a delegate re-delegate, audit. | BRD AC-03 | Specify. | Product |
| ACC-05 | Gap | Medium | AC-04 "HR with Org Admin approval" introduces a maker-checker flow for settings with no states (draft, pending, approved, rejected). | BRD AC-04, OB-01 | Specify. | Product |
| ACC-06 | Gap | Medium | VC-03 support-access approval "on / off" — who can turn it off? If the vendor can, the control is void. | BRD VC-03, VC-R6; TSD §4.3 | Only Org Admin can change VC-03. | Product |
| ACC-07 | Gap | Medium | **One mobile number, several identities.** Family-shared phones (common for blue-collar staff), the same contract worker in two client orgs, number changes. Token carries one organisation (TSD §8). | BRD UM-R3; TSD §8 | Define uniqueness and org switching. | Product + Tech |
| ACC-08 | Gap | Medium | Auditor role: PII visibility (bank, ID numbers, selfies)? scope limits? | BRD §5 | Specify. | Product |
| ACC-09 | Gap | Low | Session revocation on exit/offboarding/suspension (VC-R4 view-only mode for suspended orgs). | BRD VC-R4, EM-R5; TSD §8 | Specify. | Tech |

---

## 12. Tenancy, security and privacy (SEC)

| ID | Type | Sev | Issue | Source | Proposed resolution / question | Owner |
|---|---|---|---|---|---|---|
| SEC-01 | Risk | Blocker | **Device authentication by serial number.** Serial numbers are printed on devices and not secret; anyone can push fake punches for a known serial. | TSD §9.1 | Add a per-device secret / token, IP allow-list or relay-side auth. Depends on D1 result. | Tech |
| SEC-02 | Gap | High | **Pre-tenant lookups under RLS.** The gateway must find the org from a device serial; the scheduler fan-out must list all orgs; vendor support access sets an org explicitly. All need a role or function that bypasses RLS — not designed. | TSD §4.1, §4.3, §9.1, §10 | Define narrowly scoped `SECURITY DEFINER` functions / separate roles; include in AT20 tests. | Tech |
| SEC-03 | Risk | High | **Supabase auto-exposed Data API.** Supabase exposes tables via its REST API with the anon key by default. TSD says Supabase features are unused, but does not say the API is disabled. Skill: never assume frontend DB access is safe. | TSD §17; SKILL §3 | Disable Data API / revoke anon & authenticated grants; add a build check. | Tech |
| SEC-04 | Risk | Medium | **SQLx with transaction pooling.** Per-transaction `SET LOCAL` org setting works with poolers, but SQLx prepared statements and Supabase's transaction-mode pooler need verifying together. | TSD §4.1; TSD §2 | Spike before DB design. | Tech |
| SEC-05 | Risk | High | **Data residency vs Firebase Auth.** TB-07: all data in India. Firebase Authentication (phone numbers) and FCM tokens are not regional services. | BRD TB-07; TSD §8, D-12 | Verify Firebase Auth data location; record exception or change design. | Tech + Legal |
| SEC-06 | Gap | High | **Privacy law.** K5 calls for a privacy review; India's DPDP Act 2023 obligations (notice, consent, purpose limitation, retention, data-principal rights, breach reporting, processor contracts with Supabase/Google) are not addressed. | BRD K5, N9, N10 | Commission a privacy review; derive requirements. Do not claim compliance before it. | Legal |
| SEC-07 | Conflict | Medium | **Selfie retention vs evidence.** TSD deletes selfies after 90 days (D6); PL-07 allows corrections 3 periods back and N10 requires legal retention; selfies are evidence for disputed punches (BR-01). | TSD §11, D6; BRD PL-07, N10, BR-01 | Align retention with correction window and legal minimum. | Product + Legal |
| SEC-08 | Gap | Medium | **Biometrics.** Confirm the system never stores fingerprint or face templates (only enrolment IDs), and whether selfies are treated as biometric data. | BRD PC-R4, PC-R8, K5 | State explicitly. | Product + Legal |
| SEC-09 | Gap | Medium | **Field-level encryption key management** (bank/ID numbers): Secret Manager vs Cloud KMS, rotation, and how masked search works. | TSD §15 | Design. | Tech |
| SEC-10 | Gap | Medium | **Audit log integrity.** "Insert-only, cannot be edited through the application" — DB-level enforcement (revoked UPDATE/DELETE, triggers) and tamper evidence not specified; retention not specified. | TSD §3.3, §15; SKILL §6 | Specify. | Tech |
| SEC-11 | Gap | Medium | **Tenant exit.** No requirement for data export, retention and deletion when a client leaves. | BRD VC-R4 | Add. | Product + Legal |
| SEC-12 | Gap | Low | IP restriction (PC-05) behind carrier-grade NAT / proxies; IP source header trust on Cloud Run. | BRD PC-05, PC-15 | Specify trusted header. | Tech |

---

## 13. Data model and state machines (DM)

| ID | Type | Sev | Issue | Source | Proposed resolution / question | Owner |
|---|---|---|---|---|---|---|
| DM-01 | Gap | High | **Missing entities in TSD §6:** punch adjudications/exclusions, flagged-punch approvals, manual attendance entries, raw device payloads, bulk waivers (LA-17), exemptions (G7), permission balances/ledger, WFH requests, shift swap requests (SH-12), optional-holiday selections, post-lock adjustments (PL-R4) in V1, sector templates, custom fields (EM-R8), settings preview runs, idempotency keys, outbox. | TSD §6; BRD as cited | Add in Phase 1 domain model. | Tech |
| DM-02 | Gap | High | **Daily result history.** Skill requires previous results and provenance to be preserved on recalculation; TSD stores "results with policy version" but not prior versions. | SKILL §5.2; TSD §5.2, §6 | Keep result versions (or an append-only result log). | Tech |
| DM-03 | Gap | High | **No state machines defined** for: day result (provisional → finalised → locked), requests (submitted/approved/rejected/cancelled/escalated/auto-actioned, multi-level), period (open → cut-off → locked → unlocked?), pay run (BRD lists 7 steps but not reopen/void), employee status (EM-R5), device, opening balance (draft → confirmed → amended), client rule (draft → test → signed off → active → retired), organisation (active → suspended → exited), support-access session. | SKILL Ph2; BRD as cited | Produce in Phase 2. | Tech + Product |
| DM-04 | Gap | Medium | **Concurrent recalculation.** "Duplicate queue entries are harmless" — but two concurrent recalcs for the same key can race and store an older result last. | TSD §5.2 | Per-key lock or input-version check. | Tech |
| DM-05 | Gap | Medium | VC-R3 usage (active employees, last sign-in, devices online) must come from client tables, but TSD §4.3 says the vendor console reads vendor tables only. Population path undefined. | BRD VC-R3; TSD §4.3 | Define an aggregation job writing to vendor tables. | Tech |
| DM-06 | Gap | Medium | Multilingual master data — org-defined leave types, reason lists, components, payslip labels in Tamil/Hindi (AT14). | BRD N7, AT14, PY-R7 | Decide whether masters carry translations. | Product |
| DM-07 | Gap | Low | VC-01 employee limit — definition of "active employee" (notice, absconding, held?). | BRD VC-01, MD-02 | Define. | Product |

---

## 14. Technical and operations (TEC)

| ID | Type | Sev | Issue | Source | Proposed resolution / question | Owner |
|---|---|---|---|---|---|---|
| TEC-01 | Decision | Medium | Open TSD decisions D3 (email/SMS notification provider), D5 (App Hosting in Mumbai), D6 (selfie retention), D7 (CI tool), D8 (PITR timing). | TSD §20 | Decide. | Tech |
| TEC-02 | Gap | Medium | **Jobs in branch time.** J1 "previous day" at 03:00 IST, J5 accrual 00:30 — orgs/branches in other time zones (OR-R1) need per-branch scheduling. | TSD §10; BRD OR-R1 | Fan out by branch-local date. | Tech |
| TEC-03 | Gap | Medium | **Bulk recalculation load.** A settings change for 2,000 employees × 31 days = 62,000 tasks; G8 preview runs the same work. No throttling or N3 target for bulk. | TSD §10; BRD G8, N3 | Set limits and a target. | Tech |
| TEC-04 | Ambiguity | Medium | **Dashboard freshness.** Flow says "dashboards update" after recalc, but J11 refreshes every 15 min and realtime is not used. DB-R3/DB-R6 "who is in now". | TSD §3.2, J11; BRD DB-R3, DB-R6 | State acceptable latency. | Product |
| TEC-05 | Gap | Medium | **RPO/RTO** undefined; N8 99.5% availability is "proposed" and a single Supabase instance has no stated failover. | BRD N8, N11; TSD §15 | Set RPO/RTO; confirm N8 (Q11). | Product + Tech |
| TEC-06 | Gap | Low | Mobile OS minimums and supported browsers ("current desktop browsers"); employee self-service on mobile web? | BRD N6 | Specify. | Product |
| TEC-07 | Gap | Low | Migration of a client's historical data (punches, balances) beyond opening balances — in scope? | BRD C15 | State. | Product |
| TEC-08 | Gap | Low | SMS/WhatsApp notifications via an Indian provider need sender/template (DLT) registration per TSD §8; cost pass-through per plan not in Q5. | TSD §8, §14; BRD Q5 | Include in plan design. | Product |

---

## 15. Legal and compliance (LEG)

Per the Skill (§4 Ph9, §5.4), no statutory interpretation is assumed here; these items need a labour-law consultant (BRD K1, K2).

| ID | Type | Sev | Issue | Source | Proposed resolution / question | Owner |
|---|---|---|---|---|---|---|
| LEG-01 | Risk | High | Rules flagged by K1 (double cut for absence LM-22, cuts for *approved* leave LM-25, money fines for lateness PD, deductions from allowances, recovery cap AL-08 50%) — permissibility and caps depend on the current wage law; verify the current status of India's labour codes and state rules. | BRD K1, LM-22, LM-25, PD, AL-08 | Consultant review before each rule is enabled for a client; record per-template guidance. | Legal |
| LEG-02 | Risk | Medium | Payment timelines (pay day PY-07; final-settlement timing PY-R10) may have legal deadlines. | BRD PY-07, PY-R10 | Verify. | Legal |
| LEG-03 | Gap | Medium | Registers and payslip content required by law, including for cash payments (K2). | BRD K2 | Verify before V2 go-live. | Legal |
| LEG-04 | Gap | Medium | Record retention periods (N10, DC-07) "to confirm". | BRD N10, DC-07 | Verify. | Legal |
| LEG-05 | Risk | Low | A3 places legality on each organisation — confirm this is in the client contract. | BRD A3 | Contract clause. | Product |

---

## 16. Worked-example checks

All BRD worked examples were recomputed. Arithmetic is consistent (E4 8h05, E8 7h15, P3 ₹30.05, P6 ₹1,153.85, P7 ₹3,461.54, P11 ₹1,200, P12 ₹8,700, P14 ₹3,300, P17 ₹1,126.13). The only example-level issues are definitional:

- P6, P7, P16 depend on line granularity (PAY-01).
- E15 speaks of a "payroll input" arrear in V1 — fine as export, but the export format and adjustment record are not specified (DM-01).
- E18 assumes the manager approves flagged punches (PUN-03).
- No worked examples exist for: night shift across midnight, overlapping windows, split shift, open shift, mid-period joiner, salary revision, weekly-paid worker crossing a month, post-lock period-penalty recompute. Add these before AT1 can be meaningful.

---

## Appendix A — Status of existing questions

| Ref | Topic | Status | Register link |
|---|---|---|---|
| BRD Q1 | Daily allowance earning logic | Open | PAY-14 |
| BRD Q2 | Leave encashment in V2 | Open | REL-07, PAY-12 |
| BRD Q3 | Bank list formats | Open | PAY-13 |
| BRD Q4 | Cash payout acknowledgement | Open (V3) | — |
| BRD Q5 | Plan contents / mobile app per plan | Open | REL-07, TEC-08 |
| BRD Q6 | Reimbursements in V2 | Open | REL-07 |
| BRD Q7 | Tally version / format | Open | PAY-13 |
| BRD Q8 | Real client calculations for plug-in design | Open — **blocks CR-01** | CR-01 |
| BRD Q9 | Absconding days | Open | LV-03 |
| BRD Q10 | Extra shift detection | Open (V2) | — |
| BRD Q11 | NFR targets | Open | TEC-05, DOC-08 |
| BRD Q12 | OT pay / DA release | Open | REL-06 |
| BRD Q13 | Performance module scope | Open (V3) | — |
| BRD Q14 | BRD sign-off | Open — **blocks Gate 1** | GOV-01 |
| BRD Q15 | eSSL HTTPS | Open (= TSD D1) | PUN-01 |
| BRD Q16 | Sign-in SMS route | Decided in TSD (D-12), not reflected in BRD | DOC-02, DOC-03, DOC-04 |
| BRD Q17 | Rounding mode | Open | PAY-02 |
| BRD Q18 | Bank/cash paise split | Open | PAY-02 |
| TSD D1 | eSSL HTTPS & contact rate | Open | PUN-01, SEC-01 |
| TSD D2 | Sign-in codes | Decided (D-12) | DOC-04 |
| TSD D3 | Email / SMS notification provider | Open | TEC-01 |
| TSD D4 | Supabase region | Decided (D-14) | — |
| TSD D5 | App Hosting in Mumbai | Open | TEC-01 |
| TSD D6 | Selfie retention | Open | SEC-07 |
| TSD D7 | Build pipeline tool | Open | TEC-01 |
| TSD D8 | PITR from launch | Open | DOC-05 |

---

## Appendix B — Suggested next steps (Phase 0)

1. Settle the six Blockers that are pure decisions: GOV-01, CON-01, CON-03, PER-01, REL-01, ATT-10.
2. Write the business rules for ATT-01 (pairing), ATT-09 (one calculation order), ATT-12 (BR-04 comparison) and PAY-01/PAY-03 (line and floor rules), each with a new worked example.
3. Run the eSSL device test (PUN-01) — it unblocks SEC-01 and the gateway design.
4. Re-issue TSD §19 with Firebase SMS pricing (DOC-03) and record the D-12 rationale.
5. Then produce user stories, the permission matrix and the domain model (GOV-03) — Approval Gate 1.
