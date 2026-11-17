//! Portals as a feature: the chains every drawer draws through (scene
//! prep), the aperture mask after the direct drawers (the G-buffer phase,
//! `helio_portal_api::MASK_ORDER`), and the editor's overlay on the openings
//! (a drawer of the overlay phase). Builds nothing while portals are off
//! (`helio_pass_portal_cull::PortalSettings`); members only while the frame
//! has portal chains.

use helio_feature_api::{pass_node, Feature, FeatureRegistrar};
use helio_frame_api::{FrameNode, Phase, Slot};
use helio_pass_portal_cull::{portal_settings, portals_enabled};

use crate::{PortalChainsPass, PortalEditorOverlayPass, PortalMaskPass};

/// Portals ([`crate`]).
pub struct PortalFeature;

impl Feature for PortalFeature {
    fn id(&self) -> &'static str {
        "helio.portals"
    }

    fn register(&self, registrar: &mut FeatureRegistrar<'_>) {
        // What shader hot reload may swap without rebuilding the graph.
        registrar.swappable::<PortalMaskPass>();
        registrar.swappable::<PortalEditorOverlayPass>();

        registrar.node(
            pass_node(
                "portal_chains",
                "Portals: Chains",
                "Passes: Scene Prep",
                "The routes through portals the camera sees this frame, each composed into one \
                 map, with their openings and remaps: what every drawer draws its content \
                 through.",
                |build| {
                    if portals_enabled(build.env) {
                        let settings = portal_settings(build.env);
                        let pass = PortalChainsPass::new(build.env.device, settings.max_chains);
                        build.graph.add_pass(Box::new(pass));
                    }
                    Ok(())
                },
            )
            .in_slot(Slot::Prepare, -50),
        );

        registrar.node(
            pass_node(
                "portal_mask",
                "Portals: Aperture Mask",
                "Passes: Geometry",
                "Stamps each visible opening into the aperture mask and clears the depth behind \
                 it, after the direct drawers and before what they draw through portals.",
                |build| {
                    if portals_enabled(build.env) {
                        build
                            .graph
                            .add_pass(Box::new(PortalMaskPass::new(build.env.device)));
                    }
                    Ok(())
                },
            )
            .drawer(Phase::GBuffer, helio_portal_api::MASK_ORDER),
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
