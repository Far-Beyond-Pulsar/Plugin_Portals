//! The portals' contracts and their CPU projection.
//!
//! - [`PORTAL_OPENINGS`]: the frame's direct openings, each with its view map,
//!   which the portals' views drawer (`helio-pass-portal-instances`) renders
//!   as inline views of the frame (`helio_core::view`).
//! - The projection a host runs on its authored portals: pairs and poses
//!   ([`PortalRow`]), the resolver ([`SubLevelResolver`]) and the bridge
//!   ([`PortalProjectionBridge`]) that turn them into view rows and the
//!   target-to-source maps ([`PortalProjectionFrame`]), and
//!   [`PortalOpenings::from_frame`].
//! - The portal math ([`portal_view_map`], [`PortalPair`]) and the plugin's
//!   settings ([`PortalSettings`]).
//!
//! Every drawer of the frame draws what a portal shows with its own program,
//! from the opening's view; nothing here culls or draws geometry.

pub mod feature;
/// The portal feature's host settings.
pub mod settings;
pub use feature::{portals_enabled, PortalSettings};

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
    portal_openings, GpuPortalView, PortalOpening, PortalOpenings, PORTAL_OPENINGS,
    PORTAL_VIEW_SNIPPET,
};
pub mod components;
