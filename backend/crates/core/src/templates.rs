//! Sector starter lists (G6, DBQ-04): offered once, while the four job lists are empty (O9).
//! shortcut: the entries below are a first draft, not from the BRD; product to confirm before go-live.

/// (code, name) pairs.
pub type List = &'static [(&'static str, &'static str)];

/// One list of each kind.
pub struct Template {
    pub departments: List,
    pub designations: List,
    pub categories: List,
    pub employee_types: List,
}

/// OR-R2 example categories; OR-R3 types ("probation" is a status, not a type).
const CATEGORIES: &[(&str, &str)] = &[("UNSK", "Unskilled"), ("SEMI", "Semi-skilled"), ("SKL", "Skilled"), ("STAFF", "Staff")];
const TYPES: &[(&str, &str)] = &[("PERM", "Permanent"), ("TRN", "Trainee"), ("CAS", "Casual"), ("CON", "Contract")];

pub fn template(sector: &str) -> Option<Template> {
    let (departments, designations): (List, List) = match sector {
        "factory" => (
            &[("PROD", "Production"), ("QC", "Quality"), ("STORES", "Stores"), ("MAINT", "Maintenance"), ("ADMIN", "Administration")],
            &[("OPR", "Operator"), ("HLP", "Helper"), ("TECH", "Technician"), ("SUP", "Supervisor"), ("MGR", "Manager")],
        ),
        "supermarket" => (
            &[("OPS", "Store operations"), ("BILL", "Billing"), ("STOCK", "Stock and receiving"), ("ADMIN", "Administration")],
            &[("CASH", "Cashier"), ("SA", "Sales associate"), ("PCK", "Packer"), ("SUP", "Supervisor"), ("MGR", "Store manager")],
        ),
        "restaurant_bakery" => (
            &[("KIT", "Kitchen"), ("BAKE", "Bakery"), ("SERV", "Service"), ("ADMIN", "Administration")],
            &[("CHEF", "Chef"), ("COOK", "Commis cook"), ("BAKER", "Baker"), ("STW", "Steward"), ("MGR", "Manager")],
        ),
        "office_services" => (
            &[("OPS", "Operations"), ("ACC", "Accounts"), ("HR", "Human resources"), ("ADMIN", "Administration")],
            &[("AST", "Assistant"), ("EXE", "Executive"), ("SEXE", "Senior executive"), ("MGR", "Manager")],
        ),
        _ => return None,
    };
    Some(Template { departments, designations, categories: CATEGORIES, employee_types: TYPES })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sector_has_all_four_lists_with_unique_codes() {
        for s in ["factory", "supermarket", "restaurant_bakery", "office_services"] {
            let t = template(s).unwrap();
            for list in [t.departments, t.designations, t.categories, t.employee_types] {
                assert!(!list.is_empty());
                let mut codes: Vec<_> = list.iter().map(|(c, _)| c.to_lowercase()).collect();
                codes.sort();
                codes.dedup();
                assert_eq!(codes.len(), list.len(), "{s}");
            }
        }
        assert!(template("bank").is_none());
    }
}
