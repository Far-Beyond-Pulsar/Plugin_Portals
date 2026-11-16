//! Portals as a feature: what each opening shows (an inline view of the
//! frame, the G-buffer phase's last drawer) and the editor's overlay on the
//! openings (a drawer of the overlay phase). Builds nothing while portals are
//! off (`helio_pass_portal_cull::PortalSettings`).

use helio_feature_api::{pass_node, Feature, FeatureRegistrar};
use helio_frame_api::{FrameNode, Phase};
use helio_pass_portal_cull::portals_enabled;

use crate::{PortalEditorOverlayPass, PortalViewsPass};

/// Portals ([`crate`]).
pub struct PortalFeature;

/// Where the views drawer draws in the G-buffer phase: after every other
/// drawer, so the view's depth it stamps against and the G-buffer the
/// openings' views draw into are complete.
pub const VIEWS_ORDER: i32 = 1000;

impl Feature for PortalFeature {
    fn id(&self) -> &'static str {
        "helio.portals"
    }

    fn register(&self, registrar: &mut FeatureRegistrar<'_>) {
        // What shader hot reload may swap without rebuilding the graph.
        registrar.swappable::<PortalViewsPass>();
        registrar.swappable::<PortalEditorOverlayPass>();

        registrar.node(
            pass_node(
                "portal_views",
                "Portals: Views",
                "Passes: Geometry",
                "What each portal shows: an inline view of the frame through the opening, \
                 drawn by every G-buffer drawer; portals seen through portals recurse.",
                |build| {
                    if portals_enabled(build.env) {
                        let pass = PortalViewsPass::new(build.env.device);
                        build.graph.add_pass(Box::new(pass));
                    }
                    Ok(())
                },
            )
            .drawer(Phase::GBuffer, VIEWS_ORDER),
        );

        registrar.node(
            pass_node(
                "portal_editor_overlay",
                "Overlay: Portal Openings (Editor)",
                "Passes: Overlays",
                "Editor-only checkerboard over each portal's opening.",
                |build| {
                    if portals_enabled(build.env) {
                        let pass = PortalEditorOverlayPass::new(
                            build.env.device,
                            build.env.scene_format(),
                        );
                        build.graph.add_pass(Box::new(pass));
                    }
                    Ok(())
                },
            )
            .with_marker_after()
            .drawer(Phase::Overlay, 20),
        );
    }
}
