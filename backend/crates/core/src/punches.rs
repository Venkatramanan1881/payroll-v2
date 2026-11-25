//! Punch rules applied once at receipt (DOMAIN_MODEL §5 PunchTimeResolver, Issue Resolutions §3.4, DEC-21):
//! the effective punch time, the fence check (PC-04/14/16, BR-AT-09) and the flags they raise.

use crate::policy::{AccuracyRule, AtSettings, BlockOrFlag, FenceRule};
use chrono::{DateTime, Duration, Utc};

/// Flags as stored in `app.punch_flag`.
pub const CLOCK_MISMATCH: &str = "clock_mismatch";
pub const LATE_SYNC: &str = "late_sync";
pub const OUTSIDE_FENCE: &str = "outside_fence";
pub const POOR_ACCURACY: &str = "poor_accuracy";
pub const NO_LOCATION: &str = "no_location";
pub const BRANCH_NOT_ALLOWED: &str = "branch_not_allowed";
pub const OFF_DAY: &str = "off_day";

/// The time a punch counts at, and the time flags it raises.
/// - Live (sent at once): the phone time, unless it is off from receipt by more than PC-08 → receipt time.
/// - Queued offline: the phone time, unless the uptime-derived time differs by more than PC-08 → uptime time.
///   Older than PC-07 days at receipt → late sync (still accepted).
pub fn effective_time(
    client: DateTime<Utc>,
    uptime: Option<DateTime<Utc>>,
    received: DateTime<Utc>,
    queued: bool,
    s: &AtSettings,
) -> (DateTime<Utc>, Vec<&'static str>) {
    let tolerance = Duration::minutes(s.device_clock_tolerance_minutes as i64);
    let mut flags = Vec::new();
    let reference = if queued { uptime } else { Some(received) };
    let at = match reference {
        Some(r) if (client - r).abs() > tolerance => {
            flags.push(CLOCK_MISMATCH);
            r
        }
        _ => client,
    };
    if queued && received - at > Duration::days(s.late_sync_days as i64) {
        flags.push(LATE_SYNC);
    }
    (at, flags)
}

/// Great-circle distance in metres (haversine; fences are at most 5 km, so the sphere is close enough).
pub fn distance_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let (dp, dl) = ((lat2 - lat1).to_radians(), (lon2 - lon1).to_radians());
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * 6_371_000.0 * a.sqrt().asin()
}

/// A fence version in force at punch time; `allowed` = its branch is one the employee may punch at (PC-16).
#[derive(Debug, Clone)]
pub struct Fence<Id> {
    pub id: Id,
    pub branch: Id,
    pub latitude: f64,
    pub longitude: f64,
    pub radius_m: f64,
    pub allowed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy_m: f64,
}

