//! EM settings group (BRD EM-01..EM-11), stored in org_settings_versions.setting_values.
//! A key that is missing takes the BRD default, so an organisation with no saved version works as-is.

use crate::employee_code::CodePattern;
use chrono::{Months, NaiveDate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeMode {
    Automatic,
    Custom,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllowedBranches {
    MasterOnly,
    Chosen,
    All,
}

/// EM-10: what a branch-limited HR Manager may do with employees whose *allowed* branch is in scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllowedBranchAccess {
    NotVisible,
    View,
    ViewEdit,
}

/// Fields an organisation may make mandatory on top of the always-mandatory ones (EM-02).
pub const OPTIONAL_FIELDS: [&str; 11] = [
    "date_of_birth",
    "gender",
    "email",
    "address",
    "emergency_contact_name",
    "emergency_contact_phone",
    "designation_id",
    "contractor_id",
    "reporting_manager_id",
    "enrolment_id",
    "probation_end_date",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmSettings {
    pub employee_code_mode: CodeMode,                    // EM-01
    pub employee_code_pattern: CodePattern,              // EM-01 (used when mode = custom)
    pub mandatory_fields: Vec<String>,                   // EM-02
    pub self_edit_contact: bool,                         // EM-03
    pub probation_months: u32,                           // EM-04
    pub probation_reminder_days: u32,                    // EM-04
    pub allowed_branches: AllowedBranches,               // EM-06
    pub hr_allowed_branch_access: AllowedBranchAccess,   // EM-10
    pub contract_requires_contractor: bool,              // EM-11
}

impl Default for EmSettings {
    fn default() -> Self {
        Self {
            employee_code_mode: CodeMode::Automatic,
            employee_code_pattern: CodePattern::default(),
            mandatory_fields: Vec::new(),
            self_edit_contact: true,
            probation_months: 6,
            probation_reminder_days: 15,
            allowed_branches: AllowedBranches::MasterOnly,
            hr_allowed_branch_access: AllowedBranchAccess::View,
            contract_requires_contractor: false,
        }
    }
}

impl EmSettings {
    /// Reads a stored version; unknown keys (other groups' or future settings) are ignored.
    pub fn from_json(v: serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(v)
    }

    /// The pattern codes are built from: the BRD default unless the organisation chose a custom one.
    pub fn code_pattern(&self) -> CodePattern {
        match self.employee_code_mode {
            CodeMode::Custom => self.employee_code_pattern.clone(),
            _ => CodePattern::default(),
        }
    }

    pub fn is_mandatory(&self, field: &str) -> bool {
        self.mandatory_fields.iter().any(|f| f == field)
    }

    /// Default probation end: joining date + N months - 1 day (6 months from 1 Apr ends 30 Sep).
    pub fn default_probation_end(&self, joining: NaiveDate) -> Option<NaiveDate> {
        joining.checked_add_months(Months::new(self.probation_months))?.pred_opt()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    English,
    Tamil,
    Hindi,
}

/// OR settings group (BRD OR-01..OR-04, AC-OR-04.1 defaults).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OrSettings {
    pub financial_year_start_month: u32, // OR-01: 1 = calendar year, 4 = April–March, else custom
    pub leave_year_start_month: u32,     // OR-01
    pub week_start_day: u32,             // OR-02: ISO, 1 = Monday
    pub languages: Vec<Language>,        // OR-03
    pub currency: String,                // OR-04: ISO 4217
}

impl Default for OrSettings {
    fn default() -> Self {
        Self {
            financial_year_start_month: 4,
            leave_year_start_month: 4,
            week_start_day: 1,
            languages: vec![Language::English],
            currency: "INR".into(),
        }
    }
}

/// AC settings group (BRD AC-04). AC-01–03, 05, 07 live in roles, scopes and approver routing; AC-06 and
/// AC-08 join this group when their features are built.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AcSettings {
    /// AC-04: false = HR Manager only (default); true = HR Manager with Organisation Admin approval.
    pub settings_change_approval: bool,
}

/// (setting key, message) for each value the schema refuses.
pub type SettingErrors = Vec<(String, String)>;

pub const GROUPS: [&str; 3] = ["OR", "EM", "AC"];

/// A stored version read leniently, with defaults filled in. None = unknown group.
pub fn filled(group: &str, stored: serde_json::Value) -> Option<serde_json::Value> {
    fn fill<T: Serialize + serde::de::DeserializeOwned + Default>(v: serde_json::Value) -> serde_json::Value {
        serde_json::to_value(serde_json::from_value::<T>(v).unwrap_or_default()).expect("settings serialise")
    }
    match group {
        "OR" => Some(fill::<OrSettings>(stored)),
        "EM" => Some(fill::<EmSettings>(stored)),
        "AC" => Some(fill::<AcSettings>(stored)),
        _ => None,
    }
}

/// Strict check of a new version (O17): unknown keys and bad values are named per key.
/// Returns the complete values (missing keys take defaults). None = unknown group.
pub fn check_group(group: &str, values: &serde_json::Map<String, serde_json::Value>) -> Option<Result<serde_json::Value, SettingErrors>> {
    match group {
        "OR" => Some(check::<OrSettings>(values, |s| {
            let mut e = Vec::new();
            for (k, m) in [("financial_year_start_month", s.financial_year_start_month), ("leave_year_start_month", s.leave_year_start_month)] {
                if !(1..=12).contains(&m) {
                    e.push((k.into(), "Month must be 1 to 12".into()));
                }
            }
            if !(1..=7).contains(&s.week_start_day) {
                e.push(("week_start_day".into(), "Day must be 1 (Monday) to 7 (Sunday)".into()));
            }
            if s.languages.is_empty() {
                e.push(("languages".into(), "Turn on at least one language".into()));
            }
            if s.currency.len() != 3 || !s.currency.chars().all(|c| c.is_ascii_uppercase()) {
                e.push(("currency".into(), "Use a 3-letter currency code such as INR".into()));
            }
            e
        })),
        "EM" => Some(check::<EmSettings>(values, |s| {
            let mut e = Vec::new();
            if s.employee_code_pattern.validate().is_err() {
                e.push(("employee_code_pattern".into(), "This pattern can't produce employee codes".into()));
            }
            if let Some(f) = s.mandatory_fields.iter().find(|f| !OPTIONAL_FIELDS.contains(&f.as_str())) {
                e.push(("mandatory_fields".into(), format!("{f} can't be made mandatory")));
            }
            e
        })),
        "AC" => Some(check::<AcSettings>(values, |_| Vec::new())),
        _ => None,
    }
}

pub(crate) fn check<T: Serialize + serde::de::DeserializeOwned + Default>(
    values: &serde_json::Map<String, serde_json::Value>,
    rules: impl Fn(&T) -> SettingErrors,
) -> Result<serde_json::Value, SettingErrors> {
    let known = serde_json::to_value(T::default()).expect("settings serialise");
    let (mut errs, mut good) = (SettingErrors::new(), serde_json::Map::new());
    for (k, v) in values {
        if known.get(k).is_none() {
            errs.push((k.clone(), "Not a setting of this group".into()));
        } else if let Err(e) = serde_json::from_value::<T>(serde_json::json!({ k: v })) {
            errs.push((k.clone(), e.to_string()));
        } else {
            good.insert(k.clone(), v.clone());
        }
    }
    // Rules run on the readable keys too, so one reply names every bad key.
    let s: T = serde_json::from_value(serde_json::Value::Object(good)).map_err(|e| vec![(String::new(), e.to_string())])?;
    errs.extend(rules(&s));
    if errs.is_empty() { Ok(serde_json::to_value(s).expect("settings serialise")) } else { Err(errs) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_group_names_each_bad_key_and_fills_defaults() {
        let m = |v: serde_json::Value| v.as_object().unwrap().clone();
        let ok = check_group("OR", &m(serde_json::json!({ "week_start_day": 7 }))).unwrap().unwrap();
        assert_eq!(ok["week_start_day"], 7);
        assert_eq!(ok["currency"], "INR");
        let errs = check_group("OR", &m(serde_json::json!({ "week_start_day": 9, "colour": "red", "languages": ["klingon"] })))
            .unwrap()
            .unwrap_err();
        let keys: Vec<&str> = errs.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"colour") && keys.contains(&"languages"), "{keys:?}");
        let errs = check_group("OR", &m(serde_json::json!({ "week_start_day": 9 }))).unwrap().unwrap_err();
        assert_eq!(errs[0].0, "week_start_day");
        let errs = check_group("EM", &m(serde_json::json!({ "mandatory_fields": ["full_name"] }))).unwrap().unwrap_err();
        assert_eq!(errs[0].0, "mandatory_fields");
        assert!(check_group("XX", &serde_json::Map::new()).is_none());
    }

    #[test]
    fn empty_json_gives_brd_defaults() {
        let s = EmSettings::from_json(serde_json::json!({})).unwrap();
        assert_eq!(s, EmSettings::default());
        assert_eq!(s.code_pattern(), CodePattern::default());
    }

    #[test]
    fn unknown_keys_are_ignored_and_known_keys_read() {
        let s = EmSettings::from_json(serde_json::json!({
            "allowed_branches": "chosen", "mandatory_fields": ["email"], "EM-07": "block"
        }))
        .unwrap();
        assert_eq!(s.allowed_branches, AllowedBranches::Chosen);
        assert!(s.is_mandatory("email"));
    }

    #[test]
    fn probation_end_is_inclusive() {
        let s = EmSettings::default();
        let d = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        assert_eq!(s.default_probation_end(d(2026, 4, 1)), Some(d(2026, 9, 30)));
        assert_eq!(s.default_probation_end(d(2026, 8, 31)), Some(d(2027, 2, 27)));
    }
}
