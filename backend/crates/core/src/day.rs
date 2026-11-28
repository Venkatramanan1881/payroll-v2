//! DayCalculator (DOMAIN_MODEL §5): one employee, one attendance date, the canonical order of Issue
//! Resolutions §7 steps 2–11. Pure: the caller gathers the schedule, the punches left after adjudication
//! (manual ones included), the overlays and the settings; this works out the result and its explanation.
//! All times are local to the master branch (BR-11); the caller converts from UTC.
//! Period-end work (count- and minute-based penalties, LM-24, absconding, same-week compensation) is not here (BR-10).

use crate::policy::*;
use crate::shifts::{HalfDayPoint, NightDateRule, ShiftKind, ShiftTimes};
use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    In,
    Out,
    Untagged,
}

#[derive(Debug, Clone)]
pub struct Punch {
    pub id: Uuid,
    pub at: NaiveDateTime,
    pub dir: Dir,
    /// Where it was made (the day is costed there, E17).
    pub branch: Option<Uuid>,
    /// Flagged and waiting for review: counted provisionally (DEC-18).
    pub flagged: bool,
}

pub type Span = (NaiveDateTime, NaiveDateTime);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HalfLeave {
    FirstHalf,
    SecondHalf,
}

/// Approved things from other modules that change the expected time (§7 step 8). Leave, permissions and
/// on-duty arrive with Package 3; until then the caller passes the default (none).
#[derive(Debug, Clone, Default)]
pub struct Overlays {
    pub half_leave: Option<HalfLeave>,
    /// Approved permissions: their time counts as worked (E9).
    pub permissions: Vec<Span>,
    /// Permissions left this period, for LA-15 / EC-13 automatic conversion (E3).
    pub permissions_left: u32,
    /// The length a converted permission is taken in (rounded up to it).
    pub permission_unit_minutes: u32,
    /// Approved on-duty: counts as worked, and the day is not late or early where it covers (E16).
    pub on_duty: Vec<Span>,
    /// Bulk waiver (LA-17 / EC-16) or approved late / early regularisation for this day.
    pub late_waived: bool,
    pub early_waived: bool,
    /// EC-14 prior approval to leave early; EA-01 "punch time only when approved".
    pub early_exit_approved: bool,
    pub early_arrival_approved: bool,
}