/// What the checks decided: refused with a code, or accepted with flags and the nearest fence.
#[derive(Debug, PartialEq)]
pub enum Verdict<Id> {
    Refused(&'static str),
    Accepted { flags: Vec<&'static str>, nearest: Option<(Id, Id, f64)> },
}

/// Location rules for an app (`web` false) or web punch. A fence of a branch that is not allowed still
/// places the punch but flags it (EM-07). No fences at all: nothing to check against, no flag.
pub fn check_location<Id: Copy>(web: bool, loc: Option<&Location>, fences: &[Fence<Id>], s: &AtSettings) -> Verdict<Id> {
    let (need_location, need_fence) = if web {
        (s.web_requires_location, s.web_requires_inside_fence)
    } else {
        (s.app_requires_location, s.app_requires_inside_fence)
    };
    let mut flags = Vec::new();
    let Some(loc) = loc else {
        if !need_location {
            return Verdict::Accepted { flags, nearest: None };
        }
        if web && s.web_location_unavailable == BlockOrFlag::Flag {
            return Verdict::Accepted { flags: vec![NO_LOCATION], nearest: None };
        }
        return Verdict::Refused("location_required");
    };
    if loc.accuracy_m > s.location_accuracy_metres as f64 {
        match s.location_inaccurate {
            AccuracyRule::Block => return Verdict::Refused("poor_accuracy"),
            AccuracyRule::Flag => flags.push(POOR_ACCURACY),
            AccuracyRule::Accept => {}
        }
    }
    // (fence, metres past its edge; <= 0 is inside)
    let edges: Vec<(&Fence<Id>, f64)> = fences.iter().map(|f| (f, distance_m(loc.latitude, loc.longitude, f.latitude, f.longitude) - f.radius_m)).collect();
    let closest = |allowed: Option<bool>, inside: bool| {
        edges
            .iter()
            .filter(|(f, e)| allowed.is_none_or(|a| f.allowed == a) && (!inside || *e <= 0.0))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .copied()
    };
    let at = |(f, e): (&Fence<Id>, f64)| Some((f.id, f.branch, (e + f.radius_m).max(0.0)));
    // Inside an allowed fence: fine. Inside only another branch's fence: placed there, flagged (EM-07).
    // Otherwise outside: measured to the nearest allowed fence (any fence if none is allowed).
    if let Some(hit) = closest(Some(true), true) {
        return Verdict::Accepted { flags, nearest: at(hit) };
    }
    if let Some(hit) = closest(Some(false), true) {
        flags.push(BRANCH_NOT_ALLOWED);
        return Verdict::Accepted { flags, nearest: at(hit) };
    }
    let near = closest(Some(true), false).or_else(|| closest(None, false));
    if near.is_some() && need_fence {
        match s.outside_fence {
            FenceRule::Block => return Verdict::Refused("outside_fence"),
            FenceRule::Flag => flags.push(OUTSIDE_FENCE),
            FenceRule::Allow => {}
        }
    }
    Verdict::Accepted { flags, nearest: near.and_then(at) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 24, h, m, 0).unwrap()
    }

    #[test]
    fn offline_clock_moved_back_uses_uptime_time_w10() {
        let s = AtSettings::default();
        // Phone says 08:32, uptime says 09:02, synced 13:40 → recorded 09:02, flagged.
        assert_eq!(effective_time(t(8, 32), Some(t(9, 2)), t(13, 40), true, &s), (t(9, 2), vec![CLOCK_MISMATCH]));
        assert_eq!(effective_time(t(9, 2), Some(t(9, 3)), t(13, 40), true, &s), (t(9, 2), vec![]));
        let late = effective_time(t(9, 0), None, t(9, 0) + Duration::days(4), true, &s);
        assert_eq!(late, (t(9, 0), vec![LATE_SYNC]));
    }

    #[test]
    fn live_punch_with_wrong_clock_takes_receipt_time() {
        let s = AtSettings::default();
        assert_eq!(effective_time(t(9, 20), None, t(9, 0), false, &s), (t(9, 0), vec![CLOCK_MISMATCH]));
        assert_eq!(effective_time(t(9, 2), None, t(9, 0), false, &s), (t(9, 2), vec![]));
    }

    #[test]
    fn fence_rules_follow_settings() {
        let mut s = AtSettings::default();
        let fence = |lat, allowed| Fence { id: 1, branch: 10, latitude: lat, longitude: 80.27, radius_m: 200.0, allowed };
        let near = Location { latitude: 13.0800, longitude: 80.27, accuracy_m: 20.0 };
        // ~450 m north of the fence centre.
        let far = Location { latitude: 13.0841, longitude: 80.27, accuracy_m: 20.0 };
        assert!((distance_m(13.08, 80.27, 13.0841, 80.27) - 456.0).abs() < 5.0);
        assert_eq!(check_location(false, Some(&near), &[fence(13.08, true)], &s), Verdict::Accepted { flags: vec![], nearest: Some((1, 10, 0.0)) });
        let Verdict::Accepted { flags, nearest } = check_location(false, Some(&far), &[fence(13.08, true)], &s) else { panic!() };
        assert_eq!(flags, vec![OUTSIDE_FENCE]);
        assert!((nearest.unwrap().2 - 456.0).abs() < 5.0);
        assert_eq!(check_location(false, Some(&near), &[fence(13.08, false)], &s), Verdict::Accepted { flags: vec![BRANCH_NOT_ALLOWED], nearest: Some((1, 10, 0.0)) });
        // Outside the allowed fence but inside another branch's: EM-07, not outside_fence.
        let other = Fence { id: 2, branch: 20, latitude: 13.0841, longitude: 80.27, radius_m: 200.0, allowed: false };
        assert_eq!(check_location(false, Some(&far), &[fence(13.08, true), other], &s), Verdict::Accepted { flags: vec![BRANCH_NOT_ALLOWED], nearest: Some((2, 20, 0.0)) });
        assert_eq!(check_location::<i32>(false, None, &[], &s), Verdict::Refused("location_required"));
        let poor = Location { accuracy_m: 300.0, ..near.clone() };
        s.location_inaccurate = AccuracyRule::Block;
        assert_eq!(check_location(false, Some(&poor), &[fence(13.08, true)], &s), Verdict::Refused("poor_accuracy"));
        s.outside_fence = FenceRule::Block;
        assert_eq!(check_location(false, Some(&far), &[fence(13.08, true)], &s), Verdict::Refused("outside_fence"));
        // Web without location: blocked by default, flagged when PC-13 says so.
        assert_eq!(check_location::<i32>(true, None, &[], &s), Verdict::Refused("location_required"));
        s.web_location_unavailable = BlockOrFlag::Flag;
        assert_eq!(check_location::<i32>(true, None, &[], &s), Verdict::Accepted { flags: vec![NO_LOCATION], nearest: None });
    }
}
