//! The portal feature's host settings, described once (as `helio_settings`
//! describes the renderer's): engines build their settings UI and read the
//! values back through it.

use helio_settings::{Group, Resolved, integer, toggle};

use crate::PortalSettings;

pub const MAX_CHAIN_DEPTH: &str = "max_chain_depth";

/// The portals' quality: whether their passes build, and how deep and how
/// many routes through them a frame draws. A host lists it with its other
/// settings groups.
pub static PORTALS: Group = Group {
    key: "portals",
    title: "Portals",
    description: "Openings that show another place",
    settings: &[
        toggle(
            "enabled",
            "Portals",
            "Portals",
            true,
            "Portal passes: what an opening shows is drawn through it",
        ),
        integer(
            MAX_CHAIN_DEPTH,
            "Recursion Depth",
            "Portals",
            (1, 8),
            3,
            "How many openings deep a portal seen through portals is drawn: each level draws \
             the content again through every route of that length",
        ),
        integer(
            "max_chains",
            "Routes per Frame",
            "Portals",
            (1, helio_portal_api::MAX_CHAINS as i64),
            300,
            "The most routes through openings a frame draws, shallowest first (six portals \
             three deep make 258)",
        ),
    ],
};

impl PortalSettings {
    /// The settings `settings` (of [`PORTALS`]) hold.
    pub fn from_settings(settings: &Resolved) -> Self {
        debug_assert!(std::ptr::eq(settings.group(), &PORTALS));
        Self {
            enabled: settings.bool("enabled"),
            max_chain_depth: settings.number(MAX_CHAIN_DEPTH) as usize,
            max_chains: settings.number("max_chains") as usize,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_defaults_are_the_default_settings() {
        let resolved = PortalSettings::from_settings(&PORTALS.defaults());
        let default = PortalSettings::default();
        assert_eq!(resolved.enabled, default.enabled);
        assert_eq!(resolved.max_chain_depth, default.max_chain_depth);
        assert_eq!(resolved.max_chains, default.max_chains);
    }
}
