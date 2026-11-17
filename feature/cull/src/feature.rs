//! The portals' settings, as the host and the portals' passes read them
//! (`settings::PORTALS` describes them).

use helio_feature_api::BuildEnv;

/// Portals' settings (a renderer config's `features`, and the host's
/// projection).
#[derive(Clone, Debug)]
pub struct PortalSettings {
    /// Off: no portal pass builds (a level without portals pays nothing).
    pub enabled: bool,
    /// How many openings deep a route goes (a portal seen through portals).
    pub max_chain_depth: usize,
    /// The most routes a frame draws, shallowest first.
    pub max_chains: usize,
}

impl Default for PortalSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_chain_depth: 3,
            max_chains: 300,
        }
    }
}

/// Whether the graph being built draws portals.
pub fn portals_enabled(env: &BuildEnv<'_>) -> bool {
    portal_settings(env).enabled
}

/// The portals' settings in the graph being built.
pub fn portal_settings(env: &BuildEnv<'_>) -> PortalSettings {
    env.setting::<PortalSettings>().cloned().unwrap_or_default()
}
