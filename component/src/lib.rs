//! Portals (`pulsar.portals`): the component crate of the plugin that owns
//! portals.
//!
//! - The component ([`component`]): `PortalComponent`, an opening that shows
//!   what is behind its partner (the other portal with its Link ID).
//! - Pairing and projection ([`projection`]): each frame the authored
//!   portals are paired, expanded into views and recursion chains whose
//!   coordinate spaces come from the frame's one table, uploaded by the
//!   feature's derivation (`helio_pass_portal_cull::scene_rows`) and counted
//!   for the passes (`helio_pass_portal_cull::PORTAL_COUNTS`). The engine
//!   runs it for every renderer as a space producer ([`PortalSync`]).
//! - The plugin's Helio features ([`render_features`]: portal culling,
//!   `helio_pass_portal_cull::PortalCullFeature`, and the portal draws,
//!   `helio_pass_portal_instances::PortalFeature`) and their settings from
//!   the project's portal settings ([`configure_renderer`], the group
//!   `helio_pass_portal_cull::settings::PORTALS`).
//!
//! What an editor shows through the portal overlay (labels, the geometry a
//! selected portal maps) are the overlay's payloads ([`PORTAL_DEBUG_TEXTS`],
//! [`PORTAL_DEBUG_GEOMETRY`]), which an editor publishes for its viewport.
//!
//! This crate registers nothing itself: the plugin's root crate
//! (`portals_plugin`) submits the component class, the space producer, the
//! features and the settings group, in one place.

pub mod component;
pub mod projection;

pub use component::*;
pub use helio_pass_portal_instances::{PORTAL_DEBUG_GEOMETRY, PORTAL_DEBUG_TEXTS, PortalDebugText};
pub use projection::{
    Pairing, Portal, PortalProblem, PortalSync, authored_portals, build_resolver, pair_portals,
};

/// The editor viewport's debug mode for portal diagnostics: the renderer
/// keeps its colour output lit while the editor draws portal labels and
/// pairing overlays.
pub const PORTAL_DEBUG_VIEW_MODE: u32 = 35;

/// Portal culling and the portal draws (a G-buffer drawer, the mask and the
/// editor overlay).
pub fn render_features() -> helio_feature_api::FeatureSet {
    helio_feature_api::FeatureSet::new()
        .with(helio_pass_portal_cull::PortalCullFeature)
        .with(helio_pass_portal_instances::PortalFeature)
}

/// The features' settings from the project's portal settings.
pub fn configure_renderer(config: helio::RendererConfig) -> helio::RendererConfig {
    use engine_backend::subsystems::render::helio_renderer::project_settings;
    config.with_feature(helio_pass_portal_cull::PortalSettings::from_settings(
        &project_settings::resolved(&helio_pass_portal_cull::settings::PORTALS),
    ))
}
