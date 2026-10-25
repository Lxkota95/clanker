//! Rainbow Six Siege rank tiers and the server role changes they imply.
//!
//! Kept free of Discord I/O so it can be unit tested: callers fetch the server's roles and the
//! member's current roles, call [`plan`], then apply the result. `/r6rank` gets the tier from the
//! member's own choice; `/r6verify` gets it from a [`source::RankSource`]; both reuse [`plan`].

pub mod arenyze;
pub mod source;

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

    /// The tier for a Ranked 3.0 rank-points total. Boundaries from Arenyze's Ranked 3.0 rank list
    /// (`/ranks?version=v7`, 2026-10-25): each tier spans 500 points (five divisions of 100) from
    /// Copper at 1000 to Champion at 4500; Legend starts at 5000.
    pub fn from_rank_points(points: u32) -> Self {
        match points {
            0..1000 => Self::Unranked,
            1000..1500 => Self::Copper,
            1500..2000 => Self::Bronze,
            2000..2500 => Self::Silver,
            2500..3000 => Self::Gold,
            3000..3500 => Self::Platinum,
            3500..4000 => Self::Emerald,
            4000..4500 => Self::Diamond,
            4500..5000 => Self::Champion,
            5000.. => Self::Legend,
        }
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
    fn rank_points_map_to_tiers_at_the_boundaries() {
        assert_eq!(RankTier::from_rank_points(0), RankTier::Unranked);
        assert_eq!(RankTier::from_rank_points(999), RankTier::Unranked);
        assert_eq!(RankTier::from_rank_points(1000), RankTier::Copper);
        assert_eq!(RankTier::from_rank_points(2999), RankTier::Gold);
        assert_eq!(RankTier::from_rank_points(3500), RankTier::Emerald);
        assert_eq!(RankTier::from_rank_points(4999), RankTier::Champion);
        assert_eq!(RankTier::from_rank_points(5000), RankTier::Legend);
        assert_eq!(RankTier::from_rank_points(9000), RankTier::Legend);
    }

    #[test]
    fn role_names_match_tier_names() {
        assert_eq!(RankTier::Unranked.role_name(), "Unranked");
        assert_eq!(RankTier::Legend.role_name(), "Legend");
    }
}
