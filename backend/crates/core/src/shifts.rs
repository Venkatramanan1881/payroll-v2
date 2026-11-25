//! Shift definitions (SH-01–07, SH-11, DEC-22, DEC-23, ATT-05) and roster rotation (AC-AT-02.5).
//! The database repeats the simple checks; the cross-field ones (core inside band, break inside shift) live here.

use chrono::{Datelike, Days, NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShiftKind {
    Fixed,
    Flexible,
    FlexibleCore,
    Split,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NightDateRule {
    StartDate,
    EndDate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakKind {
    Fixed,
    Window,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HalfDayPoint {
    Middle,
    FixedTime,
    BreakStart,
}

/// The dated part of a shift (one shift version). Defaults are AC-AT-01.2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShiftTimes {
    pub kind: ShiftKind,
    pub start_time: NaiveTime,
    pub end_time: NaiveTime,
    pub is_night: bool,
    pub night_date_rule: NightDateRule,
    pub required_minutes: i32,
    pub window_before_min: i32,
    pub window_after_min: i32,
    pub flex_earliest: Option<NaiveTime>,
    pub flex_latest: Option<NaiveTime>,
    pub core_start: Option<NaiveTime>,
    pub core_end: Option<NaiveTime>,
    pub break_kind: Option<BreakKind>,
    pub break_start: Option<NaiveTime>,
    pub break_end: Option<NaiveTime>,
    pub half_day_point: HalfDayPoint,
    pub half_day_time: Option<NaiveTime>,
}

fn t(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).expect("valid time")
}

impl Default for ShiftTimes {
    fn default() -> Self {
        Self {
            kind: ShiftKind::Fixed,
            start_time: t(9, 0),
            end_time: t(18, 0),
            is_night: false,
            night_date_rule: NightDateRule::StartDate,
            required_minutes: 480,
            window_before_min: 180,
            window_after_min: 360,
            flex_earliest: None,
            flex_latest: None,
            core_start: None,
            core_end: None,
            break_kind: None,
            break_start: None,
            break_end: None,
            half_day_point: HalfDayPoint::Middle,
            half_day_time: None,
        }
    }
}

/// (field, code, message) for each rule broken.
pub type ShiftErrors = Vec<(&'static str, &'static str, String)>;

/// Minutes from `start` to `x`, going forward past midnight when needed.
fn after(start: NaiveTime, x: NaiveTime) -> i64 {
    (x - start).num_minutes().rem_euclid(24 * 60)
}

impl ShiftTimes {
    pub fn check(&self) -> ShiftErrors {
        let mut e = ShiftErrors::new();
        let flexible = matches!(self.kind, ShiftKind::Flexible | ShiftKind::FlexibleCore);
        let crosses = self.end_time <= self.start_time;
        if !flexible && crosses && !self.is_night {
            e.push(("is_night", "night_flag_required", "This shift ends at or before it starts; mark it as a night shift".into()));
        }
        if !flexible && !crosses && self.is_night {
            e.push(("is_night", "invalid_value", "Only a shift that crosses midnight is a night shift".into()));
        }
        if !(1..=1440).contains(&self.required_minutes) {
            e.push(("required_minutes", "invalid_value", "Required minutes must be 1 to 1440".into()));
        }
        for (f, v) in [("window_before_min", self.window_before_min), ("window_after_min", self.window_after_min)] {
            if !(0..=720).contains(&v) {
                e.push((f, "invalid_value", "Window must be 0 to 720 minutes".into()));
            }
        }
        match (flexible, self.flex_earliest, self.flex_latest) {
            (true, Some(a), Some(b)) => {
                if a == b {
                    e.push(("flex_latest", "invalid_value", "The flexible band needs a start and an end".into()));
                }
                if self.kind == ShiftKind::FlexibleCore {
                    match (self.core_start, self.core_end) {
                        (Some(cs), Some(ce)) => {
                            let band = after(a, b);
                            if !(after(a, cs) < after(a, ce) && after(a, ce) <= band) {
                                e.push(("core_start", "core_outside_band", "Core hours must fall inside the flexible band".into()));
                            }
                        }
                        _ => e.push(("core_start", "required", "Core start and end are required".into())),
                    }
                }
            }
            (true, _, _) => e.push(("flex_earliest", "required", "Earliest check-in and latest check-out are required".into())),
            (false, a, b) if a.is_some() || b.is_some() => e.push(("flex_earliest", "invalid_value", "Only flexible shifts have a band".into())),
            _ => {}
        }
        if self.kind != ShiftKind::FlexibleCore && (self.core_start.is_some() || self.core_end.is_some()) {
            e.push(("core_start", "invalid_value", "Only flexible shifts with core hours have core times".into()));
        }
        if self.kind == ShiftKind::Split {
            match (self.break_kind, self.break_start, self.break_end) {
                (Some(_), Some(bs), Some(be)) => {
                    let len = after(self.start_time, self.end_time);
                    if !(0 < after(self.start_time, bs) && after(self.start_time, bs) < after(self.start_time, be) && after(self.start_time, be) < len) {
                        e.push(("break_start", "invalid_value", "The break must fall inside the shift".into()));
                    }
                }
                _ => e.push(("break_kind", "required", "A split shift needs its break type, start and end".into())),
            }
        } else if self.break_kind.is_some() || self.break_start.is_some() || self.break_end.is_some() {
            e.push(("break_kind", "invalid_value", "Only split shifts have a break here; other breaks are set in the policy set".into()));
        }
        if (self.half_day_point == HalfDayPoint::FixedTime) != self.half_day_time.is_some() {
            e.push(("half_day_time", "invalid_value", "Give a half-day time only with the fixed-time split point".into()));
        }
        if self.half_day_point == HalfDayPoint::BreakStart && self.kind != ShiftKind::Split {
            e.push(("half_day_point", "invalid_value", "Break start is only for split shifts".into()));
        }
        e
    }
}

/// One slot of a rotation pattern: a shift, or a weekly off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Shift(Uuid),
    WeeklyOff,
}

/// Roster entries for `weeks` weeks from `start`: week i uses `pattern[i % len]`; days whose ISO weekday
/// (1 = Monday) is in `offs` are weekly offs. An empty pattern gives no entries.
pub fn rotation(start: NaiveDate, weeks: u32, pattern: &[Slot], offs: &[u32]) -> Vec<(NaiveDate, Slot)> {
    if pattern.is_empty() {
        return Vec::new();
    }
    (0..weeks * 7)
        .filter_map(|i| {
            let d = start.checked_add_days(Days::new(i as u64))?;
            let slot = if offs.contains(&d.weekday().number_from_monday()) { Slot::WeeklyOff } else { pattern[(i / 7) as usize % pattern.len()] };
            Some((d, slot))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(s: &ShiftTimes) -> Vec<&'static str> {
        s.check().iter().map(|e| e.1).collect()
    }

    #[test]
    fn default_shift_is_valid_and_times_parse_without_seconds() {
        assert!(ShiftTimes::default().check().is_empty());
        let s: ShiftTimes = serde_json::from_value(serde_json::json!({ "start_time": "06:00", "end_time": "14:00" })).unwrap();
        assert_eq!((s.start_time, s.end_time, s.required_minutes), (t(6, 0), t(14, 0), 480));
    }

    #[test]
    fn crossing_midnight_needs_the_night_flag_and_only_then() {
        let mut s = ShiftTimes { start_time: t(22, 0), end_time: t(6, 0), ..Default::default() };
        assert_eq!(codes(&s), ["night_flag_required"]);
        s.is_night = true;
        assert!(s.check().is_empty());
        let day = ShiftTimes { is_night: true, ..Default::default() };
        assert_eq!(codes(&day), ["invalid_value"]);
    }

    #[test]
    fn core_hours_must_sit_inside_the_band() {
        let band = ShiftTimes { kind: ShiftKind::FlexibleCore, flex_earliest: Some(t(8, 0)), flex_latest: Some(t(20, 0)), ..Default::default() };
        assert_eq!(codes(&band), ["required"]);
        let ok = ShiftTimes { core_start: Some(t(11, 0)), core_end: Some(t(16, 0)), ..band.clone() };
        assert!(ok.check().is_empty());
        let out = ShiftTimes { core_start: Some(t(7, 0)), core_end: Some(t(16, 0)), ..band };
        assert_eq!(codes(&out), ["core_outside_band"]);
    }

    #[test]
    fn split_break_inside_the_shift_even_overnight() {
        let split = ShiftTimes { kind: ShiftKind::Split, break_kind: Some(BreakKind::Fixed), break_start: Some(t(13, 0)), break_end: Some(t(15, 0)), ..Default::default() };
        assert!(split.check().is_empty());
        let outside = ShiftTimes { break_end: Some(t(19, 0)), ..split.clone() };
        assert_eq!(codes(&outside), ["invalid_value"]);
        let night = ShiftTimes { start_time: t(20, 0), end_time: t(4, 0), is_night: true, break_start: Some(t(23, 30)), break_end: Some(t(0, 30)), ..split };
        assert!(night.check().is_empty());
        let fixed_with_break = ShiftTimes { break_kind: Some(BreakKind::Fixed), ..Default::default() };
        assert_eq!(codes(&fixed_with_break), ["invalid_value"]);
    }

    #[test]
    fn rotation_cycles_weekly_and_keeps_weekly_offs() {
        let (m, e) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let start = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(); // a Monday
        let r = rotation(start, 3, &[Slot::Shift(m), Slot::Shift(e)], &[7]);
        assert_eq!(r.len(), 21);
        assert_eq!(r[0], (start, Slot::Shift(m)));
        assert_eq!(r[6].1, Slot::WeeklyOff); // Sunday
        assert_eq!(r[7].1, Slot::Shift(e)); // week 2
        assert_eq!(r[14].1, Slot::Shift(m)); // back to the first
        assert!(rotation(start, 2, &[], &[]).is_empty());
    }
}
