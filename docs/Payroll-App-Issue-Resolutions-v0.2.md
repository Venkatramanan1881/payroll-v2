# Issue Resolutions and Decision Log — Payroll App
Version 0.2 · 24 November 2026
Responds to: `Payroll-App-Open-Issues-v0.1.md` (review answers received 22 November 2026)
Approver of record: the reviewer (GOV-01 answer "Me")

**Change log**

| Version | Date | Change |
|---|---|---|
| 0.2 | 24 November 2026 | CL-07, CL-08, CL-14 and CL-17 answered (recommended option each), before Package 2 (Attendance) Phase 0. Added to §1.1 and the decision log as DEC-46–49. ATT-08 decided (DEC-50), SEC-07 selfie retention decided (DEC-51), PUN-01 approach agreed (gateway waits for the device test; the rest of Package 2 goes ahead). CL-09–13, CL-15 and CL-16 still open. |
| 0.1 | 22 November 2026 | First issue. |

---

## 0. Summary

| Status | Where | Meaning |
|---|---|---|
| **Decided** | §2.3 (DEC-01–39) | Answer is clear; becomes a requirement when the BRD/TSD are revised. |
| **Proposal accepted** | §4 | Reviewer accepted the proposed resolution; text there is now the requirement. |
| **Explained — confirm** | §3 (28 items) | Reviewer asked for an explanation; recommendation given, needs a yes/no. |
| **Clarify** | §1 (CL-01–17) | Answer conflicts with another answer, the BRD, or the Skill. |
| **Hold / later** | §9 | Parked by the reviewer. |

Nothing here edits the BRD, TSD or Skill. Section 8 lists the changes each document needs.

---

## 1. Clarifications needed (answers that conflict)

These must be answered before BRD v0.9 is written. Each one shows the conflict and a recommendation.

| ID | Conflict | Recommendation |
|---|---|---|
| **CL-01** | **How do employees sign in?** DOC-03: "sign-in SMS is not needed". DOC-04: "Zepto mail". SEC-05: "Zoho mail we will use". But UM-R3 says employees sign in with mobile number + one-time code, and TSD D-12 chose Firebase phone sign-in. Without SMS, the code needs another channel. Many blue-collar workers have no email. | Pick one: (a) one-time code by **WhatsApp** to the mobile number, (b) code by **email via ZeptoMail** (only for employees with email), (c) **employee code + PIN** set at first sign-in, issued by HR. Recommended: (c) as the default plus (b) where email exists. That removes Firebase (D-12 superseded) and the SMS cost line. |
| **CL-02** | **"Untagged device punches ignored"** (ATT-01). In the TSD, 70% of punches come from devices. If the eSSL devices aren't set to send an in/out state with each punch, most punches would be thrown away. It also conflicts with BR-01 / PC-R6 (store every punch). | Store every punch as always. Untagged punches **are not used for in/out pairing**: they are read **first-in / last-out** for the day, or the org must set up its devices to tag direction. Confirm that PR-01 keeps two modes: *direction-tagged* (default) and *first-in/last-out*. "Paired" is removed. |
| **CL-03** | **Rounding mode** (PAY-02 / Q17): "1.2 → 2.0 or keep one decimal". These are two different rules (see §3.13). | Pick one: half-up to paise (BR-15 today), round **up** to the rupee, or round to one decimal. |
| **CL-04** | **Environments "all in one"** (DOC-09). BRD TB-09 and TSD §16 require test and live to be separate. Putting dev, test and live together risks a test job writing to live payroll data. | Keep **live in its own Google project and Supabase project**. Dev and test can share one ("all in one" for non-live). |
| **CL-05** | **Unknown enrolment IDs "reject"** (PUN-02). Rejecting at the gateway loses evidence (BR-01). And the device may delete its copy once the punch is acknowledged, so the punch is gone for good. | Store as **unmapped** and don't count it. Once HR maps the ID, those punches are reprocessed automatically. The employee never sees them until they're mapped. |
| **CL-06** | **Fingerprint template sync is in scope** (PUN-11), but SEC-08 (accepted) says the system never stores biometric templates. Syncing templates between devices means the system has to relay them. | Templates are relayed **device → gateway → device in memory only**, never written to the database or logs. Or keep enrolment manual on each device. Confirm one. |
| **CL-07** | **Device spoofing** — the top-12 answer is "hold"; SEC-01 says "no need to consider". | Record it as **risk accepted for V1**, to revisit after the D1 device test. Cheap fix if wanted: a per-device token in the push URL. |
| **CL-08** | **Overtime is V3** (REL-02). But BRD M01, MD-06 and §21 note 1 say overtime hours are *identified* from V1 and included in the payroll input export. | Confirm that **overtime hours are not calculated or exported in V1**. Raw punches still allow them to be worked out back to V1 when V3 arrives. |
| **CL-09** | **Overtime in V2.** REL-06 "V3". BRD §21 note 2 lets V2 pay overtime as a manually entered amount per run. | Confirm whether the V2 manual overtime amount stays. Recommended: yes — it's just a "per run" earning component (PY-R3). |
| **CL-10** | **Multilingual master data "V5"** (DM-06). The release plan only has V1–V3. | Record it as "beyond V3 (unscheduled)". Until then, org-defined names (leave types, reasons) show as entered in all languages. |
| **CL-11** | **Vendor "need to see all"** (DM-05). TB-01 / AT20 forbid the vendor reading client data outside approved support access. | Vendor sees **all usage metrics** (counts, health, last sign-in) through the aggregation job, not employee-level data. Confirm. |
| **CL-12** | **Mid-period joiner method** (PAY-04 "date 15"). Two methods give different pay (§3.15). | Choose method A or B. |
| **CL-13** | **Permission matrix "only of HR manager"** (ACC-01). Read as: only the HR Manager can change role permissions (AC-01). The Org Admin today "manages users, roles". | Confirm: the Org Admin assigns users to roles; **only the HR Manager edits what a role can do**. |
| **CL-14** | **1M employees** (DOC-08). At 4 punches × 26 days, that's about **104M punch rows a month**. TSD §19's largest database (2XL) was sized for 50k employees. | Accept. TSD needs a size D estimate and a scaling plan (partitioning from day one, possibly sharding by org — TSD §4.6 already allows this). |
| **CL-15** | **Reimbursements** — REL-07 "V3" moves them out of V2 (BRD §21, Q6). Q5 (plan contents) and Q9 (absconding day count) were not answered. | Confirm reimbursements in V3. Q9 recommendation: **7 calendar days**. Q5 stays open. |
| **CL-16** | **Leave year** (LV-11). The answer talks about leave applying org-wide / per branch / to chosen branches — that's **leave-type scope**, not leave year. | Record: leave types can apply org-wide, to chosen branches, or be common. The leave year stays org-level (OR-01, default April–March). Confirm. |
| **CL-17** | **Lock unit** (PER-05 "per branch, per org, per payroll"). Three overlapping lock levels can leave an employee half-locked. | One lock unit = **payroll group × branch × period**. "Lock org" or "lock branch" are just shortcuts that lock every matching unit. Confirm. |

