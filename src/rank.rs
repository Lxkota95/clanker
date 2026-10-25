//! Rainbow Six Siege rank tiers and the server role changes they imply.
//!
//! Kept free of Discord I/O so it can be unit tested: callers fetch the server's roles and the
//! member's current roles, call [`plan`], then apply the result. `/rank` gets the tier from the
//! member's own choice; a future stats-API lookup would produce a [`RankTier`] the same way and
//! reuse everything here.

use poise::serenity_prelude::RoleId;

/// A Ranked 3.0 tier. Each maps to a server role of the same name (see [`RankTier::role_name`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum RankTier {
    Unranked,
    Copper,
    Bronze,
    Silver,
    Gold,
    Platinum,
    Emerald,
    Diamond,
    Champion,
    Legend,
}

impl RankTier {
    /// Every tier, lowest first.
    pub const ALL: [Self; 10] = [
        Self::Unranked,
        Self::Copper,
        Self::Bronze,
        Self::Silver,
        Self::Gold,
        Self::Platinum,
        Self::Emerald,
        Self::Diamond,
        Self::Champion,
        Self::Legend,
    ];

    /// The server role for this tier: the tier's name, e.g. "Gold".
    pub fn role_name(self) -> &'static str {
        poise::ChoiceParameter::name(&self)
    }
}

/// The role changes that give a member exactly one rank role: `tier`'s.
#[derive(Debug, PartialEq, Eq)]
pub struct RolePlan {
    /// The chosen tier's role, unless the member already has it. `None` also when it doesn't exist
    /// (then it's in `missing`).
    pub add: Option<RoleId>,
    /// Other rank roles the member has, which a new rank replaces.
    pub remove: Vec<RoleId>,
    /// Rank roles that don't exist on the server (all tiers, not just the chosen one), so the
    /// reply can say what to create.
    pub missing: Vec<&'static str>,
}

/// Works out the role changes for `tier`, given the server's roles as `(id, name)` pairs and the
/// role IDs the member has now. Role names match exactly; Discord allows duplicate names, and the
/// first match wins.
pub fn plan(tier: RankTier, server_roles: &[(RoleId, &str)], member_roles: &[RoleId]) -> RolePlan {
    let find = |name: &str| {
        server_roles
            .iter()
            .find_map(|&(id, role_name)| (role_name == name).then_some(id))
    };

    let mut add = None;
    let mut remove = Vec::new();
    let mut missing = Vec::new();
    for other in RankTier::ALL {
        match find(other.role_name()) {
            None => missing.push(other.role_name()),
            Some(id) if other == tier => {
                if !member_roles.contains(&id) {
                    add = Some(id);
                }
            }
            Some(id) => {
                if member_roles.contains(&id) {
                    remove.push(id);
                }
            }
        }
    }
    RolePlan {
        add,
        remove,
        missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLD: RoleId = RoleId::new(1);
    const SILVER: RoleId = RoleId::new(2);
    const UNRELATED: RoleId = RoleId::new(3);

    /// A server with only some of the rank roles, plus an unrelated one.
    fn server() -> Vec<(RoleId, &'static str)> {
        vec![(GOLD, "Gold"), (SILVER, "Silver"), (UNRELATED, "Moderator")]
    }

    #[test]
    fn adds_the_chosen_rank() {
        let plan = plan(RankTier::Gold, &server(), &[UNRELATED]);
        assert_eq!(plan.add, Some(GOLD));
        assert_eq!(plan.remove, Vec::<RoleId>::new());
    }

    #[test]
    fn replaces_the_previous_rank_and_keeps_other_roles() {
        let plan = plan(RankTier::Gold, &server(), &[SILVER, UNRELATED]);
        assert_eq!(plan.add, Some(GOLD));
        assert_eq!(plan.remove, [SILVER]);
    }

    #[test]
    fn nothing_to_do_when_already_that_rank() {
        let plan = plan(RankTier::Gold, &server(), &[GOLD]);
        assert_eq!(plan.add, None);
        assert_eq!(plan.remove, Vec::<RoleId>::new());
    }

    #[test]
    fn reports_missing_rank_roles() {
        let plan = plan(RankTier::Diamond, &server(), &[]);
        assert_eq!(plan.add, None);
        assert!(plan.missing.contains(&"Diamond"));
        assert!(plan.missing.contains(&"Copper"));
        assert!(!plan.missing.contains(&"Gold"));
    }

    #[test]
    fn role_names_match_tier_names() {
        assert_eq!(RankTier::Unranked.role_name(), "Unranked");
        assert_eq!(RankTier::Legend.role_name(), "Legend");
    }
}
