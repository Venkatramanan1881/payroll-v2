//! The attendance settings document held by a policy-set version (BRD v0.14 §9.4–9.7, DOMAIN_MODEL §2.13).
//! One flat JSON object; a missing key takes the BRD default as changed by the decisions (DEC-10).
//! Settings that belong to something other than an employee's day are kept where they live: shift times,
//! night date rule, window and half-day point on the shift (SH-02–07, SH-11); fences on the branch (PC-03);
//! device alerts on the device (PC-10); calendars on the branch (SH-10).

use crate::settings::{check, SettingErrors};
use chrono::NaiveTime;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// A closed list of choices, stored in snake_case. `names()` feeds the schema (PS5).
macro_rules! choice {
    ($name:ident { $($v:ident),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($v),+ }
        impl $name {
            pub fn names() -> Vec<Value> {
                vec![$(serde_json::to_value($name::$v).expect("choice serialises")),+]
            }
        }
    };
}

choice!(FenceRule { Block, Flag, Allow });
choice!(OffDayPunch { Allow, AllowAndFlag, Block });
choice!(BlockOrFlag { Block, Flag });
choice!(AccuracyRule { Accept, Flag, Block });
choice!(BothOrEither { Both, Either });
choice!(FenceScope { MasterOnly, MasterAndAllowed });
choice!(OnDutyFence { NotApplied, Applied });
choice!(WeeklyOffMode { Fixed, Alternating, FromRoster });
choice!(ShiftOverlap { NearestBoundary, EarlierShift });
choice!(ReadingMode { DirectionTagged, FirstInLastOut });
choice!(KeepPunch { First, Last });
choice!(BreakType { None, FixedAutoDeduct, Window, Flexible, FlexibleWithCap });
choice!(ExcessBreak { Deduct, EarlyMinutes, Permission, FlagOnly });
choice!(OutsideWindow { Deduct, Flag, Both });
choice!(LateFrom { ShiftStart, GraceEnd });
choice!(SlabAction { None, Mark, DeductMinutes, HalfDay, FullDay, HoldCheckIn });
choice!(PenaltySize { HalfDay, FullDay, FixedAmount });
choice!(MinutesRule { DeductExcess, HalfDayPer, FullDayPer });
choice!(CompensationWindow { SameDay, SameWeek });
choice!(ToPermission { Off, Suggest, Automatic });
choice!(CounterReset { CalendarMonth, PayrollPeriod, Quarter, Year });
choice!(FlexibleLate { Auto, NotApplicable, CoreStart, LatestCheckIn });
choice!(EarlyCounter { Separate, Combined });
choice!(UnapprovedExit { Mark, HalfDay, FlagHr });
choice!(AutoOnOff { Auto, On, Off });
choice!(EarlyArrivalFrom { ShiftStart, PunchTime, PunchTimeIfApproved });
choice!(MissedRule { Flag, AutoClose, PairsOnly, HalfDay, Absent });
choice!(CutoffRule { PairsOnly, HalfDay, Absent });
choice!(AutoCheckOut { Off, ShiftEnd, FixedTime });
choice!(StatusBasis { Auto, Shift, Hours, Hybrid });
choice!(Shortfall { HalfDay, DeductMinutes, MakeUpWeek });
choice!(BelowHalf { Absent, Proportional });
choice!(AfterWindow { LossOfPay, DebitLeave });
choice!(PenaltyOrder { Same, Own });
choice!(Direction { Nearest, Down, Up });
choice!(NoAction { Escalate, AutoApprove, AutoReject });
choice!(OverLimit { Block, HrOnly });

/// One range of late (or early) minutes and what it does (LA-05, EC-04). `to` None = no upper end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slab {
    pub from: u32,
    pub to: Option<u32>,
    pub action: SlabAction,
}

/// "2nd and 4th Saturday": ISO weekday (1 = Monday) and the weeks of the month (SH-09 alternating).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlternateOff {
    pub day: u8,
    pub weeks: Vec<u8>,
}

