//! Employee status (SM-1) and who approves a change (BR-EM-27).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmployeeStatus {
    Probation,
    Active,
    Notice,
    LongLeave,
    Absconding,
    Exited,
}

use EmployeeStatus::*;

impl EmployeeStatus {
    pub const ALL: [EmployeeStatus; 6] = [Probation, Active, Notice, LongLeave, Absconding, Exited];

    /// SM-1 / BRD EM-R5 allowed moves. Mirrored by CHECK constraints on status_change_requests.
    pub fn can_move_to(self, to: EmployeeStatus) -> bool {
        match (self, to) {
            (from, to) if from == to => false,
            (Exited, _) => false,          // final for the employment period; rejoin is not a status move
            (_, Probation) => false,       // nothing moves back to probation
            (Notice | Absconding, LongLeave) => false,
            _ => true,
        }
    }

    /// Employees in these statuses count toward the plan limit (BR-EM-12).
    pub fn counts_toward_limit(self) -> bool {
        self != Exited
    }

    /// Statuses a new joiner or rejoiner may start in (AC-EM-01.1, AC-EM-14.3).
    pub fn is_valid_initial(self) -> bool {
        matches!(self, Probation | Active)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApproverRole {
    OrgAdmin,
    HrManager,
}

/// BR-EM-27: the Org Admin approves; if an Org Admin raised it, an HR Manager approves.
pub fn approver_role(raiser_is_org_admin: bool) -> ApproverRole {
    if raiser_is_org_admin { ApproverRole::HrManager } else { ApproverRole::OrgAdmin }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The full SM-1 table from STATE_MACHINES.md, row by row (✔ = true).
    #[test]
    fn transition_table_matches_sm1() {
        let expected: [(EmployeeStatus, [bool; 6]); 6] = [
            //            Prob   Active Notice LongL  Absc   Exited
            (Probation,  [false, true,  true,  true,  true,  true]),
            (Active,     [false, false, true,  true,  true,  true]),
            (Notice,     [false, true,  false, false, true,  true]),
            (LongLeave,  [false, true,  true,  false, true,  true]),
            (Absconding, [false, true,  true,  false, false, true]),
            (Exited,     [false, false, false, false, false, false]),
        ];
        for (from, row) in expected {
            for (to, allowed) in EmployeeStatus::ALL.into_iter().zip(row) {
                assert_eq!(from.can_move_to(to), allowed, "{from:?} -> {to:?}");
            }
        }
    }

    #[test]
    fn approver_is_hr_when_org_admin_raised() {
        assert_eq!(approver_role(true), ApproverRole::HrManager);
        assert_eq!(approver_role(false), ApproverRole::OrgAdmin);
    }

    #[test]
    fn only_exited_is_outside_the_limit() {
        for s in EmployeeStatus::ALL {
            assert_eq!(s.counts_toward_limit(), s != Exited);
        }
    }
}
