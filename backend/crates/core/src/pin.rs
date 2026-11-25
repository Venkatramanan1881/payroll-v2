//! Employee PIN rules (EMQ-24: 6 digits) and sign-in lockout (EMQ-25: 5 tries, 15 minutes).

pub const PIN_LENGTH: usize = 6;
pub const MAX_FAILED_ATTEMPTS: i16 = 5;
pub const LOCK_MINUTES: i64 = 15;
pub const TEMPORARY_PIN_DAYS: i64 = 7;

#[derive(Debug, PartialEq, Eq)]
pub enum PinProblem {
    NotSixDigits,
    AllSame,
    Sequential,
}

/// A PIN the employee chooses: 6 digits, not all the same, not a straight run (123456, 987654).
pub fn check_new_pin(pin: &str) -> Result<(), PinProblem> {
    let d: Vec<u8> = pin.bytes().collect();
    if d.len() != PIN_LENGTH || !d.iter().all(u8::is_ascii_digit) {
        return Err(PinProblem::NotSixDigits);
    }
    if d.iter().all(|&c| c == d[0]) {
        return Err(PinProblem::AllSame);
    }
    let steps: Vec<i16> = d.windows(2).map(|w| w[1] as i16 - w[0] as i16).collect();
    if steps.iter().all(|&s| s == 1) || steps.iter().all(|&s| s == -1) {
        return Err(PinProblem::Sequential);
    }
    Ok(())
}

/// True when this failure should lock the account.
pub fn locks_after(failed_attempts_including_this: i16) -> bool {
    failed_attempts_including_this >= MAX_FAILED_ATTEMPTS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_pin() {
        assert_eq!(check_new_pin("482913"), Ok(()));
    }

    #[test]
    fn refuses_weak_pins() {
        assert_eq!(check_new_pin("12345"), Err(PinProblem::NotSixDigits));
        assert_eq!(check_new_pin("12a456"), Err(PinProblem::NotSixDigits));
        assert_eq!(check_new_pin("000000"), Err(PinProblem::AllSame));
        assert_eq!(check_new_pin("123456"), Err(PinProblem::Sequential));
        assert_eq!(check_new_pin("654321"), Err(PinProblem::Sequential));
    }

    #[test]
    fn fifth_failure_locks() {
        assert!(!locks_after(4));
        assert!(locks_after(5));
    }
}
