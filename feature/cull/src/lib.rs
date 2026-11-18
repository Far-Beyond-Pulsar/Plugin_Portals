//! The portals' CPU projection and their settings.
//!
//! - The projection a host runs on its authored portals: pairs and poses
//!   ([`PortalRow`]), the resolver ([`SubLevelResolver`]) and the bridge
//!   ([`PortalProjectionBridge`]) that turn them into view rows and the
//!   target-to-source maps ([`PortalProjectionFrame`]), and [`chain_set`],
//!   the chains every drawer draws its content through
//!   (`helio_portal_api::PORTAL_CHAIN_SET`).
//! - The portal math ([`portal_view_map`], [`PortalPair`]) and the plugin's
//!   settings ([`PortalSettings`]).

pub mod feature;
/// The portal feature's host settings.
pub mod settings;
pub use feature::{portal_settings, portals_enabled, PortalSettings};

mod portal_math;
pub use portal_math::{
    crossing_detected, plane_signed_distance, portal_pose_facing, portal_view_map, PortalPair,
    PortalPose,
};
mod resolver;
pub use resolver::{
    PortalOccurrence, PortalProjection, PortalRecord, ResolutionError, ResolvedPortalChain,
    SubLevelActorRecord, SubLevelContents, SubLevelResolver, SubLevelRuntimeContext,
};
mod projection;
pub use projection::{
    PortalProjectionBridge, PortalProjectionConfig, PortalProjectionFrame, PortalProjectionKey,
    ProjectionError, RuntimePortalKey, IDENTITY_COORDINATE_SPACE_SLOT,
};
mod contract;
pub use components::PortalChain;
pub use components::PortalProjectionCountsRow;
pub use components::PortalRow;
pub use components::PortalViewRow;
pub use contract::{
    chain_set, chain_set_with_poses, chain_set_with_spaces, GpuPortalView, PORTAL_VIEW_SNIPPET,
};
pub mod components;
