//! Reporting manager rules (BR-EM-11): never self, never a loop (A → B → C → A).

use uuid::Uuid;

/// Would making `new_manager` the manager of `employee` create a loop?
/// `manager_of` returns each employee's current manager. The walk is bounded so bad data can't hang it.
pub fn would_loop(employee: Uuid, new_manager: Uuid, manager_of: impl Fn(Uuid) -> Option<Uuid>) -> bool {
    if employee == new_manager {
        return true;
    }
    let mut current = Some(new_manager);
    for _ in 0..10_000 {
        match current {
            None => return false,
            Some(m) if m == employee => return true,
            Some(m) => current = manager_of(m),
        }
    }
    true // a chain this long is itself broken; refuse rather than accept
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn ids() -> (Uuid, Uuid, Uuid, Uuid) {
        (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4())
    }

    #[test]
    fn self_is_a_loop() {
        let (a, ..) = ids();
        assert!(would_loop(a, a, |_| None));
    }

    #[test]
    fn three_level_loop_is_refused() {
        // B reports to C, C reports to A. Making B A's manager closes A → B → C → A (edge case 4).
        let (a, b, c, _) = ids();
        let chain = HashMap::from([(b, c), (c, a)]);
        assert!(would_loop(a, b, |x| chain.get(&x).copied()));
    }

    #[test]
    fn unrelated_chain_is_fine() {
        let (a, b, c, d) = ids();
        let chain = HashMap::from([(b, c), (c, d)]);
        assert!(!would_loop(a, b, |x| chain.get(&x).copied()));
    }
}