/// Request types of RG-02.
pub const RG_TYPES: [&str; 5] = ["missed_punch", "late_arrival", "early_check_out", "wrong_punch", "whole_day"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AtSettings {
    // PC punch collection
    pub punch_biometric: bool,                    // PC-01
    pub punch_web: bool,                          // PC-01
    pub punch_app: bool,                          // PC-01
    pub punch_import: bool,                       // PC-01
    pub punch_hr_entry: bool,                     // PC-01
    pub app_requires_location: bool,              // PC-02
    pub app_requires_selfie: bool,                // PC-02
    pub app_requires_inside_fence: bool,          // PC-02
    pub outside_fence: FenceRule,                 // PC-04
    pub web_ip_restricted: bool,                  // PC-05
    pub web_allowed_ips: Vec<String>,             // PC-05
    pub punch_on_off_day: OffDayPunch,            // PC-06
    pub late_sync_days: u32,                      // PC-07
    pub device_clock_tolerance_minutes: u32,      // PC-08
    pub web_requires_location: bool,              // PC-12
    pub web_requires_inside_fence: bool,          // PC-12
    pub web_location_unavailable: BlockOrFlag,    // PC-13
    pub location_accuracy_metres: u32,            // PC-14
    pub location_inaccurate: AccuracyRule,        // PC-14
    pub web_location_and_ip: BothOrEither,        // PC-15
    pub fences_apply: FenceScope,                 // PC-16
    pub fence_on_on_duty: OnDutyFence,            // PC-17
    // SH shift options
    pub shift_overlap: ShiftOverlap,              // SH-07 / ATT-02: a punch inside two shift windows
    pub auto_detect_shift: bool,                  // SH-08
    pub weekly_off_mode: WeeklyOffMode,           // SH-09
    pub weekly_off_days: Vec<u8>,                 // SH-09 fixed (ISO weekdays)
    pub weekly_off_alternating: Vec<AlternateOff>, // SH-09 alternating
    pub shift_requests_enabled: bool,             // SH-12
    // PR punch reading
    pub reading_mode: ReadingMode,                // PR-01
    pub repeat_punch_seconds: u32,                // PR-02
    pub repeat_punch_keep: KeepPunch,             // PR-03
    pub min_gap_minutes: u32,                     // PR-04
    pub max_session_hours: u32,                   // PR-05
    pub max_punches_per_day: u32,                 // PR-06
    // BK breaks
    pub break_type: BreakType,                    // BK-01
    pub break_paid: bool,                         // BK-02
    pub break_minutes: u32,                       // BK-03
    pub break_window_start: Option<NaiveTime>,    // BK-04
    pub break_window_end: Option<NaiveTime>,      // BK-04
    pub break_cap_minutes: Option<u32>,           // BK-05
    pub break_cap_count: Option<u32>,             // BK-05
    pub excess_break: ExcessBreak,                // BK-06
    pub break_outside_window: OutsideWindow,      // BK-07
    pub auto_deduct_unpunched: bool,              // BK-08
    pub auto_deduct_minutes: Option<u32>,         // BK-08 X
    pub auto_deduct_after_hours: Option<u32>,     // BK-08 Y
    pub named_breaks: bool,                       // BK-09
    // LA late arrival
    pub late_tracking: bool,                      // LA-01
    pub late_measured_from: LateFrom,             // LA-02
    pub late_grace_minutes: u32,                  // LA-03
    pub late_grace_uses: Option<u32>,             // LA-04 (None = unlimited)
    pub late_slabs: Vec<Slab>,                    // LA-05
    pub late_count_penalty: bool,                 // LA-06
    pub late_free_marks: u32,                     // LA-07
    pub late_penalty_every: u32,                  // LA-08
    pub late_penalty_size: PenaltySize,           // LA-09
    pub late_minutes_penalty: bool,               // LA-10
    pub late_minutes_threshold: u32,              // LA-11 X
    pub late_minutes_rule: MinutesRule,           // LA-11
    pub late_minutes_per: Option<u32>,            // LA-11 Y
    pub late_compensation: bool,                  // LA-13
    pub late_compensation_max_minutes: u32,       // LA-13
    pub late_compensation_window: CompensationWindow, // LA-14
    pub late_to_permission: ToPermission,         // LA-15
    pub late_regularisation: bool,                // LA-16
    pub late_reasons: Vec<String>,                // LA-16
    pub late_bulk_waiver: bool,                   // LA-17
    pub late_counter_reset: CounterReset,         // LA-18
    pub late_escalation: bool,                    // LA-19
    pub late_escalate_manager_after: u32,         // LA-19
    pub late_escalate_hr_after: u32,              // LA-19
    pub late_grace_on_adjusted_start: bool,       // LA-20
    pub late_flexible: FlexibleLate,              // LA-21
    pub late_slab_counts_as_mark: bool,           // LA-23
    // EC early check-out
    pub early_tracking: bool,                     // EC-01
    pub early_grace_minutes: u32,                 // EC-02
    pub early_grace_uses: Option<u32>,            // EC-03
    pub early_slabs: Vec<Slab>,                   // EC-04
    pub early_count_penalty: bool,                // EC-05
    pub early_free_marks: u32,                    // EC-06
    pub early_penalty_every: u32,                 // EC-07
    pub early_penalty_size: PenaltySize,          // EC-08
    pub early_minutes_penalty: bool,              // EC-09
    pub early_minutes_threshold: u32,             // EC-09
    pub early_minutes_rule: MinutesRule,          // EC-09
    pub early_minutes_per: Option<u32>,           // EC-09
    pub early_counter: EarlyCounter,              // EC-10
    pub early_compensation: bool,                 // EC-12
    pub early_to_permission: ToPermission,        // EC-13
    pub early_prior_approval: bool,               // EC-14
    pub early_unapproved_exit: UnapprovedExit,    // EC-14
    pub early_hours_met_override: AutoOnOff,      // EC-15 (auto = on for flexible, off for fixed)
    pub early_regularisation: bool,               // EC-16
    pub early_bulk_waiver: bool,                  // EC-16
    pub early_counter_reset: CounterReset,        // EC-17
    pub early_escalation: bool,                   // EC-17
    pub early_escalate_manager_after: u32,        // EC-17
    pub early_escalate_hr_after: u32,             // EC-17
    pub early_grace_on_adjusted_end: bool,        // EC-17
    pub early_flexible: FlexibleLate,             // EC-17
    // EA early arrival
    pub early_arrival_from: EarlyArrivalFrom,     // EA-01
    pub early_arrival_max_minutes: u32,           // EA-02
    // MP missed punches
    pub odd_punches: MissedRule,                  // MP-01
    pub single_punch: MissedRule,                 // MP-02
    pub unresolved_at_cutoff: CutoffRule,         // MP-03
    pub missed_punch_reminders: bool,             // MP-04
    pub reminder_after_minutes: u32,              // MP-04
    pub auto_check_out: AutoCheckOut,             // MP-05
    pub auto_check_out_time: Option<NaiveTime>,   // MP-05
    // DS day status
    pub status_basis: StatusBasis,                // DS-01 (auto = shift-based for fixed and split, hours-based for flexible)
    pub full_day_min_percent: Option<u32>,        // DS-02 (None = 100 for hours-based, 75 for hybrid)
    pub half_day_min_percent: u32,                // DS-03
    pub shortfall_tolerance_minutes: u32,         // DS-04
    pub shortfall_action: Shortfall,              // DS-05
    pub below_half_day: BelowHalf,                // DS-07
    pub present_by_default: bool,                 // DS-09
    // UA unapproved absence
    pub absent_apply_days: u32,                   // UA-01
    pub after_apply_window: AfterWindow,          // UA-02
    pub debit_order: Vec<String>,                 // UA-03 (leave type codes, ending in loss_of_pay)
    pub penalty_order: PenaltyOrder,              // UA-04
    pub penalty_own_order: Vec<String>,           // UA-04
    pub sick_auto_debit: bool,                    // UA-05
    // RD rounding
    pub rounding: bool,                           // RD-01
    pub rounding_unit_minutes: u32,               // RD-02
    pub rounding_direction: Direction,            // RD-03
    // RG regularisation
    pub regularisation: bool,                     // RG-01
    pub rg_types: Vec<String>,                    // RG-02 (turned on)
    pub rg_limit_overall: u32,                    // RG-03
    pub rg_limit_per_type: BTreeMap<String, u32>, // RG-03
    pub rg_window_days: u32,                      // RG-04
    pub rg_levels: u8,                            // RG-05
    pub rg_auto_approve_types: Vec<String>,       // RG-06
    pub rg_no_action: NoAction,                   // RG-07
    pub rg_no_action_days: u32,                   // RG-07
    pub rg_over_limit: OverLimit,                 // RG-08
    pub rg_attachment_required_types: Vec<String>, // RG-09
    pub hr_direct_correction: bool,               // RG-10
    pub hr_bulk_regularisation: bool,             // RG-11
    // HD holidays
    pub optional_holidays_per_year: u32,          // HD-01
    pub holiday_needs_presence_around: bool,      // HD-03
    pub daily_wage_paid_holidays: bool,           // HD-04
    pub daily_wage_paid_weekly_offs: bool,        // HD-05
}

impl Default for AtSettings {
    fn default() -> Self {
        let slab = |from, to, action| Slab { from, to, action };
        Self {
            punch_biometric: true,
            punch_web: true,
            punch_app: true,
            punch_import: true,
            punch_hr_entry: true,
            app_requires_location: true,
            app_requires_selfie: false,
            app_requires_inside_fence: true,
            outside_fence: FenceRule::Flag,
            web_ip_restricted: false,
            web_allowed_ips: Vec::new(),
            punch_on_off_day: OffDayPunch::AllowAndFlag,
            late_sync_days: 3,
            device_clock_tolerance_minutes: 5,
            web_requires_location: true,
            web_requires_inside_fence: true,
            web_location_unavailable: BlockOrFlag::Block,
            location_accuracy_metres: 100,
            location_inaccurate: AccuracyRule::Flag,
            web_location_and_ip: BothOrEither::Either,
            fences_apply: FenceScope::MasterAndAllowed,
            fence_on_on_duty: OnDutyFence::NotApplied,
            shift_overlap: ShiftOverlap::NearestBoundary,
            auto_detect_shift: false,
            weekly_off_mode: WeeklyOffMode::Fixed,
            weekly_off_days: vec![7],
            weekly_off_alternating: Vec::new(),
            shift_requests_enabled: false,
            reading_mode: ReadingMode::DirectionTagged,
            repeat_punch_seconds: 60,
            repeat_punch_keep: KeepPunch::First,
            min_gap_minutes: 5,
            max_session_hours: 16,
            max_punches_per_day: 12,
            break_type: BreakType::FixedAutoDeduct,
            break_paid: false,
            break_minutes: 60,
            break_window_start: None,
            break_window_end: None,
            break_cap_minutes: None,
            break_cap_count: None,
            excess_break: ExcessBreak::Deduct,
            break_outside_window: OutsideWindow::Flag,
            auto_deduct_unpunched: false,
            auto_deduct_minutes: None,
            auto_deduct_after_hours: None,
            named_breaks: false,
            late_tracking: true,
            late_measured_from: LateFrom::ShiftStart,
            late_grace_minutes: 10,
            late_grace_uses: None,
            late_slabs: vec![slab(11, Some(120), SlabAction::Mark), slab(121, None, SlabAction::HalfDay)],
            late_count_penalty: false,
            late_free_marks: 0,
            late_penalty_every: 3,
            late_penalty_size: PenaltySize::HalfDay,
            late_minutes_penalty: false,
            late_minutes_threshold: 60,
            late_minutes_rule: MinutesRule::DeductExcess,
            late_minutes_per: None,
            late_compensation: false,
            late_compensation_max_minutes: 60,
            late_compensation_window: CompensationWindow::SameDay,
            late_to_permission: ToPermission::Suggest,
            late_regularisation: true,
            late_reasons: Vec::new(),
            late_bulk_waiver: true,
            late_counter_reset: CounterReset::PayrollPeriod,
            late_escalation: true,
            late_escalate_manager_after: 3,
            late_escalate_hr_after: 5,
            late_grace_on_adjusted_start: true,
            late_flexible: FlexibleLate::Auto,
            late_slab_counts_as_mark: false,
            early_tracking: true,
            early_grace_minutes: 5,
            early_grace_uses: None,
            early_slabs: vec![slab(6, Some(120), SlabAction::Mark), slab(121, None, SlabAction::HalfDay)],
            early_count_penalty: false,
            early_free_marks: 0,
            early_penalty_every: 3,
            early_penalty_size: PenaltySize::HalfDay,
            early_minutes_penalty: false,
            early_minutes_threshold: 60,
            early_minutes_rule: MinutesRule::DeductExcess,
            early_minutes_per: None,
            early_counter: EarlyCounter::Separate,
            early_compensation: false,
            early_to_permission: ToPermission::Suggest,
            early_prior_approval: false,
            early_unapproved_exit: UnapprovedExit::Mark,
            early_hours_met_override: AutoOnOff::Auto,
            early_regularisation: true,
            early_bulk_waiver: true,
            early_counter_reset: CounterReset::PayrollPeriod,
            early_escalation: true,
            early_escalate_manager_after: 3,
            early_escalate_hr_after: 5,
            early_grace_on_adjusted_end: true,
            early_flexible: FlexibleLate::Auto,
            early_arrival_from: EarlyArrivalFrom::ShiftStart,
            early_arrival_max_minutes: 60,
            odd_punches: MissedRule::Flag,
            single_punch: MissedRule::Flag,
            unresolved_at_cutoff: CutoffRule::PairsOnly,
            missed_punch_reminders: true,
            reminder_after_minutes: 30,
            auto_check_out: AutoCheckOut::Off,
            auto_check_out_time: None,
            status_basis: StatusBasis::Auto,
            full_day_min_percent: None,
            half_day_min_percent: 50,
            shortfall_tolerance_minutes: 15,
            shortfall_action: Shortfall::HalfDay,
            below_half_day: BelowHalf::Absent,
            present_by_default: false,
            absent_apply_days: 3,
            after_apply_window: AfterWindow::DebitLeave,
            debit_order: vec!["casual".into(), "loss_of_pay".into()],
            penalty_order: PenaltyOrder::Same,
            penalty_own_order: Vec::new(),
            sick_auto_debit: false,
            rounding: false,
            rounding_unit_minutes: 15,
            rounding_direction: Direction::Nearest,
            regularisation: true,
            rg_types: RG_TYPES.iter().map(|t| t.to_string()).collect(),
            rg_limit_overall: 3,
            rg_limit_per_type: BTreeMap::new(),
            rg_window_days: 7,
            rg_levels: 1,
            rg_auto_approve_types: Vec::new(),
            rg_no_action: NoAction::Escalate,
            rg_no_action_days: 2,
            rg_over_limit: OverLimit::HrOnly,
            rg_attachment_required_types: Vec::new(),
            hr_direct_correction: true,
            hr_bulk_regularisation: true,
            optional_holidays_per_year: 0,
            holiday_needs_presence_around: false,
            daily_wage_paid_holidays: false,
            daily_wage_paid_weekly_offs: false,
        }
    }
}

impl AtSettings {
    /// Reads a stored version leniently: unknown keys are ignored, a bad stored value falls back to defaults.
    pub fn from_json(v: Value) -> Self {
        serde_json::from_value(v).unwrap_or_default()
    }

    /// DS-02 with its decision-dependent default.
    pub fn full_day_min(&self) -> u32 {
        self.full_day_min_percent.unwrap_or(if self.status_basis == StatusBasis::Hybrid { 75 } else { 100 })
    }
}

/// Slabs start right after grace, run without gaps or overlaps, at most 5, only the last open (LA-05, BR-AT-14).
fn slab_rules(key: &str, slabs: &[Slab], grace: u32, early: bool, e: &mut SettingErrors) {
    let mut bad = |m: String| e.push((key.into(), m));
    if slabs.len() > 5 {
        bad("At most 5 slabs".into());
    }
    let mut next = grace + 1;
    for (i, s) in slabs.iter().enumerate() {
        if s.from != next {
            bad(format!("Slab {} must start at {next} minutes (grace + 1, or right after the slab before)", i + 1));
            return;
        }
        match s.to {
            Some(t) if t < s.from => return bad(format!("Slab {} ends before it starts", i + 1)),
            Some(t) => next = t + 1,
            None if i + 1 < slabs.len() => return bad(format!("Only the last slab can be open-ended (slab {})", i + 1)),
            None => {}
        }
        if early && s.action == SlabAction::HoldCheckIn {
            bad("Hold check-in is not an early check-out action".into());
        }
    }
}

/// Cross-checks of AC-AT-21.5 and the ranges the BRD gives. Each problem names its key.
fn rules(s: &AtSettings) -> SettingErrors {
    let mut e = SettingErrors::new();
    let mut need = |ok: bool, k: &str, m: &str| {
        if !ok {
            e.push((k.into(), m.into()));
        }
    };
    need(s.late_grace_minutes <= 120, "late_grace_minutes", "0 to 120 minutes");
    need(s.early_grace_minutes <= 120, "early_grace_minutes", "0 to 120 minutes");
    need(s.late_penalty_every >= 1, "late_penalty_every", "At least 1");
    need(s.early_penalty_every >= 1, "early_penalty_every", "At least 1");
    need(s.late_minutes_rule == MinutesRule::DeductExcess || s.late_minutes_per.is_some_and(|n| n > 0), "late_minutes_per", "Give the minutes per deduction");
    need(s.early_minutes_rule == MinutesRule::DeductExcess || s.early_minutes_per.is_some_and(|n| n > 0), "early_minutes_per", "Give the minutes per deduction");
    need(s.weekly_off_days.iter().all(|d| (1..=7).contains(d)), "weekly_off_days", "Days are 1 (Monday) to 7 (Sunday)");
    need(
        s.weekly_off_alternating.iter().all(|a| (1..=7).contains(&a.day) && !a.weeks.is_empty() && a.weeks.iter().all(|w| (1..=5).contains(w))),
        "weekly_off_alternating",
        "Each entry needs a day 1 to 7 and weeks of the month 1 to 5",
    );
    need(s.weekly_off_mode != WeeklyOffMode::Alternating || !s.weekly_off_alternating.is_empty(), "weekly_off_alternating", "Choose the alternating days");
    need(s.max_punches_per_day >= 2, "max_punches_per_day", "At least 2");
    need((1..=24).contains(&s.max_session_hours), "max_session_hours", "1 to 24 hours");
    let window = matches!(s.break_type, BreakType::Window);
    match (s.break_window_start, s.break_window_end) {
        (Some(a), Some(b)) => need(a < b, "break_window_end", "The break window must end after it starts"),
        (None, None) => need(!window, "break_window_start", "A window break needs the window"),
        _ => need(false, "break_window_end", "Give both window start and end"),
    }
    need(s.break_type != BreakType::FlexibleWithCap || s.break_cap_minutes.is_some() || s.break_cap_count.is_some(), "break_cap_minutes", "Give a cap");
    need(!s.auto_deduct_unpunched || (s.auto_deduct_minutes.is_some() && s.auto_deduct_after_hours.is_some()), "auto_deduct_minutes", "Give the minutes and after how many hours");
    need(s.auto_check_out != AutoCheckOut::FixedTime || s.auto_check_out_time.is_some(), "auto_check_out_time", "Give the time");
    need(s.full_day_min_percent.is_none_or(|p| (1..=100).contains(&p)), "full_day_min_percent", "1 to 100");
    need(s.half_day_min_percent < s.full_day_min(), "half_day_min_percent", "The half-day minimum must be below the full-day minimum");
    need([5, 6, 10, 15, 30].contains(&s.rounding_unit_minutes), "rounding_unit_minutes", "5, 6, 10, 15 or 30");
    need((1..=2).contains(&s.rg_levels), "rg_levels", "1 (manager) or 2 (manager then HR)");
    for (k, list) in [("rg_types", &s.rg_types), ("rg_auto_approve_types", &s.rg_auto_approve_types), ("rg_attachment_required_types", &s.rg_attachment_required_types)] {
        need(list.iter().all(|t| RG_TYPES.contains(&t.as_str())), k, "Types are missing_punch, late_arrival, early_check_out, wrong_punch, whole_day");
    }
    need(s.rg_limit_per_type.keys().all(|t| RG_TYPES.contains(&t.as_str())), "rg_limit_per_type", "Unknown request type");
    need(s.debit_order.last().is_some_and(|t| t == "loss_of_pay"), "debit_order", "The order ends in loss_of_pay");
    need(s.penalty_order == PenaltyOrder::Same || s.penalty_own_order.last().is_some_and(|t| t == "loss_of_pay"), "penalty_own_order", "The order ends in loss_of_pay");
    need(s.late_escalate_manager_after <= s.late_escalate_hr_after, "late_escalate_hr_after", "HR is told after the manager");
    need(s.early_escalate_manager_after <= s.early_escalate_hr_after, "early_escalate_hr_after", "HR is told after the manager");
    need(s.web_allowed_ips.iter().all(|ip| ip.parse::<std::net::IpAddr>().is_ok() || ip.contains('/')), "web_allowed_ips", "Use IP addresses or CIDR ranges");
    need(!s.web_ip_restricted || !s.web_allowed_ips.is_empty(), "web_allowed_ips", "List the allowed addresses");
    slab_rules("late_slabs", &s.late_slabs, s.late_grace_minutes, false, &mut e);
    slab_rules("early_slabs", &s.early_slabs, s.early_grace_minutes, true, &mut e);
    e
}

/// Strict check of a version's values (PS4): unknown keys, bad values and broken rules, each named.
/// Returns the complete document (defaults filled in).
pub fn check_values(values: &Map<String, Value>) -> Result<Value, SettingErrors> {
    check::<AtSettings>(values, rules)
}

/// The stored values with every default filled in (PS4 `with_defaults`).
pub fn filled(stored: Value) -> Value {
    serde_json::to_value(AtSettings::from_json(stored)).expect("settings serialise")
}

/// Settings removed by decisions and never offered (AC-AT-21.1).
pub const REMOVED: [(&str, &str); 6] = [
    ("DS-06", "DEC-24: no quarter-day statuses"),
    ("DS-10", "DEC-25: replaced by the UA group"),
    ("HD-02", "DEC-38: a holiday on a weekly off stays as it is"),
    ("PC-09", "DEC-14: work from home is version 3"),
    ("PC-11", "Version 1: HR Manager only"),
    ("LA-22", "Exempt groups get their own policy set; dated exemptions are waivers"),
];

type Choices = Option<fn() -> Vec<Value>>;

/// (key, BRD code, label, choices) for each setting, in form order (PS5).
const META: &[(&str, &str, &str, Choices)] = &[
    ("punch_biometric", "PC-01", "Punches from biometric devices", None),
    ("punch_web", "PC-01", "Punches from the web", None),
    ("punch_app", "PC-01", "Punches from the mobile app", None),
    ("punch_import", "PC-01", "Punches from file or API import", None),
    ("punch_hr_entry", "PC-01", "Punches entered by HR", None),
    ("app_requires_location", "PC-02", "App punch needs location", None),
    ("app_requires_selfie", "PC-02", "App punch needs a selfie", None),
    ("app_requires_inside_fence", "PC-02", "App punch must be inside the geo-fence", None),
    ("outside_fence", "PC-04", "Punch outside the geo-fence", Some(FenceRule::names)),
    ("web_ip_restricted", "PC-05", "Web punch only from listed IP addresses", None),
    ("web_allowed_ips", "PC-05", "Allowed IP addresses", None),
    ("punch_on_off_day", "PC-06", "Punch on a weekly off, holiday or with no shift", Some(OffDayPunch::names)),
    ("late_sync_days", "PC-07", "Flag punches synced more than N days late", None),
    ("device_clock_tolerance_minutes", "PC-08", "Device clock mismatch tolerance (minutes)", None),
    ("web_requires_location", "PC-12", "Web punch needs location", None),
    ("web_requires_inside_fence", "PC-12", "Web punch must be inside the geo-fence", None),
    ("web_location_unavailable", "PC-13", "Web location not available", Some(BlockOrFlag::names)),
    ("location_accuracy_metres", "PC-14", "Location accuracy limit (metres)", None),
    ("location_inaccurate", "PC-14", "Less accurate reading", Some(AccuracyRule::names)),
    ("web_location_and_ip", "PC-15", "Web location and IP restriction both on", Some(BothOrEither::names)),
    ("fences_apply", "PC-16", "Fences that apply", Some(FenceScope::names)),
    ("fence_on_on_duty", "PC-17", "Geo-fence on approved on-duty days", Some(OnDutyFence::names)),
    ("shift_overlap", "SH-07", "Punch inside two shift windows", Some(ShiftOverlap::names)),
    ("auto_detect_shift", "SH-08", "Detect the shift from the first punch", None),
    ("weekly_off_mode", "SH-09", "Weekly offs", Some(WeeklyOffMode::names)),
    ("weekly_off_days", "SH-09", "Fixed weekly off days (1 = Monday)", None),
    ("weekly_off_alternating", "SH-09", "Alternating weekly offs (day and weeks of the month)", None),
    ("shift_requests_enabled", "SH-12", "Shift change and swap requests", None),
    ("reading_mode", "PR-01", "Punch reading mode", Some(ReadingMode::names)),
    ("repeat_punch_seconds", "PR-02", "Repeat-punch window (seconds)", None),
    ("repeat_punch_keep", "PR-03", "Repeat punch kept", Some(KeepPunch::names)),
    ("min_gap_minutes", "PR-04", "Minimum gap (minutes)", None),
    ("max_session_hours", "PR-05", "Maximum session length (hours)", None),
    ("max_punches_per_day", "PR-06", "Maximum punches per day before flagging", None),
    ("break_type", "BK-01", "Break type", Some(BreakType::names)),
    ("break_paid", "BK-02", "Break is paid", None),
    ("break_minutes", "BK-03", "Break duration (minutes)", None),
    ("break_window_start", "BK-04", "Break window start", None),
    ("break_window_end", "BK-04", "Break window end", None),
    ("break_cap_minutes", "BK-05", "Cap on break minutes per day", None),
    ("break_cap_count", "BK-05", "Cap on breaks per day", None),
    ("excess_break", "BK-06", "Excess break time", Some(ExcessBreak::names)),
    ("break_outside_window", "BK-07", "Break outside the window", Some(OutsideWindow::names)),
    ("auto_deduct_unpunched", "BK-08", "Auto-deduct when no break is punched", None),
    ("auto_deduct_minutes", "BK-08", "Minutes deducted", None),
    ("auto_deduct_after_hours", "BK-08", "After hours worked", None),
    ("named_breaks", "BK-09", "Several named breaks", None),
    ("late_tracking", "LA-01", "Late tracking", None),
    ("late_measured_from", "LA-02", "Late minutes measured from", Some(LateFrom::names)),
    ("late_grace_minutes", "LA-03", "Grace period (minutes)", None),
    ("late_grace_uses", "LA-04", "Grace uses per period (empty = unlimited)", None),
    ("late_slabs", "LA-05", "Late slabs", Some(SlabAction::names)),
    ("late_count_penalty", "LA-06", "Count-based penalty", None),
    ("late_free_marks", "LA-07", "Penalty-free late marks per period", None),
    ("late_penalty_every", "LA-08", "Penalty on every Nth late mark", None),
    ("late_penalty_size", "LA-09", "Penalty size", Some(PenaltySize::names)),
    ("late_minutes_penalty", "LA-10", "Minutes-based penalty", None),
    ("late_minutes_threshold", "LA-11", "Late minutes in the period before the penalty", None),
    ("late_minutes_rule", "LA-11", "Minutes rule", Some(MinutesRule::names)),
    ("late_minutes_per", "LA-11", "Minutes per half or full day", None),
    ("late_compensation", "LA-13", "Compensation by working extra", None),
    ("late_compensation_max_minutes", "LA-13", "Most minutes that can be made up", None),
    ("late_compensation_window", "LA-14", "Compensation window", Some(CompensationWindow::names)),
    ("late_to_permission", "LA-15", "Convert late arrival to a permission", Some(ToPermission::names)),
    ("late_regularisation", "LA-16", "Late regularisation", None),
    ("late_reasons", "LA-16", "Late reasons", None),
    ("late_bulk_waiver", "LA-17", "Bulk waiver by HR", None),
    ("late_counter_reset", "LA-18", "Counters reset every", Some(CounterReset::names)),
    ("late_escalation", "LA-19", "Escalation alerts", None),
    ("late_escalate_manager_after", "LA-19", "Tell the manager after N late marks", None),
    ("late_escalate_hr_after", "LA-19", "Tell HR after N late marks", None),
    ("late_grace_on_adjusted_start", "LA-20", "Grace applies to an adjusted start time", None),
    ("late_flexible", "LA-21", "Late on flexible shifts", Some(FlexibleLate::names)),
    ("late_slab_counts_as_mark", "LA-23", "A day cut by a slab also counts as a late mark", None),
    ("early_tracking", "EC-01", "Early check-out tracking", None),
    ("early_grace_minutes", "EC-02", "Grace before shift end (minutes)", None),
    ("early_grace_uses", "EC-03", "Grace uses per period (empty = unlimited)", None),
    ("early_slabs", "EC-04", "Early slabs", Some(SlabAction::names)),
    ("early_count_penalty", "EC-05", "Count-based penalty", None),
    ("early_free_marks", "EC-06", "Penalty-free early marks per period", None),
    ("early_penalty_every", "EC-07", "Penalty on every Nth early mark", None),
    ("early_penalty_size", "EC-08", "Penalty size", Some(PenaltySize::names)),
    ("early_minutes_penalty", "EC-09", "Minutes-based penalty", None),
    ("early_minutes_threshold", "EC-09", "Early minutes in the period before the penalty", None),
    ("early_minutes_rule", "EC-09", "Minutes rule", Some(MinutesRule::names)),
    ("early_minutes_per", "EC-09", "Minutes per half or full day", None),
    ("early_counter", "EC-10", "Counter", Some(EarlyCounter::names)),
    ("early_compensation", "EC-12", "Compensation by approved early arrival", None),
    ("early_to_permission", "EC-13", "Convert early check-out to a permission", Some(ToPermission::names)),
    ("early_prior_approval", "EC-14", "Prior approval needed to leave early", None),
    ("early_unapproved_exit", "EC-14", "Unapproved early exit", Some(UnapprovedExit::names)),
    ("early_hours_met_override", "EC-15", "No early mark when required hours are done", Some(AutoOnOff::names)),
    ("early_regularisation", "EC-16", "Early check-out regularisation", None),
    ("early_bulk_waiver", "EC-16", "Bulk waiver by HR", None),
    ("early_counter_reset", "EC-17", "Counters reset every", Some(CounterReset::names)),
    ("early_escalation", "EC-17", "Escalation alerts", None),
    ("early_escalate_manager_after", "EC-17", "Tell the manager after N early marks", None),
    ("early_escalate_hr_after", "EC-17", "Tell HR after N early marks", None),
    ("early_grace_on_adjusted_end", "EC-17", "Grace applies to an adjusted end time", None),
    ("early_flexible", "EC-17", "Early check-out on flexible shifts", Some(FlexibleLate::names)),
    ("early_arrival_from", "EA-01", "Early check-in counts from", Some(EarlyArrivalFrom::names)),
    ("early_arrival_max_minutes", "EA-02", "Most early minutes that count", None),
    ("odd_punches", "MP-01", "Odd number of punches", Some(MissedRule::names)),
    ("single_punch", "MP-02", "Single punch", Some(MissedRule::names)),
    ("unresolved_at_cutoff", "MP-03", "Still unresolved at the cut-off", Some(CutoffRule::names)),
    ("missed_punch_reminders", "MP-04", "Reminders to the employee", None),
    ("reminder_after_minutes", "MP-04", "Remind after shift end plus (minutes)", None),
    ("auto_check_out", "MP-05", "Auto check-out", Some(AutoCheckOut::names)),
    ("auto_check_out_time", "MP-05", "Auto check-out time", None),
    ("status_basis", "DS-01", "Status basis", Some(StatusBasis::names)),
    ("full_day_min_percent", "DS-02", "Minimum net hours for a full day (% of required)", None),
    ("half_day_min_percent", "DS-03", "Minimum net hours for a half day (% of required)", None),
    ("shortfall_tolerance_minutes", "DS-04", "Shortfall tolerance (minutes)", None),
    ("shortfall_action", "DS-05", "Shortfall below the full-day minimum", Some(Shortfall::names)),
    ("below_half_day", "DS-07", "Below the half-day minimum", Some(BelowHalf::names)),
    ("present_by_default", "DS-09", "Present by default (no punches needed)", None),
    ("absent_apply_days", "UA-01", "Days to apply leave for an absent day", None),
    ("after_apply_window", "UA-02", "After that", Some(AfterWindow::names)),
    ("debit_order", "UA-03", "Debit order (leave types, ending in loss_of_pay)", None),
    ("penalty_order", "UA-04", "Penalties use", Some(PenaltyOrder::names)),
    ("penalty_own_order", "UA-04", "Penalty debit order", None),
    ("sick_auto_debit", "UA-05", "Sick leave may be debited automatically", None),
    ("rounding", "RD-01", "Rounding of net hours", None),
    ("rounding_unit_minutes", "RD-02", "Rounding unit (minutes)", None),
    ("rounding_direction", "RD-03", "Rounding direction", Some(Direction::names)),
    ("regularisation", "RG-01", "Regularisation", None),
    ("rg_types", "RG-02", "Request types turned on", None),
    ("rg_limit_overall", "RG-03", "Limit per period", None),
    ("rg_limit_per_type", "RG-03", "Limit per period by type", None),
    ("rg_window_days", "RG-04", "Request within N days", None),
    ("rg_levels", "RG-05", "Approval levels", None),
    ("rg_auto_approve_types", "RG-06", "Auto-approved types", None),
    ("rg_no_action", "RG-07", "Approver doesn't act", Some(NoAction::names)),
    ("rg_no_action_days", "RG-07", "After N days", None),
    ("rg_over_limit", "RG-08", "Request over the limit", Some(OverLimit::names)),
    ("rg_attachment_required_types", "RG-09", "Types that need an attachment", None),
    ("hr_direct_correction", "RG-10", "Direct correction by HR", None),
    ("hr_bulk_regularisation", "RG-11", "Bulk regularisation by HR", None),
    ("optional_holidays_per_year", "HD-01", "Optional holidays per year", None),
    ("holiday_needs_presence_around", "HD-03", "Paid holiday needs presence the working day before and after", None),
    ("daily_wage_paid_holidays", "HD-04", "Daily-wage and hourly workers paid for holidays", None),
    ("daily_wage_paid_weekly_offs", "HD-05", "Daily-wage and hourly workers paid for weekly offs", None),
];

/// PS5: the settings document as a JSON Schema, with BRD code, label, default and choices per key.
pub fn schema() -> Value {
    let defaults = serde_json::to_value(AtSettings::default()).expect("settings serialise");
    let mut props = Map::new();
    for (key, code, label, choices) in META {
        let d = &defaults[*key];
        let kind = match d {
            Value::Bool(_) => "boolean",
            Value::Number(_) => "integer",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
            Value::Null => "null",
        };
        let mut p = json!({ "x-code": code, "title": label, "default": d, "type": kind });
        if let Some(f) = choices {
            p[if kind == "string" { "enum" } else { "x-actions" }] = Value::Array(f());
        }
        props.insert(key.to_string(), p);
    }
    let removed: Vec<Value> = REMOVED.iter().map(|(c, why)| json!({ "code": c, "reason": why })).collect();
    json!({ "type": "object", "additionalProperties": false, "properties": props, "x-removed": removed })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn defaults_pass_their_own_rules_and_every_key_has_a_label() {
        assert!(rules(&AtSettings::default()).is_empty(), "{:?}", rules(&AtSettings::default()));
        let defaults = serde_json::to_value(AtSettings::default()).unwrap();
        let keys: Vec<&String> = defaults.as_object().unwrap().keys().collect();
        let mut meta: Vec<&str> = META.iter().map(|x| x.0).collect();
        meta.sort();
        assert_eq!(keys, meta);
        assert_eq!(check_values(&Map::new()).unwrap(), defaults);
    }

    #[test]
    fn slabs_start_after_grace_without_gaps() {
        let bad = check_values(&m(json!({ "late_grace_minutes": 15 }))).unwrap_err();
        assert_eq!(bad[0].0, "late_slabs", "{bad:?}");
        let ok = json!({ "late_grace_minutes": 15, "late_slabs": [{ "from": 16, "to": 60, "action": "mark" }, { "from": 61, "to": null, "action": "full_day" }] });
        assert!(check_values(&m(ok)).is_ok());
        let gap = json!({ "late_slabs": [{ "from": 11, "to": 60, "action": "mark" }, { "from": 70, "to": null, "action": "half_day" }] });
        assert_eq!(check_values(&m(gap)).unwrap_err()[0].0, "late_slabs");
        let hold = json!({ "early_slabs": [{ "from": 6, "to": null, "action": "hold_check_in" }] });
        assert_eq!(check_values(&m(hold)).unwrap_err()[0].0, "early_slabs");
    }

    #[test]
    fn cross_checks_name_their_keys() {
        let bad = check_values(&m(json!({ "half_day_min_percent": 80, "status_basis": "hybrid", "break_type": "window",
                                           "rounding_unit_minutes": 7, "colour": "red", "late_grace_minutes": "ten" }))).unwrap_err();
        let keys: Vec<&str> = bad.iter().map(|(k, _)| k.as_str()).collect();
        for k in ["half_day_min_percent", "break_window_start", "rounding_unit_minutes", "colour", "late_grace_minutes"] {
            assert!(keys.contains(&k), "{k} missing from {keys:?}");
        }
        let window = json!({ "break_type": "window", "break_window_start": "13:00:00", "break_window_end": "12:00:00" });
        assert_eq!(check_values(&m(window)).unwrap_err()[0].0, "break_window_end");
    }

    #[test]
    fn schema_lists_choices_and_removed_settings() {
        let s = schema();
        assert_eq!(s["properties"]["outside_fence"]["enum"], json!(["block", "flag", "allow"]));
        assert_eq!(s["properties"]["late_grace_minutes"]["default"], 10);
        assert_eq!(s["x-removed"].as_array().unwrap().len(), REMOVED.len());
    }
}