### 1.1 Clarification answers (22 November 2026)

| ID | Answer | Resolution | DEC |
|---|---|---|---|
| CL-01 | Recommended | Employees sign in with **employee code + PIN** (PIN set at first sign-in, issued and reset by HR). A one-time code by **email (ZeptoMail)** is available where the employee has an email address. Firebase phone sign-in (D-12) is dropped, and so is the sign-in SMS cost line. | DEC-40 |
| CL-02 | Recommended | Every punch is stored. PR-01 has two modes: **direction-tagged** (default) and **first-in / last-out**. Untagged punches are never paired by count. "Paired" mode is removed. | DEC-41 |
| CL-03 | "1.2 stays 1.2; 1.95 becomes 2.0" | **Round each amount line to one decimal (10 paise), half up**: 1.2 → 1.2, 1.94 → 1.9, 1.95 → 2.0. Rates are still never rounded inside a calculation, and net pay is still rounded by PY-06. Effect on BRD examples: P3 30.048 → **30.0**; P6 1,153.846 → **1,153.8**; P7 1,153.8 + 1,730.8 + 576.9 = **3,461.5**; P17 1,126.125 → **1,126.1**. | DEC-42 |
| CL-04 | All in one | **Dev, test and live share one Google project and one Supabase project.** This contradicts BRD TB-09 and TSD §16, which must be reworded. **Risk accepted:** a test job, migration or mistaken connection can reach live payroll data. Minimum safeguards proposed: a separate database (or schema) and service account for each environment; the live service account is never used by test; client data copied into test is masked (TB-09 masking stays). | DEC-43 |
| CL-05 | OK | Punches with an unknown enrolment ID are **stored as unmapped and not counted**. Once HR maps the ID, they are reprocessed. | DEC-44 |
| CL-06 | OK | Fingerprint/face templates are synced **device → gateway → device in memory only**. They are never written to the database, file store or logs. | DEC-45 |

| CL-07 | Recommended, with the cheap fix | **Each device gets its own secret token in its push URL**, revocable per device. A punch arriving without a valid token for that device is rejected and logged. Full spoof protection is still not claimed; revisit after the D1 device test. | DEC-46 |
| CL-08 | Recommended | **No overtime is calculated, shown or exported in V1.** V1 stores raw punches and worked hours only; V3 can work overtime out back to V1 from them. | DEC-47 |
| CL-14 | Recommended | **Punch and daily-attendance tables are partitioned by month from day one.** The TSD gets a size D estimate and a scaling plan; sharding by organisation stays possible (TSD §4.6). | DEC-48 |
| CL-17 | Recommended | **One lock unit = payroll group × branch × period.** "Lock organisation" and "lock branch" lock every matching unit; an employee is never half-locked. | DEC-49 |

CL-09 to CL-13, CL-15 and CL-16 are still open; the recommendations in the table above stand until answered.

---

## 2. Decision log (GOV-02)

### 2.1 Why one log

