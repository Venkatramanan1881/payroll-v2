//! Employee code pattern (EM-01, BR-EM-01, AC-EM-02.1–02.6).
//! Default: organisation short code + branch short code + 4-digit running number per branch.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "part", rename_all = "snake_case")]
pub enum CodePart {
    OrgShortCode,
    BranchShortCode,
    RunningNumber,
    Text { value: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodePattern {
    pub parts: Vec<CodePart>,
    pub separator: String,
    pub digits: u8,
    pub start_number: u32,
}

impl Default for CodePattern {
    fn default() -> Self {
        Self {
            parts: vec![CodePart::OrgShortCode, CodePart::BranchShortCode, CodePart::RunningNumber],
            separator: String::new(),
            digits: 4,
            start_number: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternError {
    NoRunningNumber,
    DigitsOutOfRange,
    EmptyText,
}

impl CodePattern {
    pub fn validate(&self) -> Result<(), PatternError> {
        if self.parts.iter().filter(|p| **p == CodePart::RunningNumber).count() != 1 {
            return Err(PatternError::NoRunningNumber);
        }
        if !(1..=9).contains(&self.digits) {
            return Err(PatternError::DigitsOutOfRange);
        }
        if self.parts.iter().any(|p| matches!(p, CodePart::Text { value } if value.trim().is_empty())) {
            return Err(PatternError::EmptyText);
        }
        Ok(())
    }

    /// The running-number sequence a code belongs to. Per branch when the pattern contains the
    /// branch short code (keyed by branch id, so a short-code rename keeps counting), else per organisation.
    pub fn sequence_key(&self, branch_id: &str) -> String {
        if self.parts.contains(&CodePart::BranchShortCode) { format!("branch:{branch_id}") } else { "org".into() }
    }

    /// Builds the code for running number `n`. A number wider than `digits` simply uses more digits
    /// (PBYCHN10000, AC-EM-02.6).
    pub fn format(&self, org_short_code: &str, branch_short_code: &str, n: u32) -> String {
        let width = self.digits as usize;
        self.parts
            .iter()
            .map(|p| match p {
                CodePart::OrgShortCode => org_short_code.to_string(),
                CodePart::BranchShortCode => branch_short_code.to_string(),
                CodePart::RunningNumber => format!("{n:0width$}"),
                CodePart::Text { value } => value.trim().to_string(),
            })
            .collect::<Vec<_>>()
            .join(&self.separator)
    }

    /// The number to use after `last_value` from the sequence row (0 = nothing issued yet).
    pub fn next_number(&self, last_value: u32) -> u32 {
        last_value.max(self.start_number.saturating_sub(1)) + 1
    }

    /// HR is warned once 90% of the digit range is used (AC-EM-02.6).
    pub fn range_nearly_used(&self, n: u32) -> bool {
        let max = 10u64.pow(self.digits as u32) - 1;
        u64::from(n) * 10 >= max * 9
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pattern_matches_ac_em_02_1() {
        let p = CodePattern::default();
        assert_eq!(p.format("PBY", "CHN", 1), "PBYCHN0001");
        assert_eq!(p.format("PBY", "CHN", 2), "PBYCHN0002");
        assert_eq!(p.sequence_key("b1"), "branch:b1");
        assert!(p.validate().is_ok());
    }

    #[test]
    fn overflow_adds_a_digit() {
        assert_eq!(CodePattern::default().format("PBY", "CHN", 10000), "PBYCHN10000");
    }

    #[test]
    fn warns_at_ninety_percent() {
        let p = CodePattern::default();
        // 90% of 9999 is 8999.1, so 9000 is the first number that warns.
        assert!(!p.range_nearly_used(8999));
        assert!(p.range_nearly_used(9000));
    }

    #[test]
    fn custom_pattern_with_separator_and_text() {
        let p = CodePattern {
            parts: vec![CodePart::Text { value: "EMP".into() }, CodePart::RunningNumber],
            separator: "-".into(),
            digits: 5,
            start_number: 100,
        };
        assert_eq!(p.sequence_key("b1"), "org");
        assert_eq!(p.next_number(0), 100);
        assert_eq!(p.next_number(150), 151);
        assert_eq!(p.format("PBY", "CHN", 100), "EMP-00100");
    }

    #[test]
    fn pattern_needs_exactly_one_running_number() {
        let mut p = CodePattern::default();
        p.parts.retain(|x| *x != CodePart::RunningNumber);
        assert_eq!(p.validate(), Err(PatternError::NoRunningNumber));
    }
}
