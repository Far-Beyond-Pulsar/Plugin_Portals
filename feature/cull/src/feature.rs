//! The portals' settings, as the portals' drawer reads them when it builds
//! (`helio_pass_portal_instances::PortalFeature`).

use helio_feature_api::BuildEnv;

/// Portals' settings (a renderer config's `features`).
#[derive(Clone, Debug)]
pub struct PortalSettings {
    /// Off: no portal pass builds (a level without portals pays nothing).
    pub enabled: bool,
}

impl Default for PortalSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Whether the graph being built draws portals.
pub fn portals_enabled(env: &BuildEnv<'_>) -> bool {
    env.setting::<PortalSettings>()
        .is_none_or(|settings| settings.enabled)
}