/// Everything known about the day.
#[derive(Debug, Clone)]
pub struct DayInput<'a> {
    pub date: NaiveDate,
    /// The expected shift (roster entry or default shift); None = no shift that day.
    pub shift: Option<&'a ShiftTimes>,
    pub weekly_off: bool,
    /// A holiday from the branch calendar (an optional one only when picked and approved).
    pub holiday: bool,
    /// HD-03: present on the nearest working day before and after (§3.12); None = not known yet.
    pub presence_around: Option<bool>,
    pub punches: Vec<Punch>,
    pub overlays: Overlays,
    pub settings: &'a AtSettings,
    /// Grace uses left this period (LA-04, EC-03); None = unlimited.
    pub late_grace_left: Option<u32>,
    pub early_grace_left: Option<u32>,
    /// MP-03 applies: the regularisation window or the lock has passed (§3.6).
    pub past_cutoff: bool,
    /// The day the calculation runs, for "Absent – pending" (UA-01).
    pub today: NaiveDate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Present,
    HalfDay,
    Absent,
    AbsentPending,
    WeeklyOff,
    Holiday,
    HolidayUnpaid,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Session {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub minutes: i64,
    pub branch: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayOutcome {
    pub status: Status,
    pub first_in: Option<NaiveDateTime>,
    pub last_out: Option<NaiveDateTime>,
    pub sessions: Vec<Session>,
    pub gross_minutes: i64,
    pub break_minutes: i64,
    /// Unrounded; day status uses this (§3.8).
    pub net_minutes: i64,
    pub net_minutes_rounded: i64,
    pub late_minutes: i64,
    pub early_minutes: i64,
    pub late_mark: bool,
    pub early_mark: bool,
    /// Cut by a rule: 0, 0.5 or 1 (DEC-24). Plain absence is handled by UA, not here.
    pub deduction_days: f64,
    /// Minutes to deduct (slab "deduct actual minutes", DS-05); replaced by a day cut (§3.2).
    pub deduct_minutes: i64,
    pub late_grace_used: bool,
    pub early_grace_used: bool,
    /// A permission taken by automatic conversion (LA-15 / EC-13).
    pub permission_used: Option<Span>,
    pub flags: Vec<&'static str>,
    /// Minutes worked per branch where punched (E17).
    pub costed: Vec<(Option<Uuid>, i64)>,
    /// The explanation: (setting, what it did) in the order applied (BR-08).
    pub rules: Vec<(&'static str, String)>,
}

pub const MISSED_PUNCH: &str = "missed_punch";
pub const UNTAGGED_PUNCH: &str = "untagged_punch";
pub const SESSION_TOO_LONG: &str = "session_too_long";
pub const TOO_MANY_PUNCHES: &str = "too_many_punches";
pub const AUTO_CHECK_OUT: &str = "auto_check_out";
pub const FLAGGED_PUNCH: &str = "flagged_punch";
pub const EXCESS_BREAK: &str = "excess_break";
pub const BREAK_OUTSIDE_WINDOW: &str = "break_outside_window";
pub const PERMISSION_SUGGESTED: &str = "permission_suggested";
pub const HELD_CHECK_IN: &str = "held_check_in";
pub const UNAPPROVED_EXIT: &str = "unapproved_exit";
pub const PROVISIONAL_LATE: &str = "late_compensation_pending";
pub const MAKE_UP_PENDING: &str = "make_up_pending";
pub const PROPORTIONAL: &str = "proportional_pay";
pub const OVERLAP: &str = "window_overlap";
pub const SHIFT_NOT_DETECTED: &str = "shift_not_detected";
pub const NO_ROSTER: &str = "no_roster";

fn mins(a: NaiveDateTime, b: NaiveDateTime) -> i64 {
    (b - a).num_minutes()
}

/// Minutes from `start` forward to `x`, past midnight when needed.
fn after(start: NaiveTime, x: NaiveTime) -> i64 {
    (x - start).num_minutes().rem_euclid(24 * 60)
}

/// The shift's start and end on attendance date `date` (SH-06: a night shift dated by its end starts the day before).
pub fn bounds(date: NaiveDate, sh: &ShiftTimes) -> Span {
    let day = if sh.is_night && sh.night_date_rule == NightDateRule::EndDate { date - Duration::days(1) } else { date };
    let start = day.and_time(sh.start_time);
    let len = match after(sh.start_time, sh.end_time) {
        0 => 24 * 60,
        n => n,
    };
    (start, start + Duration::minutes(len))
}

/// The punch window (SH-07).
pub fn window(date: NaiveDate, sh: &ShiftTimes) -> Span {
    let (s, e) = bounds(date, sh);
    (s - Duration::minutes(sh.window_before_min as i64), e + Duration::minutes(sh.window_after_min as i64))
}

/// Which attendance date a punch belongs to, among the windows of the days around it. Two windows: the nearest
/// shift boundary or the earlier shift wins (ATT-02 option), and the punch is flagged as an overlap (W2). None = in no window.
pub fn assign(at: NaiveDateTime, days: &[(NaiveDate, &ShiftTimes)], rule: ShiftOverlap) -> Option<(NaiveDate, bool)> {
    let hits: Vec<_> = days
        .iter()
        .filter(|(d, sh)| {
            let (a, b) = window(*d, sh);
            a <= at && at <= b
        })
        .map(|(d, sh)| {
            let (s, e) = bounds(*d, sh);
            let key = match rule {
                ShiftOverlap::NearestBoundary => mins(at, s).abs().min(mins(e, at).abs()),
                ShiftOverlap::EarlierShift => mins(at, s), // smaller = starts earlier
            };
            (*d, key)
        })
        .collect();
    let best = hits.iter().min_by_key(|h| h.1)?;
    Some((best.0, hits.len() > 1))
}

/// SH-08 auto-detect (ATT-04, AC-AT-03.1–2): the first punch inside any candidate's window on `date` picks the
/// candidate whose start is nearest it, within that candidate's window; on a tie the earlier start. None = no
/// candidate's window holds any punch (the caller keeps the default shift and flags the day).
pub fn detect(date: NaiveDate, punches: &[NaiveDateTime], shifts: &[&ShiftTimes]) -> Option<usize> {
    let inside = |i: usize, at: NaiveDateTime| {
        let (a, b) = window(date, shifts[i]);
        a <= at && at <= b
    };
    let first = punches.iter().copied().filter(|at| (0..shifts.len()).any(|i| inside(i, *at))).min()?;
    (0..shifts.len()).filter(|i| inside(*i, first)).min_by_key(|i| {
        let start = bounds(date, shifts[*i]).0;
        (mins(first, start).abs(), start)
    })
}

/// A time of day placed inside the shift that starts at `start`.
fn in_shift(start: NaiveDateTime, t: NaiveTime) -> NaiveDateTime {
    start + Duration::minutes(after(start.time(), t))
}

/// (end of the first half, start of the second half) per SH-11.
fn halves(sh: &ShiftTimes, (s, e): Span) -> (NaiveDateTime, NaiveDateTime) {
    let mid = s + Duration::minutes(mins(s, e) / 2);
    match sh.half_day_point {
        HalfDayPoint::Middle => (mid, mid),
        HalfDayPoint::FixedTime => sh.half_day_time.map(|t| (in_shift(s, t), in_shift(s, t))).unwrap_or((mid, mid)),
        HalfDayPoint::BreakStart => match (sh.break_start, sh.break_end) {
            (Some(a), b) => (in_shift(s, a), in_shift(s, b.unwrap_or(a))),
            _ => (mid, mid),
        },
    }
}

/// Merges overlapping spans; returns them sorted.
fn union(mut spans: Vec<Span>) -> Vec<Span> {
    spans.retain(|(a, b)| a < b);
    spans.sort();
    let mut out: Vec<Span> = Vec::new();
    for (a, b) in spans {
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

fn covers(spans: &[Span], t: NaiveDateTime) -> bool {
    spans.iter().any(|(a, b)| *a <= t && t <= *b)
}

fn slab_for(slabs: &[Slab], minutes: i64) -> Option<SlabAction> {
    slabs.iter().find(|s| minutes >= s.from as i64 && s.to.is_none_or(|t| minutes <= t as i64)).map(|s| s.action)
}

fn round(n: i64, s: &AtSettings) -> i64 {
    if !s.rounding || s.rounding_unit_minutes == 0 {
        return n;
    }
    let u = s.rounding_unit_minutes as i64;
    match s.rounding_direction {
        Direction::Down => n / u * u,
        Direction::Up => (n + u - 1) / u * u,
        Direction::Nearest => (n + u / 2) / u * u,
    }
}

/// Steps 4–6: clean, read into sessions, validate. Returns (sessions, an "in" left open, any punch left unpaired).
fn read(punches: &[Punch], s: &AtSettings, flags: &mut Vec<&'static str>, rules: &mut Vec<(&'static str, String)>) -> (Vec<Session>, Option<Punch>, bool) {
    let mut ps: Vec<Punch> = punches.to_vec();
    ps.sort_by_key(|p| p.at);
    // PR-02/03: punches of the same direction closer than the window are one punch.
    let mut kept: Vec<Punch> = Vec::new();
    for p in ps {
        match kept.last_mut() {
            Some(last) if last.dir == p.dir && (p.at - last.at).num_seconds() < s.repeat_punch_seconds as i64 => {
                if s.repeat_punch_keep == KeepPunch::Last {
                    *last = p;
                }
            }
            _ => kept.push(p),
        }
    }
    if kept.len() as u32 > s.max_punches_per_day {
        flags.push(TOO_MANY_PUNCHES);
    }
    // PR-01: untagged punches are never paired by count; the day is read first-in / last-out (DEC-41).
    let filo = s.reading_mode == ReadingMode::FirstInLastOut || kept.iter().any(|p| p.dir == Dir::Untagged);
    if filo && s.reading_mode == ReadingMode::DirectionTagged {
        flags.push(UNTAGGED_PUNCH);
        rules.push(("PR-01", "untagged punches: read first-in / last-out".into()));
    }
    let mut sessions = Vec::new();
    let (mut open, mut unpaired): (Option<Punch>, bool) = (None, false);
    if filo {
        match kept.len() {
            0 => {}
            1 => open = Some(kept[0].clone()),
            n => sessions.push(Session { start: kept[0].at, end: kept[n - 1].at, minutes: 0, branch: kept[0].branch }),
        }
    } else {
        for p in kept {
            match (p.dir, open.take()) {
                (Dir::In, prev) => {
                    unpaired |= prev.is_some();
                    open = Some(p);
                }
                (_, Some(i)) => sessions.push(Session { start: i.at, end: p.at, minutes: 0, branch: i.branch }),
                (_, None) => unpaired = true,
            }
        }
    }
    // PR-04: an out-then-in gap shorter than the minimum is continuous work.
    let mut merged: Vec<Session> = Vec::new();
    for x in sessions {
        match merged.last_mut() {
            Some(last) if mins(last.end, x.start) < s.min_gap_minutes as i64 => last.end = x.end,
            _ => merged.push(x),
        }
    }
    // PR-05: longer than the maximum is a missed punch, not a session.
    merged.retain(|x| {
        let ok = mins(x.start, x.end) <= s.max_session_hours as i64 * 60;
        if !ok {
            flags.push(SESSION_TOO_LONG);
            unpaired = true;
        }
        ok
    });
    for x in merged.iter_mut() {
        x.minutes = mins(x.start, x.end);
    }
    (merged, open, unpaired)
}

pub fn calculate(d: &DayInput) -> DayOutcome {
    let s = d.settings;
    let mut flags: Vec<&'static str> = Vec::new();
    let mut rules: Vec<(&'static str, String)> = Vec::new();
    if d.punches.iter().any(|p| p.flagged) {
        flags.push(FLAGGED_PUNCH);
    }
    let (mut sessions, open, unpaired) = read(&d.punches, s, &mut flags, &mut rules);
    let span = d.shift.map(|sh| bounds(d.date, sh));

    // MP-05 / MP-01 / MP-02 / MP-03: a missing punch (BR-09: never an early check-out).
    let missed = open.is_some() || unpaired;
    let mut missed_rule = None;
    if missed {
        let rule = if d.past_cutoff {
            match s.unresolved_at_cutoff {
                CutoffRule::PairsOnly => MissedRule::PairsOnly,
                CutoffRule::HalfDay => MissedRule::HalfDay,
                CutoffRule::Absent => MissedRule::Absent,
            }
        } else if sessions.is_empty() {
            s.single_punch
        } else {
            s.odd_punches
        };
        let close_at = match (s.auto_check_out, rule, span) {
            (AutoCheckOut::ShiftEnd, _, Some((_, e))) | (AutoCheckOut::Off, MissedRule::AutoClose, Some((_, e))) => Some(e),
            (AutoCheckOut::FixedTime, _, Some((st, _))) => s.auto_check_out_time.map(|t| in_shift(st, t)),
            _ => None,
        };
        match (&open, close_at) {
            (Some(i), Some(e)) if e > i.at => {
                sessions.push(Session { start: i.at, end: e, minutes: mins(i.at, e), branch: i.branch });
                flags.push(AUTO_CHECK_OUT);
                rules.push(("MP-05", format!("closed at {}", e.time())));
            }
            _ => {
                flags.push(MISSED_PUNCH);
                rules.push((if d.past_cutoff { "MP-03" } else if sessions.is_empty() { "MP-02" } else { "MP-01" }, format!("{rule:?}")));
                missed_rule = Some(rule);
            }
        }
    }
    let auto_closed = flags.contains(&AUTO_CHECK_OUT);

    // EA-01/02: work before the shift start counts from the start (or up to EA-02 earlier).
    if let (Some((st, _)), Some(first)) = (span, sessions.first_mut()) {
        if matches!(d.shift.map(|x| x.kind), Some(ShiftKind::Fixed | ShiftKind::Split)) && first.start < st {
            let from = match s.early_arrival_from {
                EarlyArrivalFrom::PunchTime => st - Duration::minutes(s.early_arrival_max_minutes as i64),
                EarlyArrivalFrom::PunchTimeIfApproved if d.overlays.early_arrival_approved => st - Duration::minutes(s.early_arrival_max_minutes as i64),
                _ => st,
            };
            first.start = first.start.max(from).min(first.end);
            first.minutes = mins(first.start, first.end);
        }
    }

    // Step 7: hours. Permissions and on-duty count as worked (E9, E16).
    let o = &d.overlays;
    let punched: Vec<Span> = sessions.iter().map(|x| (x.start, x.end)).collect();
    let worked = union(punched.iter().chain(&o.on_duty).chain(&o.permissions).copied().collect());
    let gross: i64 = worked.iter().map(|(a, b)| mins(*a, *b)).sum();
    let gaps: Vec<Span> = worked.windows(2).map(|w| (w[0].1, w[1].0)).collect();
    let gap_total: i64 = gaps.iter().map(|(a, b)| mins(*a, *b)).sum();
    let required = d.shift.map(|x| x.required_minutes as i64).unwrap_or(480);
    let shift_break = d.shift.and_then(|sh| Some((sh.break_start?, sh.break_end?))).zip(span).map(|((a, b), (st, _))| (in_shift(st, a), in_shift(st, b)));
    let mut extra_break = 0; // deducted on top of the gaps
    let mut early_from_break = 0;
    let allowance = match shift_break {
        Some((a, b)) => mins(a, b),
        None => s.break_cap_minutes.unwrap_or(s.break_minutes) as i64,
    };
    if s.break_paid {
        extra_break = -gap_total.min(s.break_minutes as i64);
    } else if shift_break.is_none() {
        match s.break_type {
            BreakType::None => {}
            // shortcut: the fixed break is deducted only from a day longer than half the required hours, so
            // a half day (E12) keeps its hours; revisit if a policy needs the break on every worked day.
            BreakType::FixedAutoDeduct if gross > required / 2 => extra_break = (s.break_minutes as i64 - gap_total).max(0),
            BreakType::FixedAutoDeduct => {}
            _ if gap_total == 0 && s.auto_deduct_unpunched && gross >= s.auto_deduct_after_hours.unwrap_or(0) as i64 * 60 => {
                extra_break = s.auto_deduct_minutes.unwrap_or(0) as i64
            }
            _ => {}
        }
    }
    // BK-06: break taken beyond the allowance.
    let excess = if matches!(s.break_type, BreakType::FixedAutoDeduct | BreakType::None) && shift_break.is_none() { 0 } else { (gap_total - allowance).max(0) };
    if excess > 0 {
        match s.excess_break {
            ExcessBreak::Deduct => rules.push(("BK-06", format!("{excess} minutes over the break deducted"))),
            ExcessBreak::EarlyMinutes => early_from_break = excess,
            ExcessBreak::Permission | ExcessBreak::FlagOnly => {
                extra_break -= excess;
                flags.push(EXCESS_BREAK);
            }
        }
    }
    // BK-07: a break outside the window.
    let win = shift_break.or_else(|| Some((s.break_window_start?, s.break_window_end?)).zip(span).map(|((a, b), (st, _))| (in_shift(st, a), in_shift(st, b))));
    if let (BreakType::Window, Some((a, b))) = (s.break_type, win) {
        if gaps.iter().any(|(x, y)| *x < a || *y > b) && s.break_outside_window != OutsideWindow::Deduct {
            flags.push(BREAK_OUTSIDE_WINDOW);
        }
    }
    let net = (gross - extra_break).max(0);
    let break_minutes = gap_total + extra_break;

    // Costed to the branch where punched (E17).
    let mut costed: Vec<(Option<Uuid>, i64)> = Vec::new();
    for x in &sessions {
        match costed.iter_mut().find(|c| c.0 == x.branch) {
            Some(c) => c.1 += x.minutes,
            None => costed.push((x.branch, x.minutes)),
        }
    }

    let mut out = DayOutcome {
        status: Status::Absent,
        first_in: sessions.first().map(|x| x.start).or(open.as_ref().map(|p| p.at)),
        last_out: sessions.last().map(|x| x.end),
        sessions,
        gross_minutes: gross,
        break_minutes,
        net_minutes: net,
        net_minutes_rounded: round(net, s),
        late_minutes: 0,
        early_minutes: 0,
        late_mark: false,
        early_mark: false,
        deduction_days: 0.0,
        deduct_minutes: 0,
        late_grace_used: false,
        early_grace_used: false,
        permission_used: None,
        flags,
        costed,
        rules,
    };

    // Step 8: a holiday or weekly off is neither late nor early; the hours are kept (HD-02, DEC-38).
    if d.holiday || d.weekly_off {
        out.status = if !d.holiday {
            Status::WeeklyOff
        } else if s.holiday_needs_presence_around && d.presence_around == Some(false) {
            out.rules.push(("HD-03", "no presence on the working day before or after".into()));
            Status::HolidayUnpaid
        } else {
            Status::Holiday
        };
        return out;
    }
    let (Some(sh), Some((st, en))) = (d.shift, span) else {
        out.status = if out.net_minutes > 0 { Status::Present } else { absent(d, &mut out.rules) };
        return out;
    };
    let basis = match s.status_basis {
        StatusBasis::Auto if matches!(sh.kind, ShiftKind::Flexible | ShiftKind::FlexibleCore) => StatusBasis::Hours,
        StatusBasis::Auto => StatusBasis::Shift,
        b => b,
    };
    let flexible = matches!(sh.kind, ShiftKind::Flexible | ShiftKind::FlexibleCore);
    let (first_end, second_start) = halves(sh, (st, en));
    let real: Vec<Span> = union(punched.iter().chain(&o.on_duty).copied().collect());

    // Expected start and end, moved by half-day leave and permissions at the edges (§7 step 8, E12).
    let mut start_ref = match (flexible, s.late_flexible, sh.core_start, sh.flex_latest) {
        (false, ..) => Some(st),
        (true, FlexibleLate::Auto | FlexibleLate::CoreStart, Some(c), _) => Some(in_shift(st, c)),
        (true, FlexibleLate::LatestCheckIn, _, Some(l)) => Some(in_shift(st, l)),
        _ => None,
    };
    let mut end_ref = match (flexible, s.early_flexible, sh.core_end, sh.flex_earliest) {
        (false, ..) => Some(en),
        (true, FlexibleLate::Auto | FlexibleLate::CoreStart, Some(c), _) => Some(in_shift(st, c)),
        // shortcut: "latest check-in" for early check-out is read as the band's earliest allowed check-out end,
        // i.e. the shift end; refine if HR needs a separate earliest-leave time.
        (true, FlexibleLate::LatestCheckIn, _, _) => Some(en),
        _ => None,
    };
    let mut adjusted_start = false;
    let mut adjusted_end = false;
    let mut required_here = required;
    match o.half_leave {
        Some(HalfLeave::FirstHalf) => {
            start_ref = start_ref.map(|_| second_start);
            adjusted_start = true;
            required_here = required / 2;
        }
        Some(HalfLeave::SecondHalf) => {
            end_ref = end_ref.map(|_| first_end);
            adjusted_end = true;
            required_here = required / 2;
        }
        None => {}
    }
    if let Some(p) = o.permissions.iter().filter(|p| start_ref.is_some_and(|r| p.0 <= r && r < p.1)).map(|p| p.1).max() {
        start_ref = Some(p);
        adjusted_start = true;
    }
    if let Some(p) = o.permissions.iter().filter(|p| end_ref.is_some_and(|r| p.0 < r && r <= p.1)).map(|p| p.0).min() {
        end_ref = Some(p);
        adjusted_end = true;
    }

    // Step 9: late arrival in the §12 order.
    let first_real = real.first().map(|x| x.0);
    let mut late_cut: Option<SlabAction> = None;
    if let (true, Some(r), Some(f)) = (s.late_tracking, start_ref, first_real) {
        let raw = if covers(&o.on_duty, r) { 0 } else { mins(r, f).max(0) };
        // Back late from a mid-day permission: late minutes, no grace (§3.10, E10).
        let overstay: i64 = o
            .permissions
            .iter()
            .filter(|p| Some(p.1) != start_ref && p.1 < en)
            .filter_map(|p| real.iter().find(|x| x.0 >= p.1).map(|x| mins(p.1, x.0)))
            .filter(|m| *m > 0)
            .sum();
        let grace = if adjusted_start && !s.late_grace_on_adjusted_start || d.late_grace_left == Some(0) { 0 } else { s.late_grace_minutes as i64 };
        let mut late = if raw == 0 {
            0
        } else if raw <= grace {
            out.late_grace_used = true;
            out.rules.push(("LA-03", format!("{raw} minutes within grace")));
            0
        } else if s.late_measured_from == LateFrom::GraceEnd {
            raw - grace
        } else {
            raw
        };
        late += overstay;
        if overstay > 0 {
            out.rules.push(("PM", format!("{overstay} minutes back late from a permission")));
        }
        out.late_minutes = late;
        if late > 0 && o.late_waived {
            out.rules.push(("LA-17", "late waived".into()));
        } else if late > 0 && overstay == 0 && s.late_to_permission == ToPermission::Automatic && o.permissions_left > 0 && o.permission_unit_minutes > 0 {
            let unit = o.permission_unit_minutes as i64;
            let p = (r, r + Duration::minutes((raw + unit - 1) / unit * unit));
            out.permission_used = Some(p);
            out.late_minutes = 0;
            out.rules.push(("LA-15", format!("converted to a permission {}–{}", p.0.time(), p.1.time())));
        } else if late > 0 && basis == StatusBasis::Hours {
            out.rules.push(("DS-01", "hours-based: late reported, no penalty".into()));
        } else if late > 0 {
            if s.late_to_permission == ToPermission::Suggest {
                out.flags.push(PERMISSION_SUGGESTED);
            }
            let extra = out.last_out.filter(|_| !missed && !auto_closed).zip(end_ref).map_or(0, |(l, e)| mins(e, l).max(0));
            if s.late_compensation && s.late_compensation_window == CompensationWindow::SameDay && late <= s.late_compensation_max_minutes as i64 && extra >= late {
                out.rules.push(("LA-13", format!("{late} late minutes made up by {extra} extra")));
            } else {
                if s.late_compensation && s.late_compensation_window == CompensationWindow::SameWeek {
                    out.flags.push(PROVISIONAL_LATE);
                }
                match slab_for(&s.late_slabs, late) {
                    Some(SlabAction::Mark) => out.late_mark = true,
                    Some(SlabAction::DeductMinutes) => out.deduct_minutes += late,
                    Some(a @ (SlabAction::HalfDay | SlabAction::FullDay)) => {
                        late_cut = Some(a);
                        out.late_mark = s.late_slab_counts_as_mark;
                    }
                    Some(SlabAction::HoldCheckIn) => {
                        out.late_mark = true;
                        out.flags.push(HELD_CHECK_IN);
                    }
                    Some(SlabAction::None) | None => {}
                }
                out.rules.push(("LA-05", format!("{late} late minutes: {:?}", slab_for(&s.late_slabs, late))));
            }
        }
    }

    // Step 9 mirrored: early check-out. A missing or automatic check-out is never early (BR-09).
    let mut early_cut: Option<SlabAction> = None;
    let hours_met = match s.early_hours_met_override {
        AutoOnOff::On => true,
        AutoOnOff::Off => false,
        AutoOnOff::Auto => flexible,
    } && net >= required_here;
    let last_real = real.last().map(|x| x.1);
    if let (true, Some(r), Some(l), false, false) = (s.early_tracking, end_ref, last_real.filter(|_| open.is_none() && !auto_closed), hours_met, covers(&o.on_duty, end_ref.unwrap_or(en))) {
        let raw = mins(l, r).max(0) + early_from_break;
        let grace = if adjusted_end && !s.early_grace_on_adjusted_end || d.early_grace_left == Some(0) { 0 } else { s.early_grace_minutes as i64 };
        let early = if raw == 0 {
            0
        } else if raw <= grace {
            out.early_grace_used = true;
            0
        } else {
            raw
        };
        out.early_minutes = early;
        let compensated = s.early_compensation && o.early_arrival_approved && first_real.is_some_and(|f| mins(f, st) >= early);
        if early == 0 || basis == StatusBasis::Hours {
        } else if o.early_waived {
            out.rules.push(("EC-16", "early check-out waived".into()));
        } else if compensated {
            out.rules.push(("EC-12", "made up by approved early arrival".into()));
        } else if s.early_to_permission == ToPermission::Automatic && out.permission_used.is_none() && o.permissions_left > 0 && o.permission_unit_minutes > 0 {
            let unit = o.permission_unit_minutes as i64;
            let p = (r - Duration::minutes((early + unit - 1) / unit * unit), r);
            out.permission_used = Some(p);
            out.early_minutes = 0;
            out.rules.push(("EC-13", format!("converted to a permission {}–{}", p.0.time(), p.1.time())));
        } else if s.early_prior_approval && !o.early_exit_approved {
            match s.early_unapproved_exit {
                UnapprovedExit::Mark => out.early_mark = true,
                UnapprovedExit::HalfDay => early_cut = Some(SlabAction::HalfDay),
                UnapprovedExit::FlagHr => out.flags.push(UNAPPROVED_EXIT),
            }
            out.rules.push(("EC-14", "left early without approval".into()));
        } else {
            if s.early_to_permission == ToPermission::Suggest {
                out.flags.push(PERMISSION_SUGGESTED);
            }
            match slab_for(&s.early_slabs, early) {
                Some(SlabAction::Mark | SlabAction::HoldCheckIn) => out.early_mark = true,
                Some(SlabAction::DeductMinutes) => out.deduct_minutes += early,
                Some(a @ (SlabAction::HalfDay | SlabAction::FullDay)) => {
                    early_cut = Some(a);
                    out.early_mark = s.late_slab_counts_as_mark;
                }
                Some(SlabAction::None) | None => {}
            }
            out.rules.push(("EC-04", format!("{early} early minutes: {:?}", slab_for(&s.early_slabs, early))));
        }
    }

    // Step 10: day status from unrounded hours.
    let worked_any = !real.is_empty() || !o.permissions.is_empty();
    if !worked_any && !missed {
        out.status = if s.present_by_default {
            out.rules.push(("DS-09", "present by default".into()));
            Status::Present
        } else {
            absent(d, &mut out.rules)
        };
        return out;
    }
    let mut cut = 0.0_f64;
    let mut status = if out.sessions.is_empty() && o.on_duty.is_empty() {
        // Only an unpaired punch (E13, W9): waits for regularisation unless the rule says otherwise.
        match missed_rule {
            Some(MissedRule::HalfDay) => Status::HalfDay,
            Some(MissedRule::Absent | MissedRule::PairsOnly) | None => Status::Absent,
            Some(_) => Status::AbsentPending,
        }
    } else {
        Status::Present
    };
    if status == Status::Present {
        match missed_rule {
            Some(MissedRule::HalfDay) => status = Status::HalfDay,
            Some(MissedRule::Absent) => status = Status::Absent,
            _ => {}
        }
        let full_min = required_here * s.full_day_min() as i64 / 100;
        let half_min = required * s.half_day_min_percent as i64 / 100;
        if matches!(basis, StatusBasis::Hours | StatusBasis::Hybrid) && net < full_min - s.shortfall_tolerance_minutes as i64 && status == Status::Present {
            let short = full_min - net;
            if net < half_min && o.half_leave.is_none() {
                match s.below_half_day {
                    BelowHalf::Absent => status = Status::Absent,
                    BelowHalf::Proportional => {
                        status = Status::Absent;
                        out.flags.push(PROPORTIONAL);
                    }
                }
                out.rules.push(("DS-07", format!("{net} minutes is below the half-day minimum {half_min}")));
            } else {
                match s.shortfall_action {
                    Shortfall::HalfDay => status = Status::HalfDay,
                    Shortfall::DeductMinutes => out.deduct_minutes += short,
                    Shortfall::MakeUpWeek => {
                        out.deduct_minutes += short;
                        out.flags.push(MAKE_UP_PENDING);
                    }
                }
                out.rules.push(("DS-05", format!("{short} minutes short of {full_min}")));
            }
        }
        // A slab day cut (the larger of late and early) replaces minute deductions for the day (§3.2, BR-04).
        let worst = [late_cut, early_cut].into_iter().flatten().map(|a| if a == SlabAction::FullDay { 1.0 } else { 0.5 }).fold(0.0, f64::max);
        if worst > 0.0 && status == Status::Present {
            status = if worst >= 1.0 { Status::Absent } else { Status::HalfDay };
            cut = worst;
            out.deduct_minutes = 0;
        }
    }
    // Half-day leave: the worked half is the day's half-day (E12); the leave half is not a cut.
    if o.half_leave.is_some() && status == Status::Present {
        status = Status::HalfDay;
    } else if status == Status::HalfDay && cut == 0.0 {
        cut = 0.5;
    } else if status == Status::Absent && cut == 0.0 && worked_any {
        cut = 1.0;
    }
    out.status = status;
    out.deduction_days = cut;
    out
}

/// No work on a working day: "Absent – pending" for UA-01 days, then absent (§3.1); the debit is the leave module's.
fn absent(d: &DayInput, rules: &mut Vec<(&'static str, String)>) -> Status {
    if d.today < d.date + Duration::days(d.settings.absent_apply_days as i64 + 1) {
        rules.push(("UA-01", format!("pending until {}", d.date + Duration::days(d.settings.absent_apply_days as i64))));
        Status::AbsentPending
    } else {
        Status::Absent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shifts::BreakKind;

    const D: (i32, u32, u32) = (2026, 11, 3);

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(D.0, D.1, D.2).unwrap()
    }
    fn at(h: u32, m: u32) -> NaiveDateTime {
        date().and_hms_opt(h, m, 0).unwrap()
    }
    fn p(t: NaiveDateTime, dir: Dir) -> Punch {
        Punch { id: Uuid::nil(), at: t, dir, branch: None, flagged: false }
    }
    fn io(i: (u32, u32), o: Option<(u32, u32)>) -> Vec<Punch> {
        let mut v = vec![p(at(i.0, i.1), Dir::In)];
        v.extend(o.map(|o| p(at(o.0, o.1), Dir::Out)));
        v
    }
    /// §13 defaults: fixed 09:00–18:00, 60-minute fixed break, 8 hours, shift-based, grace 10 / 5, default slabs.
    fn day<'a>(sh: &'a ShiftTimes, s: &'a AtSettings, punches: Vec<Punch>) -> DayInput<'a> {
        DayInput {
            date: date(),
            shift: Some(sh),
            weekly_off: false,
            holiday: false,
            presence_around: None,
            punches,
            overlays: Overlays::default(),
            settings: s,
            late_grace_left: None,
            early_grace_left: None,
            past_cutoff: false,
            today: date() + Duration::days(10),
        }
    }
    fn hm(h: i64, m: i64) -> i64 {
        h * 60 + m
    }

    #[test]
    fn e1_e2_e6_e7_late_and_early_slabs() {
        let (sh, s) = (ShiftTimes::default(), AtSettings::default());
        let r = calculate(&day(&sh, &s, io((9, 8), Some((18, 0)))));
        assert_eq!((r.status, r.late_mark, r.late_minutes, r.late_grace_used, r.net_minutes), (Status::Present, false, 0, true, hm(7, 52)));
        let r = calculate(&day(&sh, &s, io((9, 25), Some((18, 0)))));
        assert_eq!((r.status, r.late_mark, r.late_minutes, r.deduction_days), (Status::Present, true, 25, 0.0));
        // E6: 150 late → half day, not a late mark (LA-23 no).
        let r = calculate(&day(&sh, &s, io((11, 30), Some((18, 0)))));
        assert_eq!((r.status, r.late_mark, r.late_minutes, r.deduction_days), (Status::HalfDay, false, 150, 0.5));
        let r = calculate(&day(&sh, &s, io((9, 0), Some((17, 20)))));
        assert_eq!((r.status, r.early_mark, r.early_minutes), (Status::Present, true, 40));
        let r = calculate(&day(&sh, &s, io((9, 0), Some((17, 57)))));
        assert_eq!((r.early_mark, r.early_grace_used), (false, true));
        // No grace uses left: 8 minutes late is late.
        let mut d = day(&sh, &s, io((9, 8), Some((18, 0))));
        d.late_grace_left = Some(0);
        assert_eq!(calculate(&d).late_minutes, 8);
    }

    #[test]
    fn e3_e4_e14_conversion_compensation_waiver() {
        let sh = ShiftTimes::default();
        let mut s = AtSettings { late_to_permission: ToPermission::Automatic, ..AtSettings::default() };
        let mut d = day(&sh, &s, io((9, 25), Some((18, 0))));
        d.overlays.permissions_left = 2;
        d.overlays.permission_unit_minutes = 30;
        let r = calculate(&d);
        assert_eq!((r.status, r.late_mark, r.permission_used), (Status::Present, false, Some((at(9, 0), at(9, 30)))));
        s.late_to_permission = ToPermission::Off;
        s.late_compensation = true;
        let r = calculate(&day(&sh, &s, io((9, 25), Some((18, 30)))));
        assert_eq!((r.status, r.late_mark, r.net_minutes), (Status::Present, false, hm(8, 5)));
        let mut d = day(&sh, &s, io((9, 25), Some((18, 0))));
        d.overlays.late_waived = true;
        assert!(!calculate(&d).late_mark);
    }

    #[test]
    fn e8_hours_based_flexible() {
        let sh = ShiftTimes { kind: ShiftKind::Flexible, flex_earliest: Some(NaiveTime::from_hms_opt(8, 0, 0).unwrap()), ..ShiftTimes::default() };
        let mut s = AtSettings::default();
        let r = calculate(&day(&sh, &s, io((10, 15), Some((18, 30)))));
        assert_eq!((r.net_minutes, r.status, r.late_mark, r.early_mark, r.deduction_days), (hm(7, 15), Status::HalfDay, false, false, 0.5));
        s.shortfall_action = Shortfall::DeductMinutes;
        let r = calculate(&day(&sh, &s, io((10, 15), Some((18, 30)))));
        assert_eq!((r.status, r.deduct_minutes), (Status::Present, 45));
    }

    #[test]
    fn e9_e10_e12_e16_overlays() {
        let (sh, s) = (ShiftTimes::default(), AtSettings::default());
        // E9: permission 16:00–18:00, out 16:02.
        let mut d = day(&sh, &s, io((9, 0), Some((16, 2))));
        d.overlays.permissions = vec![(at(16, 0), at(18, 0))];
        let r = calculate(&d);
        assert_eq!((r.status, r.early_mark, r.early_minutes, r.net_minutes), (Status::Present, false, 0, hm(8, 0)));
        // E10: permission 10:00–11:00, out 10:00, back 11:25 → 25 late minutes, no grace, a late mark.
        let mut d = day(&sh, &s, vec![p(at(9, 0), Dir::In), p(at(10, 0), Dir::Out), p(at(11, 25), Dir::In), p(at(18, 0), Dir::Out)]);
        d.overlays.permissions = vec![(at(10, 0), at(11, 0))];
        let r = calculate(&d);
        assert_eq!((r.status, r.late_minutes, r.late_mark), (Status::Present, 25, true));
        // E12: first-half leave, half point fixed at 14:00, in 14:20 → 20 late, a late mark; half day leave + half present.
        let sh12 = ShiftTimes { half_day_point: HalfDayPoint::FixedTime, half_day_time: Some(NaiveTime::from_hms_opt(14, 0, 0).unwrap()), ..ShiftTimes::default() };
        let mut d = day(&sh12, &s, io((14, 20), Some((18, 0))));
        d.overlays.half_leave = Some(HalfLeave::FirstHalf);
        let r = calculate(&d);
        assert_eq!((r.status, r.late_minutes, r.late_mark, r.deduction_days), (Status::HalfDay, 20, true, 0.0));
        // E16: on-duty 14:00–18:00, in 09:00, out 13:55 → present, full day, no early mark.
        let mut d = day(&sh, &s, io((9, 0), Some((13, 55))));
        d.overlays.on_duty = vec![(at(14, 0), at(18, 0))];
        let r = calculate(&d);
        assert_eq!((r.status, r.early_mark, r.early_minutes, r.deduction_days), (Status::Present, false, 0, 0.0));
    }

    #[test]
    fn e13_w9_missed_punches() {
        let (sh, s) = (ShiftTimes::default(), AtSettings::default());
        // E13: in, no out → flagged for regularisation, waits (never an early check-out, BR-09).
        let r = calculate(&day(&sh, &s, io((9, 0), None)));
        assert_eq!((r.status, r.flags.clone(), r.early_minutes, r.first_in), (Status::AbsentPending, vec![MISSED_PUNCH], 0, Some(at(9, 0))));
        // After approval the 18:05 check-out is a manual punch: present.
        let r = calculate(&day(&sh, &s, io((9, 0), Some((18, 5)))));
        assert_eq!(r.status, Status::Present);
        // W9: the in-punch excluded after rejection → single out punch → MP-02 flag.
        let r = calculate(&day(&sh, &s, vec![p(at(18, 0), Dir::Out)]));
        assert_eq!((r.status, r.flags.clone()), (Status::AbsentPending, vec![MISSED_PUNCH]));
        // MP-03 at the cut-off: pairs only, none → absent; MP-05 auto check-out closes at shift end, flagged.
        let mut d = day(&sh, &s, io((9, 0), None));
        d.past_cutoff = true;
        assert_eq!(calculate(&d).status, Status::Absent);
        let s5 = AtSettings { auto_check_out: AutoCheckOut::ShiftEnd, ..AtSettings::default() };
        let r = calculate(&day(&sh, &s5, io((9, 0), None)));
        assert_eq!((r.status, r.last_out, r.flags.clone()), (Status::Present, Some(at(18, 0)), vec![AUTO_CHECK_OUT]));
        // Odd punches with a full pair: still flagged, hours from the pair.
        let r = calculate(&day(&sh, &s, vec![p(at(9, 0), Dir::In), p(at(18, 0), Dir::Out), p(at(18, 30), Dir::In)]));
        assert_eq!((r.status, r.flags.contains(&MISSED_PUNCH)), (Status::Present, true));
    }

    #[test]
    fn e17_costed_to_branch_and_cleaning() {
        let (sh, s) = (ShiftTimes::default(), AtSettings::default());
        let b = Uuid::from_u128(7);
        let mut ps = io((9, 0), Some((18, 0)));
        ps.iter_mut().for_each(|x| x.branch = Some(b));
        // A repeat punch 30 s later is one punch (PR-02); a 3-minute out-in gap is continuous (PR-04).
        ps.push(Punch { at: at(9, 0) + Duration::seconds(30), ..ps[0].clone() });
        ps.push(Punch { at: at(12, 0), dir: Dir::Out, ..ps[0].clone() });
        ps.push(Punch { at: at(12, 3), dir: Dir::In, ..ps[0].clone() });
        let r = calculate(&day(&sh, &s, ps));
        assert_eq!((r.status, r.sessions.len(), r.costed.clone(), r.net_minutes), (Status::Present, 1, vec![(Some(b), hm(9, 0))], hm(8, 0)));
        // Untagged punches: first-in / last-out, flagged.
        let r = calculate(&day(&sh, &s, vec![p(at(9, 2), Dir::Untagged), p(at(13, 0), Dir::Untagged), p(at(18, 1), Dir::Untagged)]));
        assert_eq!((r.status, r.net_minutes, r.flags.clone()), (Status::Present, hm(7, 59), vec![UNTAGGED_PUNCH]));
    }

    #[test]
    fn w1_w2_night_shift_and_overlapping_windows() {
        let s = AtSettings::default();
        let night = ShiftTimes { start_time: NaiveTime::from_hms_opt(22, 0, 0).unwrap(), end_time: NaiveTime::from_hms_opt(6, 0, 0).unwrap(), is_night: true, ..ShiftTimes::default() };
        let next = |h, m| (date() + Duration::days(1)).and_hms_opt(h, m, 0).unwrap();
        let r = calculate(&day(&night, &s, vec![p(at(21, 52), Dir::In), p(next(6, 4), Dir::Out)]));
        assert_eq!((r.status, r.late_minutes, r.early_minutes, r.first_in), (Status::Present, 0, 0, Some(at(22, 0))));
        assert_eq!(window(date(), &night).1, next(12, 0)); // finalised after 13:00 (§3.26)
        // W2: 4 Nov 08:55 is in both windows; nearest boundary is the day shift's start.
        let dayshift = ShiftTimes::default();
        let tomorrow = date() + Duration::days(1);
        let both = [(date(), &night), (tomorrow, &dayshift)];
        assert_eq!(assign(next(8, 55), &both, ShiftOverlap::NearestBoundary), Some((tomorrow, true)));
        assert_eq!(assign(next(5, 50), &both, ShiftOverlap::NearestBoundary), Some((date(), false)));
        // ATT-02 option: the earlier shift (the night that started at 22:00) takes the overlapping punch, still flagged.
        assert_eq!(assign(next(8, 55), &both, ShiftOverlap::EarlierShift), Some((date(), true)));
        assert_eq!(assign(next(9, 30), &[(tomorrow, &dayshift), (date(), &night)], ShiftOverlap::EarlierShift), Some((date(), true)));
        // SH-06 end date: the 22:00 start is on the day before.
        let by_end = ShiftTimes { night_date_rule: NightDateRule::EndDate, ..night.clone() };
        assert_eq!(bounds(tomorrow, &by_end).0, at(22, 0));
    }

    #[test]
    fn w3_split_shift_window_break() {
        let t = |h, m| NaiveTime::from_hms_opt(h, m, 0).unwrap();
        let sh = ShiftTimes { kind: ShiftKind::Split, end_time: t(21, 0), break_kind: Some(BreakKind::Window), break_start: Some(t(13, 0)), break_end: Some(t(17, 0)), ..ShiftTimes::default() };
        let s = AtSettings::default();
        let r = calculate(&day(&sh, &s, vec![p(at(9, 5), Dir::In), p(at(13, 2), Dir::Out), p(at(17, 10), Dir::In), p(at(21, 0), Dir::Out)]));
        assert_eq!((r.status, r.net_minutes, r.break_minutes, r.late_mark, r.late_grace_used), (Status::Present, hm(7, 47), hm(4, 8), false, true));
        // Under hybrid the 13-minute shortfall is inside the 15-minute tolerance: still a full day.
        let s = AtSettings { status_basis: StatusBasis::Hybrid, full_day_min_percent: Some(100), ..AtSettings::default() };
        let r = calculate(&day(&sh, &s, vec![p(at(9, 5), Dir::In), p(at(13, 2), Dir::Out), p(at(17, 10), Dir::In), p(at(21, 0), Dir::Out)]));
        assert_eq!((r.status, r.deduct_minutes), (Status::Present, 0));
    }

    #[test]
    fn holidays_absence_rounding() {
        let sh = ShiftTimes::default();
        let mut s = AtSettings::default();
        let mut d = day(&sh, &s, vec![]);
        d.holiday = true;
        assert_eq!(calculate(&d).status, Status::Holiday);
        s.holiday_needs_presence_around = true;
        let mut d = day(&sh, &s, vec![]);
        d.holiday = true;
        d.presence_around = Some(false);
        assert_eq!(calculate(&d).status, Status::HolidayUnpaid);
        d.holiday = false;
        d.weekly_off = true;
        assert_eq!(calculate(&d).status, Status::WeeklyOff);
        // UA-01: absent with no leave is pending for 3 days, then absent.
        let mut d = day(&sh, &s, vec![]);
        d.today = date() + Duration::days(2);
        assert_eq!(calculate(&d).status, Status::AbsentPending);
        d.today = date() + Duration::days(4);
        assert_eq!(calculate(&d).status, Status::Absent);
        s.present_by_default = true;
        assert_eq!(calculate(&day(&sh, &s, vec![])).status, Status::Present);
        // RD: 7h52 → 7h45 / 8h00; status from unrounded hours.
        let s = AtSettings { rounding: true, ..AtSettings::default() };
        let r = calculate(&day(&sh, &s, io((9, 8), Some((18, 0)))));
        assert_eq!((r.net_minutes, r.net_minutes_rounded), (hm(7, 52), hm(7, 45)));
    }

    #[test]
    fn detect_picks_nearest_start_within_window() {
        let sh = |h: u32, before: i32| ShiftTimes {
            start_time: NaiveTime::from_hms_opt(h, 0, 0).unwrap(),
            end_time: NaiveTime::from_hms_opt((h + 8) % 24, 0, 0).unwrap(),
            is_night: h + 8 >= 24,
            window_before_min: before,
            ..ShiftTimes::default()
        };
        let (mor, gen, eve) = (sh(6, 180), sh(9, 180), sh(14, 180));
        let all = [&mor, &gen, &eve];
        // 08:10 is inside MOR's and GEN's windows; GEN starts nearer (50 min vs 130).
        assert_eq!(detect(date(), &[at(8, 10), at(17, 0)], &all), Some(1));
        // The first punch decides, not a later one nearer another start.
        assert_eq!(detect(date(), &[at(13, 55), at(5, 50)], &all), Some(0));
        // Tie (07:30 is 90 minutes from both 06:00 and 09:00): the earlier shift.
        assert_eq!(detect(date(), &[at(7, 30)], &all), Some(0));
        // 12:40 is nearer EVE's 14:00 start, but EVE's window (1 h before) opens at 13:00: only GEN is eligible.
        let eve_tight = sh(14, 60);
        assert_eq!(detect(date(), &[at(12, 40)], &[&gen, &eve_tight]), Some(0));
        // No window holds any punch: none detected (AC-AT-03.2).
        let gen_only = ShiftTimes { window_after_min: 0, ..sh(9, 60) };
        assert_eq!(detect(date(), &[at(3, 0), at(23, 0)], &[&gen_only]), None);
        assert_eq!(detect(date(), &[], &all), None);
    }
}