The TSD has two numbering schemes: **D1–D8** (TSD §20, "open technical decisions") and **D-12 / D-14** (in the change log, from a register that isn't in the repo). The BRD has its own **Q1–Q18**. Nobody can tell which decision is current or what it replaced. From now on, every decision gets one ID **DEC-nn**. It records the date, the decider, the source question, and what it supersedes. The old IDs are kept as cross-references.

### 2.2 The TSD decisions explained

| Old ID | What it asks | Why it matters | Status now |
|---|---|---|---|
| D1 | Can the eSSL models push over HTTPS, and how often do they contact the server? | If not over HTTPS, a relay is needed (about $10–15/month). The contact rate drives gateway cost (TSD §19.3). | **Open — not decided yet** (PUN-01) |
| D2 / D-12 | How employees get sign-in codes. | Firebase SMS was the biggest cost line at scale. | **Superseded by DEC-40** (employee code + PIN; email code via ZeptoMail) |
| D3 | Email provider; SMS provider for notifications. | Needed for notifications and admin password resets. | **Decided: ZeptoMail (Zoho)** for email. SMS/WhatsApp notifications in V3 (TEC-08). |
| D4 / D-14 | Supabase region. | Data must stay in India (TB-07). | **Decided: Mumbai** (16 Nov 2026) |
| D5 | Firebase App Hosting in Mumbai, or plain Cloud Run. | Data location and CDN. | **Proposal accepted:** App Hosting if Mumbai is offered, otherwise Cloud Run. Check at setup. |
| D6 | Selfie retention. | Evidence vs storage and privacy. | **Explained — confirm** (§3.21) |
| D7 | Build pipeline tool. | Delivery. | **Proposal accepted:** GitHub Actions (or Cloud Build if the code stays in Google). Tech to finalise. |
| D8 | Point-in-time recovery from launch or from Growth size. | +$100/month vs losing a day of punches. | **Decided: from the first live client** (DOC-05) |

### 2.3 Decisions recorded on 22 November 2026

| DEC | Decision | Source issue | Supersedes |
|---|---|---|---|
| DEC-01 | The reviewer approves the BRD and TSD, subject to the v0.9 revisions in §8. | GOV-01, Q14 | — |
| DEC-02 | Mobile app is **Flutter**. | CON-01 | Skill §3 "React Native + Expo" |
| DEC-03 | **No Pub/Sub** for now; Cloud Tasks + Scheduler with a transactional outbox. | CON-02 | Skill §3 Pub/Sub |
| DEC-04 | **Offline punches are in V1.** | CON-03 | Skill Ph10 "later" |
| DEC-05 | **No cache** in V1. | CON-04 | TSD §4.2 cache line |
| DEC-06 | **Payroll groups (membership and period) move to V1.** Pay calculation stays V2. | REL-01 | BRD C12, §21 |
| DEC-07 | Locked periods **can be unlocked** through the audited PL-06 workflow. | PER-01 | BR-06 wording |
| DEC-08 | Monthly salary **never goes below zero.** | PAY-03 | — |
| DEC-09 | Client-specific payroll rules run as a **separate microservice.** | CR-01 | TSD §4.4 "loaded per org" |
| DEC-10 | **All BRD defaults confirmed**, except where a DEC changes them. | GOV-05 | — |
| DEC-11 | Email through **ZeptoMail (Zoho)**. | DOC-04, SEC-05, D3 | — |
| DEC-12 | **PITR from the first live client.** | DOC-05, D8 | TSD §15 |
| DEC-13 | Plan capacity for **1,000,000 employees.** | DOC-08 | TSD §18 |
| DEC-14 | Overtime (identification, approval, pay), daily allowance, WFH, HW-01 "overtime", Supervisor role → **V3.** | REL-02/03/04/08/11 | BRD M01, §21 notes 1–3 |
| DEC-15 | Advances/loans and Payroll **depend on each other** (advances need Payroll). | REL-05 | AL-R8 export with Payroll off |
| DEC-16 | **Leave encashment in V2**; reimbursements V3. | REL-07 | BRD §21 V2 list |
| DEC-17 | **No kiosk.** HR can raise requests on behalf of an employee. | REL-09 | PC-01 "kiosk" |
| DEC-18 | Flagged punches **count provisionally**. Anyone except the person who punched may approve, within their scope. | PUN-03, PUN-04 | E18 "not counted until approved" |
| DEC-19 | **Liveness check** for selfie punches: an option per organisation. | PUN-06 | — |
| DEC-20 | **Device binding** for the employee app. | PUN-07 | — |
| DEC-21 | Device clock mismatch → **use receipt time** (see §4 for limits). | PUN-10 | — |
| DEC-22 | **Open shifts and weekly-hours staff are out of scope.** | ATT-03, ATT-06 | SH-01 "open", SH-03 per week |
| DEC-23 | **Night shift is a flag** on a shift, not a shift type. | ATT-07 | — |
| DEC-24 | **No quarter-day statuses or cuts.** A half day is always half of the employee's own shift. | ATT-11 | DS-06, quarter-day options in LA/EC |
| DEC-25 | **One absence-handling setting group** (§3.1). | ATT-10 | DS-10, LM-28, LV-04 |
| DEC-26 | Mid-period transfer: each day uses **the branch's calendar for that day**. | ATT-20 | — |
| DEC-27 | Default leave types: **Casual (planned)** and **Sick (emergency)**; the recovery order is configurable. | ATT-21 | LA-12 default |
| DEC-28 | **No approver delegation.** | ACC-04 | AC-03 |
| DEC-29 | **No Auditor role.** | ACC-08 | BRD §5 |
| DEC-30 | **One mobile number = one identity.** | ACC-07 | — |
| DEC-31 | Self-approval fallback = **Organisation Admin**. End of escalation = **Org Admin / HR Manager**. | ACC-02, ACC-03 | — |
| DEC-32 | **Supabase Data API disabled.** | SEC-03 | — |
| DEC-33 | Tenant exit: **data kept 3 months, then deleted**, with an export. | SEC-11 | — |
| DEC-34 | **HR can override any past month** (beyond PL-07). | PER-03 | PL-07 hard limit |
| DEC-35 | Active-employee statuses: active / long leave / notice / absconding. **A status change needs approval.** | DM-07 | EM-R5 |
| DEC-36 | **Historical data migration in scope.** SMS/WhatsApp notifications V3. | TEC-07, TEC-08 | — |
| DEC-37 | Absconding sets **a flag only** (no pay hold in V1). | LV-03 | LM-31 hold in V1 |
| DEC-38 | **No HD-02 holiday move.** Holidays on a weekly off stay as they are. | LV-05 | HD-02 |
| DEC-39 | LV-01 auto-cancel: **balance restored, day flagged.** | LV-07 | — |
| DEC-40 | Employee sign-in: **employee code + PIN**; email one-time code via ZeptoMail where an email exists. | CL-01 | D-12 (Firebase phone sign-in), UM-R3 |
| DEC-41 | Reading modes: **direction-tagged** (default) and **first-in/last-out**; "paired" removed. | CL-02, ATT-01 | PR-01 |
| DEC-42 | Money lines rounded to **one decimal, half up**. | CL-03, PAY-02, Q17 | BR-15 "2 decimals" |
| DEC-43 | **Dev, test and live in one project** (risk accepted; safeguards in §1.1). | CL-04, DOC-09 | BRD TB-09, TSD §16 |
| DEC-44 | Unknown enrolment punches **stored as unmapped, not counted**, reprocessed when mapped. | CL-05, PUN-02 | — |
| DEC-45 | Biometric templates relayed **in memory only**, never stored. | CL-06, PUN-11 | — |
| DEC-46 | **Per-device secret token** in each device's push URL, revocable per device. | CL-07, SEC-01 | — |
| DEC-47 | **No overtime calculated, shown or exported in V1**; raw punches and worked hours only. | CL-08, REL-02 | BRD M01 "identified from V1", MD-06, §21 note 1 |
| DEC-48 | **Punch and daily-attendance tables partitioned by month from day one.** | CL-14, DOC-08 | TSD §19 sizing |
| DEC-49 | **Lock unit = payroll group × branch × period.** | CL-17, PER-05 | — |
| DEC-50 | Attendance date, shift and late/early minutes use **the master branch's time zone on that date**, wherever the punch is made. The punch's local time is kept for display. | ATT-08 | — |
| DEC-51 | Punch selfies are **kept 13 months**, then deleted. | SEC-07, D6 | TSD 90-day selfie retention |

---

## 3. Explanations requested

### 3.1 ATT-10 — Four settings decide the same absent day (top-12 #7)

**The settings today**

| Setting | What it does | Default |
|---|---|---|
| **DS-10** (Day status) | Absent with no leave applied → loss of pay / auto-debit leave in a chosen order / hold as pending for N days. | Pending 3 days, then LOP |
| **LM-28** (Leave) | Absent days not covered by leave → loss of pay / auto-debit leave types in a chosen order. | LOP |
| **LV-04** (Leave & attendance) | A list of leave types in order, used "for absences and penalties". | — |
| **LA-12 / EC-11** (Late / Early) | Penalty recovery order: permission / chosen leave types / LOP / fine / none. | Casual leave, then LOP |

**The clash, with an example.** Ravi is absent on Tuesday 10 November, with no leave request.
- DS-10 says: wait 3 days for him to apply; if he doesn't, LOP.
- LM-28 says: LOP — no waiting.
- If HR sets DS-10 = auto-debit and LM-28 = LOP, the engine can't tell which wins.
- DS-10 and LM-28 each have their own debit order, and LV-04 is a third list. That makes three possible orders for one day.
- LA-12 is a fourth list, used when a late penalty becomes a half-day cut.

**Merged design (DEC-25)** — one group called **Unapproved absence (UA)**:

| New | Rule | Default |
|---|---|---|
| UA-01 | Days the employee has to apply leave for an absent day; until then the day shows "Absent – pending". | 3 days |
| UA-02 | After UA-01: loss of pay / debit leave using UA-03. | Debit leave, then LOP |
| UA-03 | **The single debit order** (leave types in order, ending in LOP). | Casual → LOP |
| UA-04 | Penalties (late, early, count-based) use: the same order as UA-03 / their own order. | Same as UA-03 |
| UA-05 | Sick leave may be debited automatically: yes / no (sick is for emergencies, so normally the employee applies for it). | No |

DS-10, LM-28 and LV-04 are removed. LA-12/EC-11 become UA-04. LM-21 (uninformed double cut) and LM-24 (excess absence) don't change. They are *extra* cuts on top, not ways of handling the day itself.

Ravi under the merged rules: 10 Nov shows "Absent – pending". On 13 Nov (3 days later) Casual leave is debited; if he has no Casual balance, the day is LOP. If he applies on 12 Nov, the day becomes informed leave and nothing is auto-debited.

### 3.2 ATT-12 — BR-04 "no double penalty" (top-12 #8)

**What the rule means.** One shortfall (for example arriving 150 minutes late) may cause only **one** pay effect. If two rules both hit it, only the larger one applies.

**Example.** Late 150 minutes, monthly ₹15,000, divisor 26:
- Slab LA-05: above 120 minutes = **half day** → 0.5 × 576.92 = ₹288.46
- Money rule PD per-minute: 150 × 1.2019… = ₹180.29
- BR-04: half day is larger, so only the half day applies.

**Why it can't be applied as written.**
1. In V1 there's no money, so a half day can't be compared with ₹180.
2. A **fixed-amount** count penalty (LA-09 "₹100 every 3rd late mark") isn't tied to one day's minutes, so it isn't the "same shortfall" as a slab cut.

**Recommendation.**
- **Turn the PD warning into a block.** A payroll group can't have PD money deduction on together with LA-06/LA-10/EC-05/EC-09 count- or minute-based penalties. Then BR-04 only ever compares like with like.
- **Same day, same minutes:** a day cut (half/full day) always replaces a minute-based deduction for those minutes. No money needed, so it works in V1.
- **When rupees are available (V2):** compare in rupees at the employee's own rates, and apply the larger.

### 3.3 GOV-04 — "Finalised" and "manual action" (accepted)

- **Finalised day:** a day whose result is in the final state (§3.17 state machine) with no open flag, request or regularisation.
- **Manual action:** any regularisation, HR correction, flagged-punch approval or manual attendance entry touching that day.
- **Measure:** finalised days with no manual action ÷ all employee-days in locked periods. Target ≥ 95%.

### 3.4 PUN-05 — Offline punch rules (V1, DEC-04)

| Rule | Proposal |
|---|---|
| Original time | The phone's time, checked against the phone's uptime clock (TSD §9.3). |
| Clock check | If phone time and uptime-derived time differ by more than 5 minutes (reuses PC-08): flag, and use the uptime-derived time. |
| Maximum offline age | Accepted any time. Over 3 days it's flagged as late sync (PC-07). |
| Synced into a locked period | Becomes a post-lock adjustment (PL-R4), not an edit to the locked period. |
| Location / geo-fence | Captured at punch time. The server checks it against the fence version **in force at punch time**. |
| Selfie | Captured offline, uploaded on sync. Liveness (DEC-19) checked on the server after upload. |
| Duplicates | A unique key per punch; syncing twice creates no duplicate. |
| On the phone | Queue encrypted. Cleared after the server confirms. The app never decides day status. |

**Example.** The phone is offline at 09:02. Uptime shows 4h38m elapsed until sync at 13:40, so the derived time is 09:02 — they agree, and the punch is accepted at 09:02. If the employee had set the phone clock back 30 minutes, phone time 08:32 ≠ derived 09:02. The punch is flagged and recorded at 09:02.

### 3.5 ATT-13 — Slab boundaries vs grace (accepted: validate)

- Late minutes are counted from **shift start** (LA-02 default). Slabs are **inclusive** ranges of whole minutes.
- **When settings are saved:** the first slab must start at exactly grace + 1, and slabs must not overlap or leave gaps. If grace changes from 10 to 15, the system asks HR to fix the slab "11–120" → "16–120".

### 3.6 ATT-14 — Missed-punch cut-off (answer: "regularisation is possible")

Cut-off = **end of the regularisation window (RG-04, default 7 days) or the period lock, whichever comes first.** Until then, the employee can still regularise. After it, MP-03 applies (default: count completed pairs only).

### 3.7 ATT-15 — Overtime rounding defined twice

RD-04 rounds "net hours / overtime / both". OT-06 also rounds overtime. Since overtime is V3 (DEC-14), **in V1 RD-04 applies to net hours only**. When V3 arrives, OT-06 alone governs overtime and RD-04 drops its "overtime" and "both" options.

### 3.8 ATT-16 — Rounding: daily or period total?

Example: 15-minute nearest rounding, an employee works 7h52 each day for 26 days.
- **Daily:** 7h52 → 8h00 each day → 208h for the period.
- **Period:** 26 × 7h52 = 204h32 → 204h30.

The difference is 3.5 hours a month. **Recommendation:** round the **daily** net hours (that's what the employee sees on each day), and work out **day status from the unrounded** hours. That way rounding can never turn a short day into a full day.

### 3.9 ATT-17 — When do "same week" compensation and make-up close?

- **Same day (LA-14 default):** settled in that day's calculation.
- **Same week:** the late mark is **provisional** until the week ends (week start from OR-02, default Monday). On Sunday night the week is checked: if extra minutes in the week cover the late minutes, the mark is cleared.
- **Week crossing a period lock** (e.g. Mon 28 Oct – Sun 3 Nov, October locked on 1 Nov): the October part is judged at lock using only the October days. Anything still uncompensated stands.

### 3.10 ATT-18 — Grace on permission over-stay

E10: permission 10:00–11:00, back at 11:25, so 25 minutes extra. With grace (10), the late minutes would be 15. **Recommendation: no grace on over-stay.** Grace is for arriving at shift start, not for returning from an approved absence. 25 minutes falls in slab 11–120, so one late mark, as E10 already shows.

### 3.11 LV-01 — Sandwich between two different leave types (answer: make it an option)

New option **LM-20a**: sandwiched days take the leave type of **the day before / the day after / the type with more balance / unpaid**. Default: the day before. If one side is uninformed absence, the sandwiched day is uninformed absence (LM-17 "either").

### 3.12 LV-04 — Paid holiday needs presence before and after (HD-03)

Example: holiday Wednesday. Tuesday the employee was on approved Casual leave, Thursday present. Is the holiday paid?

**Recommendation:** these count as presence: present, half day, approved paid leave, on-duty, weekly off, another holiday. These don't: LOP, uninformed absence, unpaid leave. Look at the **nearest working day** on each side, skipping weekly offs. In the example, the holiday is paid.

### 3.13 PAY-02 / Q17 — Rounding mode options (see CL-03)

| Option | 1,126.125 | 30.048 | 1.2 |
|---|---|---|---|
| Half up to paise (BR-15 today) | 1,126.13 | 30.05 | 1.20 |
| Up to whole rupee ("1.2 → 2") | 1,127 | 31 | 2 |
| One decimal, half up | 1,126.1 | 30.0 | 1.2 |

Rounding **up** on every line favours whoever receives that line. On a deduction, the employee always loses up to ₹0.99 per line. **Recommendation:** keep half up to paise per line, and round **net pay** to the rupee (PY-06 default) for whole-rupee pay.

### 3.14 PAY-01 — What is a "line"? (answer: round off)

P7 has 6 deduction days at ₹576.923… each:
- **One line per reason** (as P7 shows): 2 days ₹1,153.85 + 3 days ₹1,730.77 + 1 day ₹576.92 = **₹3,461.54**
- **One line per day:** 6 × ₹576.92 = **₹3,461.52**

**Recommendation (matches the existing examples):** one payslip line **per reason per pay run**. Days × rate is computed exactly and rounded once on that line. Confirm.

### 3.15 PAY-04 — Mid-period joiner (answer: "date 15") — see CL-12

₹15,000 monthly, divisor 26, joins **Sunday 15 November 2026**. From 15 to 30 November there are 16 calendar days, of which 13 are working days (Sundays 15, 22, 29 excluded).

| Method | Calculation | Pay |
|---|---|---|
| **A. Per-day rate × working days** (consistent with LOP) | 13 × 15,000 ÷ 26 | **₹7,500.00** |
| **B. Calendar-day share** | 15,000 × 16 ÷ 30 | **₹8,000.00** |

A uses the same per-day rate as every deduction, so payslips explain themselves. B pays the weekly offs inside the employment period. **Recommendation:** A as the default, B as an option per payroll group. The same rules apply to leavers and to salary revisions (old rate before the date, new rate after).

### 3.16 PAY-03 — Salary floor (DEC-08 accepted)

- Monthly salary line = salary − deduction days × per-day rate, **never below 0**.
- **All working days absent ⇒ salary 0.** This fixes the divisor-30 case where February with 28 absent days would still pay ₹1,000.
- Example: December 2026 has 27 working days. Absent all 27 with divisor 26 ⇒ 27 × 576.92 = ₹15,576.92 deduction, capped ⇒ salary ₹0.

### 3.17 PER-02 — Correction after lock (accepted, with the reviewer's states)

Period lifecycle per lock unit (CL-17): **Open → Attendance locked → Finalised (final values) → Marked as paid**, plus an unlock path back to Open (DEC-07, Org Admin approval + reason).

When a locked day is corrected, the whole locked period is **recalculated in the background as a shadow copy**, including period-end rules. The difference is posted to the next open period, tagged with the original date. The locked period itself never changes.

Example: October has 3 late marks, so a half-day penalty (every 3rd). On 5 November the 14 October late mark is regularised. The October shadow now has 2 marks and no penalty, so November gets **+0.5 day reversal tagged "October penalty"**.

### 3.18 PER-04 — Automatic adjustments from "assumed present"

Cut-off on the 25th, PL-03 "assumed present": 26–31 October are paid as present in October. On 28 October the employee was actually absent, so November carries **−1 day, tagged 28 October, reason "actual after cut-off"**. Report R23 shows it as an *automatic* adjustment, separate from HR corrections.

### 3.19 PAY-06 — Month-based rules for weekly-paid workers

Weekly group, weeks Monday–Sunday. The week 26 Oct – 1 Nov straddles the month end. LM-26 counts uninformed absences "in the month".

**Recommendation:** month-based rules (LM-25/26, monthly permission limits) are worked out at **calendar month end**. The result goes into **the first pay run whose period ends after that month end** — here the week ending 1 November. The payslip line says "October excess absence".

### 3.20 PAY-07 — Recovery cap base

AL-08 caps recovery at 50% of net pay, but the recovery itself lowers net pay, which is circular. P11 already shows the answer: net ₹2,400 **before** recovery, cap 50% = ₹1,200. **Rule:** cap = AL-08 % × (earnings − attendance deductions), i.e. **before** advance and loan recoveries.

### 3.21 PAY-08 — Net-pay rounding difference

PY-06 nearest ₹10: net ₹14,504.37 → ₹14,500. **Recommendation:** show a payslip line "Net pay rounding −4.37". It's **not** carried forward. Totals still add up (AT10, AT11).

### 3.22 SEC-07 — Selfie retention (D6) — confirm

The TSD proposes deleting selfies after 90 days. But HR can now correct **any** past month (DEC-34), and disputes over a punch need its selfie. Deleting at 90 days means evidence disappears while corrections are still possible. **Recommendation: keep selfies 13 months** (a full leave year plus a month), then delete. At 1M employees with 30% selfie use that adds storage, still small next to the database line. Alternatively, keep only selfies attached to a flagged or disputed punch beyond 90 days.

### 3.23 ACC-05 — Maker-checker for settings (AC-04 option 2)

When "HR Manager with Org Admin approval" is chosen:
**Draft → Pending approval → Approved (scheduled for start date) → Active**, or **Pending → Rejected (back to draft)**.
Nothing changes results until Active. The previous version stays active meanwhile. Every step is in settings history (G9). Org Admin can't approve their own draft (BR-07).

### 3.24 ACC-06 — Who controls support-access approval (VC-03)

VC-03 means the vendor needs the Org Admin's approval each time it opens a client's account. If the **vendor** could switch VC-03 off, it could enter any client account without asking, and the control would mean nothing. **Recommendation:** only the **Organisation Admin** can change VC-03. The default stays on. Every change is logged and visible to the client.

### 3.25 DM-02 — Keeping old daily results

Today the TSD overwrites a day's result when it's recalculated. Example: on 5 November the employee is late (late mark). On 7 November the late is regularised and the day becomes present. If only "present" is kept, nobody can later show why the late-mark notification went out, or why the penalty count changed.

**Recommendation:** every recalculation **adds a new result version** (with the reason: new punch, approval, settings change). The latest version is current. Old versions stay for audit and are never shown as current. Storage is roughly 1.5–2× the daily results table.

### 3.26 TEC-02 — Finalising on the branch's local date (with a case)

**Case 1 — night shift, one time zone.** Shift 22:00–06:00, SH-07 window ends 6 hours after the shift, so 12:00 the next day. J1 runs at 03:00 on 4 November and finalises 3 November. But the night shift that *started* on 3 November is still being worked until 06:00, and punches for it can arrive until 12:00. The day is finalised **too early**.

**Case 2 — two time zones.** An org with Chennai (IST) and Dubai (UTC+4, 1.5 hours behind). 03:00 IST is 01:30 in Dubai. Dubai's evening shift punches may still be arriving.

**Recommendation:** J1 runs **hourly**. It finalises a (branch, date) only when *branch-local time* is past **the latest shift window end for that date + 1 hour**. It works per branch, not "everyone at 03:00 IST".

### 3.27 TEC-05 — RPO and RTO

- **RPO** (how much data we can lose): with daily backups only, up to 24 hours of punches. With PITR (DEC-12), a few minutes.
- **RTO** (how long we can be down): the time to restore and switch over.
- 99.5% availability (N8) allows about **3.6 hours of downtime a month**.

**Recommendation:** RPO 15 minutes, RTO 4 hours, restore tested quarterly (TSD §15). Devices keep punches locally and the app has offline mode, so a short outage doesn't lose punches.

### 3.28 Worked examples — E18 (#3) and the missing cases (#4)

**E18 changes because of DEC-18.** A web check-in 450 m away, PC-04 = record and flag: **recorded, flagged, and counted provisionally**. If the manager rejects it, the punch is excluded through an adjudication record (never deleted). The day is recalculated, and if it's already locked, the change goes through PL-R4.

The missing examples are written out in §5.

---

## 4. Accepted proposals (now requirements)

| ID | Requirement |
|---|---|
| CON-02 | Outbox table written in the same transaction as the punch/approval. A worker drains it to Cloud Tasks. A nightly sweeper re-queues any day whose inputs changed after its last result. |
| CON-05 | The Phase 1 domain model names the owning module for every aggregate, even inside one service. |
| CON-06 | A **punch adjudication** record: exclude, change attendance date, or change direction, with actor, reason and approval. Raw punches untouched. |
| CON-07 | Skill §3 updated to say Supabase is used only as Postgres hosting. |
| DOC-01 | BRD §22 refers to TSD v0.2. |
| DOC-05 | PITR from first live client (DEC-12). |
| DOC-07 | MFA **mandatory** for vendor users. For org admin roles, default on (AC-08), and the org may turn it off. |
| DOC-08 | Size D (1M employees) added to TSD §18/§19 (see CL-14). |
| REL-01 | V1 payroll groups (DEC-06). |
| REL-09 | HR raises leave, permission, on-duty and regularisation requests on behalf of an employee. The request records "raised by HR for employee". The approver must be someone other than that HR user. |
| REL-10 | Punch import format — §6. |
| PUN-08 | PC-R9 manual attendance and RG-10 HR correction create **manual punch** or **day override** records, both separate from raw punches, with a reason. |
| PUN-09 | "Hold check-in" slab action: the punch is stored, and the day is held as *pending manager approval*. Until approved, the late slab's cut applies provisionally. |
| ATT-02 | Overlapping windows: org option **nearest shift boundary** (default) / **earlier shift wins**. Every overlapping punch is flagged. |
| ATT-04 | Auto-detect shift: among the shifts allowed for the employee, choose the shift whose **start is nearest the first punch**, within that shift's window. On a tie, the earlier shift. If none matches, flag. |
| ATT-05 | Split shift = one shift with a **fixed or windowed break** of any length. Late is measured against the first segment start, early against the last segment end. The half-day point is the break start. |
| ATT-09 | Canonical order — §7. |
| ATT-19 | A new policy version may be back-dated only into **open** periods. Back-dating into a locked period needs an unlock (DEC-07). |
| ATT-21 | LA-12 default debit order: Casual → LOP. Sick is optional (UA-05). |
| LV-02 | LM-25/LM-10 count holidays inside a leave stretch: option, default **no**. |
| LV-06 | Optional holidays: the employee picks from the list up to HD-01. Manager approval. Cancellable before the date. |
| LV-09 | CO-R4 choice: an option on the holiday-work punch / request. Default **comp-off**. |
| LV-10 | Unpaid permission and PM-18 unpaid time go into the export **in minutes**, with hours and day-equivalents alongside. |
| PER-01 | BR-06 reworded: "A locked period changes only through the audited unlock workflow (PL-06); otherwise corrections post the difference to the next open period." |
| PAY-05 | Per-minute rate = per-day rate ÷ the employee's **required daily hours** (SH-03) ÷ 60. |
| PAY-09 | Held pay: the run excludes the employee's net pay. On release, the held amount is paid in the next run as a separate "released hold" line, tagged with the original run. |
| PAY-10 | CTC is a field on the employee master. |
| PAY-11 | Daily pay is worked out next morning only. The "at check-out" option (PY-08) is removed. |
| CR-01 | Client-rules **microservice** (DEC-09): one service hosting per-org rule versions. The pay run calls it at the defined hook points (CR-R1) through a versioned contract. Pure, deterministic functions. The rule version is stored on every pay-run line. Releasing a client rule redeploys only this service. |
| CR-02 | If the org has no closed period, the test run (CR-R4) uses the **standard base calculation on sample data** and shows the differences. |
| SEC-04 | Spike: SQLx + Supabase transaction pooler + per-transaction org setting, before database design. |
| SEC-06 | A privacy review (DPDP Act) before the first live client. No compliance claims before it. |
| SEC-08 | The system never stores fingerprint or face templates (see CL-06). Selfies are treated as sensitive personal data. |
| SEC-09 | Bank/ID numbers: envelope encryption with **Cloud KMS** keys per environment, data keys per org. Last 4 digits stored separately, unencrypted, for masked display and search. Yearly key rotation. |
| SEC-10 | Audit log: the database role has no UPDATE/DELETE on it, and a trigger blocks them. Each row carries a hash of the previous row (tamper evidence). Nothing is deleted. |
| SEC-12 | Client IP taken from the Google load balancer's `X-Forwarded-For` (the address Google appends), never from a client-set header. |
| DM-01 | Missing entities added to the domain model (list in Open Issues DM-01). |
| DM-03 | State machines produced in Phase 2 for every lifecycle listed. |
| DM-04 | Recalculation takes a per-(employee, date) lock and stores the input version. An older result can never overwrite a newer one. |
| TEC-03 | Limits: bulk recalculation runs at most 50 tasks per second per org and 500 overall. A settings change for 2,000 employees × 31 days (62,000 days) finishes within 30 minutes. A G8 preview over one period for 2,000 employees returns within 10 minutes, in the background. |
| TEC-04 | Dashboards refresh every 15 minutes by default, with a reload button for fresh data. |
| TEC-06 | Android 8+, iOS 15+. Latest two versions of Chrome, Edge, Firefox and Safari. |
| PUN-10 / DEC-21 | Receipt time is used only for **live** pushes (received within 2 minutes of device time). For late-synced batches, device time is kept and the punch is flagged. Otherwise, all punches from a device that was offline would be stamped with the sync time. |

---

## 5. New worked examples

Defaults as in BRD §13 unless stated.

**W1. Night shift across midnight.** Shift 22:00–06:00 (night flag), SH-06 = start date. In 3 Nov 21:52, out 4 Nov 06:04. Attendance date **3 Nov**, on time, no early mark. The day is finalised after 4 Nov 13:00 (window end 12:00 + 1 h, §3.26).

**W2. Overlapping windows.** Night shift 3 Nov 22:00–06:00 (window to 4 Nov 12:00). Day shift 4 Nov 09:00–18:00 (window from 06:00). Punch 4 Nov 08:55 falls in both. *Nearest boundary* (default): 5 minutes to the day-shift start vs 2h55 to the night-shift end, so it's assigned to the **4 Nov day shift**. Flagged as overlap.

**W3. Split shift with window break.** 09:00–21:00, window break 13:00–17:00, required 8 h. Punches 09:05, 13:02, 17:10, 21:00. Late 5 minutes, within grace. Break 4h08, 8 minutes over → BK-06 deducts from hours. Net = 11h55 − 4h08 = **7h47**. Shortfall 13 min < 15 min tolerance → **present, full day**.

**W4. Mid-period joiner.** See §3.15: method A ₹7,500.00, method B ₹8,000.00.

**W5. Mid-month transfer.** Branch A from 1–15 Nov, Branch B from 16–30 Nov. Branch A holiday 25 Nov, Branch B holiday 10 Nov. **Neither applies** — on 10 Nov he was at A, on 25 Nov at B. Late-mark and permission counters **continue** across the transfer (they belong to the employee). One period, one payslip.

**W6. Salary floor.** December 2026, 27 working days, all absent, divisor 26: deduction capped, **salary ₹0** (§3.16).

**W7. Weekly worker crossing a month.** Week 26 Oct – 1 Nov. LM-26: X = 3, Y = 1. Uninformed days in October: 2, 9, 16, 27 October = 4. One day beyond X → **1 day cut, paid in the week ending 1 Nov run**, line "October excess uninformed absence" (§3.19).

**W8. Post-lock correction changes a period penalty.** §3.17: **+0.5 day reversal in November**, tagged October.

**W9. Flagged punch counted, then rejected.** In 09:00 (450 m outside the fence, flagged, counted), out 18:00. Present. Manager rejects on 5 Nov → the in-punch is excluded → the day becomes single punch → MP-02 flag for regularisation (§3.28).

**W10. Offline punch with a moved clock.** §3.4: recorded at **09:02**, flagged for a clock mismatch.

**W11. Absent, merged rules.** §3.1: 10 Nov "Absent – pending" → 13 Nov Casual debited; no balance → LOP.

---

## 6. Punch import format (REL-10)

One CSV (UTF-8, header row) or the same fields as a JSON array to `POST /v1/punches/import`.

| Column | Required | Format | Rule |
|---|---|---|---|
| `external_id` | Yes | text ≤ 64 | Unique per org and source. A repeat import of the same ID is ignored (no duplicate). |
| `employee_code` | One of | text | Matched within the org. |
| `enrolment_id` | One of | text | Used with `device_serial` if no employee code. |
| `device_serial` | No | text | Must be a registered device if given. |
| `punch_time` | Yes | ISO 8601 with offset, e.g. `2026-11-04T09:02:00+05:30` | Stored as UTC. |
| `direction` | No | `IN` / `OUT` | Blank = untagged (CL-02). |
| `branch_code` | No | text | Defaults to the device's branch, else the master branch. |
| `latitude`, `longitude`, `accuracy_m` | No | decimal | Geo-fence checked if present. |
| `note` | No | text ≤ 200 | |

- Imported punches are marked with source `import`, the importing user, and the file checksum.
- Rows that fail validation are listed in an error report. Valid rows are kept (no all-or-nothing).
- Who can import: HR Manager. Import must also be on in PC-01.

---

## 7. Canonical daily calculation order (ATT-09)

For one employee and one attendance date:

1. **Context:** employee status, master branch on that date, payroll group, policy set version in force (BR-05).
2. **Expected schedule:** shift or roster entry, night flag, weekly off, holiday from **the branch's calendar for that date** (DEC-26).
3. **Collect punches** in the shift window (SH-07). Overlaps use ATT-02. Apply adjudications (CON-06), manual punches and day overrides.
4. **Clean:** repeat-punch window (PR-02/03), minimum gap (PR-04).
5. **Read into sessions:** direction-tagged or first-in/last-out (CL-02). Untagged punches are never paired by count.
6. **Validate:** maximum session length (PR-05), maximum punches (PR-06), missed punch (MP-01/02). Flagged punches count provisionally (DEC-18).
7. **Hours:** gross → breaks (BK) → net hours (unrounded).
8. **Overlays that change the expected time:** full-day leave / holiday / weekly off / on-duty (day not late or early); half-day leave and approved permission move the expected start or end.
9. **Late and early**, in the BRD §12 order of checks: grace → waiver/regularisation → permission conversion → compensation → slab action. A day cut replaces minute deductions (§3.2).
10. **Day status** (DS-01 basis, from unrounded hours), then daily rounding of net hours for reporting (§3.8).
11. **Absence handling** (UA, §3.1) and flags; send exceptions to regularisation.
12. **Store** a new result version with its explanation: punches, rules, versions, approvals (BR-08, DM-02).

**At period end** (after regularisation is settled, BR-10): count-based and minute-based penalties, excess absence (LM-24), absconding (flag only), week-based compensation still open.

---

## 8. Document changes required

**BRD → v0.9**
- §4.1 / §21: payroll groups in V1 (DEC-06). Overtime fully in V3 (DEC-14, CL-08). Reimbursements in V3, leave encashment in V2 (DEC-16). MD-04: advances need Payroll (DEC-15); remove AL-R8.
- §5: remove Auditor (DEC-29). Supervisor in V3. Add "HR raises requests on behalf of an employee".
- PC-01: remove kiosk. Add the import format (§6). PR-01: two modes (CL-02). Remove SH-01 open and SH-03 weekly. Night is a flag. Remove quarter-day options. Remove HD-02 move. Remove AC-03.
- Replace DS-10, LM-28, LV-04, LA-12/EC-11 with UA-01–05 (DEC-25).
- BR-06 reworded (PER-01). BR-04 plus the PD block (§3.2). BR-15: one decimal, half up, one line per reason (DEC-42, §3.14). TB-05 to match. Add the salary floor (DEC-08).
- UM-R3: employee code + PIN, email code optional (DEC-40). TB-09: one shared project with per-environment database and service account (DEC-43).
- Recompute P3, P6, P7, P12, P17 at one decimal (§1.1).
- E18 updated. W1–W11 added to §13.
- Close Q14, Q16 (CL-01). Update Q6, Q9, Q12.

**TSD → v0.3**
- §2/§8: replace Firebase phone sign-in with employee code + PIN; email codes via ZeptoMail (DEC-40). §5.3 rounding to one decimal (DEC-42).
- §3.2/§5.2: outbox and sweeper. Result versions. Per-key lock.
- §4.2: remove the cache line. §4.1: Data API disabled. §10: J1 hourly per branch.
- §11: selfie retention per §3.22. §15: KMS, audit hash chain, PITR from launch.
- §16: one shared project for dev/test/live, separate database and service account per environment (DEC-43). §18/§19: size D and repricing without SMS.
- Add the client-rules microservice (DEC-09).

**Skill (`Skills/SKILL.md`)**
- §3: Mobile = Flutter (DEC-02). No Pub/Sub for now; Cloud Tasks + outbox (DEC-03). Supabase only as Postgres hosting.
- §4 Phase 10: offline punch capture is V1 (DEC-04). Offline clients still never finalise results.

---

## 9. Still on hold or later

| ID | Topic |
|---|---|
| PUN-01 / D1 | eSSL HTTPS test — not run yet. Agreed 24 November 2026: Package 2 goes ahead without it; only the eSSL gateway story waits for the test. |
| ATT-08 | **Decided 24 November 2026 (DEC-50):** master branch time zone. |
| DOC-02 | Close Q16 in the BRD — after CL-01 |
| DOC-06 | Languages "without code change" |
| PAY-12 | V2 final settlement flow — discuss later |
| PAY-13 / PAY-14 | Bank / Tally formats; daily allowance logic — later |
| SEC-01 / SEC-02 | Device auth risk accepted (CL-07). Note: the pre-tenant lookup (SEC-02) is a technical necessity, not a scenario choice — it will be handled in the database design. |
| LEG-05 | Check assumption A3 is in the client contract |
| Q5 | Plan contents |
| W-P6/P7/P16, E15 | Pay-line examples belong to V2 (answered "V2") |
