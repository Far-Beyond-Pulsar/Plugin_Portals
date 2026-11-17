//! The portals' public contract: what a portal shows, as other kinds draw it.
//!
//! A portal is not a view. It is a rigid map between places (a transition
//! between coordinate spaces), and what it shows is drawn into the frame's
//! own G-buffer and depth as more occurrences of each drawer's content, with
//! the drawer's own program: no second camera, no extra target, no
//! per-portal pass, no composite. A **chain** is a route through up to a few
//! openings (a portal seen through a portal); its maps compose into one
//! matrix, `content_to_drawn`, so a drawer projects its content once with
//! the frame's camera.
//!
//! - A host publishes the frame's chains as [`PortalChainSet`] under
//!   [`PORTAL_CHAIN_SET`] (the openings it pairs and projects, and every
//!   route through them up to its depth and count settings). Absent or
//!   empty, there is nothing to draw through and no portal pass runs.
//! - The portals' chains pass keeps the routes the camera can see this
//!   frame, uploads them as [`GpuPortalChain`]s with their
//!   [`GpuPortalStage`]s ([`PORTAL_CHAINS`], [`PORTAL_STAGES`],
//!   [`PORTAL_CHAIN_COUNT`]) and publishes each chain's drawn-to-content map
//!   as its remap (`helio_frame_api::lighting::VIEW_REMAPS`), so the
//!   lighting shades a chain's surfaces in their own world.
//! - The portals' mask pass, after the direct drawers ([`MASK_ORDER`]),
//!   stamps each visible outermost opening into [`PORTAL_MASK`] and clears
//!   the depth behind it.
//! - Each drawer then draws its content through the live chains
//!   ([`OCCURRENCE_ORDER`]), clipping with [`PORTAL_CHAIN_SNIPPET`] and
//!   tagging what it draws with the chain's remap; a drawer that culls
//!   against the depth those left draws at [`OCCLUDED_OCCURRENCE_ORDER`].
//!   [`PORTAL_CHAIN_VOLUMES`] bounds what each chain shows, for drawers
//!   that look their content up in a tree.

use glam::{DMat4, Vec2};

/// The frame's portal chains, as a host publishes them ([`PORTAL_CHAIN_SET`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PortalChainSet {
    /// The openings the chains pass through.
    pub openings: Vec<PortalOpening>,
    /// Every route through them, shallowest first.
    pub chains: Vec<PortalRoute>,
}

impl PortalChainSet {
    pub fn is_empty(&self) -> bool {
        self.chains.is_empty()
    }
}

/// One opening (a portal surface seen from its front).
///
/// Its pose and map are f64, relative to the frame's world origin: a route's
/// maps compose in f64 and become f32 once, where the chains are uploaded, so
/// a portal pair kilometres apart (or far from the origin) does not lose the
/// precision the composition needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PortalOpening {
    /// The opening's pose (frame space): local Z negative on the side it is
    /// seen from, positive beyond.
    pub transform: DMat4,
    /// Half its extent in local X/Y.
    pub half_extent: Vec2,
    /// What it shows to where it shows it: the rigid map placing the place
    /// beyond its partner behind this opening.
    pub content_to_drawn: DMat4,
}

/// One route through the openings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortalRoute {
    /// Stable while the route exists: its previous frame's map is kept
    /// under it (motion vectors).
    pub key: u64,
    /// Indices into [`PortalChainSet::openings`], outermost (seen directly)
    /// first.
    pub openings: Vec<u32>,
}

/// The payload a host publishes the frame's chains under.
pub const PORTAL_CHAIN_SET: helio_core::PayloadContract<PortalChainSet> =
    helio_core::PayloadContract::new("portal_chain_set");

/// The chains published in `scene` this frame, if any.
pub fn portal_chain_set(
    scene: &helio_core::SceneBufferProjection,
) -> Option<std::sync::Arc<PortalChainSet>> {
    scene
        .payload(&PORTAL_CHAIN_SET)
        .filter(|set| !set.is_empty())
}

/// One live chain on the GPU (the snippet's `GpuPortalChain`, 160 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuPortalChain {
    pub content_to_drawn: [f32; 16],
    pub prev_content_to_drawn: [f32; 16],
    pub first_stage: u32,
    pub stage_count: u32,
    pub first_hole: u32,
    pub hole_count: u32,
    /// The outermost opening's id ([`PORTAL_MASK`] holds `opening + 1`).
    pub opening: u32,
    /// The chain's remap: its index in [`PORTAL_CHAINS`] and in
    /// `helio_frame_api::lighting::VIEW_REMAPS`.
    pub remap: u32,
    pub _pad: [u32; 2],
}

/// One opening as a chain sees it (the snippet's `GpuPortalStage`, 96
/// bytes): a stage of the chain, or a hole of it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuPortalStage {
    /// The chain's content to the opening's frame.
    pub content_to_local: [f32; 16],
    /// The camera in the opening's frame (xyz).
    pub eye: [f32; 4],
    /// The opening's half extent (xy).
    pub half_extent: [f32; 4],
}

/// What a chain can show, as half-spaces in its content's own world (the
/// snippet's `GpuPortalVolume`, 176 bytes): the camera's frustum mapped back
/// through the chain, the four sides of the pyramid from the camera's place
/// in the content through the chain's deepest opening, and that opening's
/// plane. A drawer culls its content per chain against it (bounds in the
/// content's world); what passes may still be clipped per fragment.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuPortalVolume {
    /// `xyz` the inward normal, `w` the offset: a point `p` is inside where
    /// `dot(xyz, p) + w >= 0`.
    pub planes: [[f32; 4]; PORTAL_VOLUME_PLANES],
    pub plane_count: u32,
    pub _pad: [u32; 3],
}

/// The planes of a [`GpuPortalVolume`].
pub const PORTAL_VOLUME_PLANES: usize = 10;

/// The live chains' volumes: a storage buffer of [`GpuPortalVolume`], a row
/// per row of [`PORTAL_CHAINS`].
pub const PORTAL_CHAIN_VOLUMES: &str = "portal_chain_volumes";

/// The live chains: a storage buffer of [`GpuPortalChain`], published by the
/// portals' chains pass.
pub const PORTAL_CHAINS: &str = "portal_chains";
/// Their stages and holes: a storage buffer of [`GpuPortalStage`].
pub const PORTAL_STAGES: &str = "portal_stages";
/// How many chains are live this frame (the rows of [`PORTAL_CHAINS`]).
pub const PORTAL_CHAIN_COUNT: helio_core::ResourceKey<u32> =
    helio_core::ResourceKey::new("portal_chain_count");

/// A live chain as the CPU sees it: what a drawer with CPU-side work per eye
/// (residency around the camera) needs of each row of [`PORTAL_CHAINS`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveChain {
    /// The route's key ([`PortalRoute::key`]): stable while the route lives.
    pub key: u64,
    /// Openings deep (1: seen directly through one opening).
    pub depth: u32,
    /// The content's world to the drawn (frame) space.
    pub content_to_drawn: glam::Mat4,
    /// The camera in the content's world (frame space there).
    pub content_eye: glam::Vec3,
}

/// This frame's live chains, in the order of [`PORTAL_CHAINS`] (row `i` is
/// chain `i`), shallowest first.
pub const PORTAL_LIVE_CHAINS: helio_core::ResourceKey<&[LiveChain]> =
    helio_core::ResourceKey::new("portal_live_chains");
/// The aperture mask: an `R32Uint` texture at the frame's internal size,
/// `opening + 1` where an outermost opening is visible, `0` elsewhere.
pub const PORTAL_MASK: &str = "portal_mask";
pub const PORTAL_MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;

/// The G-buffer draw order of the portals' mask: after every drawer of the
/// direct view (the meshes open the phase; foliage, virtual geometry and
/// voxel terrain draw at 20 to 40), before the drawers' occurrences.
pub const MASK_ORDER: i32 = 60;
/// The G-buffer draw order of what drawers draw through chains: after the
/// mask, before the screen-space drawers that change what is drawn (decals,
/// 100), which then change a chain's surfaces too.
pub const OCCURRENCE_ORDER: i32 = 70;
/// The G-buffer draw order of occurrences that cull against the depth the
/// others left (a GPU-driven drawer with its own occlusion pyramid of the
/// frame's depth, such as virtual geometry): after [`OCCURRENCE_ORDER`], so
/// what the classic occurrences drew occludes them.
pub const OCCLUDED_OCCURRENCE_ORDER: i32 = 75;

/// The most chains a frame can draw through: one remap each.
pub const MAX_CHAINS: u32 = helio_frame_api::lighting::VIEW_REMAP_CHAINS as u32;

/// The WGSL of [`GpuPortalChain`] and [`GpuPortalStage`] and the tests a
/// drawer clips with (`//!use portal_chain`).
pub const PORTAL_CHAIN_SNIPPET: helio_core::shader::ShaderSnippet =
    helio_core::wgsl_snippet!("//!use portal_chain", "portal_chain.wgsl");

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows match their WGSL: naga's layout of the snippet's structs is
    /// the Rust structs' size, member for member.
    #[test]
    fn rows_match_their_wgsl() {
        assert_eq!(std::mem::size_of::<GpuPortalChain>(), 160);
        assert_eq!(std::mem::size_of::<GpuPortalStage>(), 96);
        assert_eq!(std::mem::size_of::<GpuPortalVolume>(), 176);
        let module = naga::front::wgsl::parse_str(PORTAL_CHAIN_SNIPPET.source).expect("parses");
        let mut layouter = naga::proc::Layouter::default();
        layouter.update(module.to_ctx()).expect("lays out");
        for (name, size) in [
            ("GpuPortalChain", std::mem::size_of::<GpuPortalChain>()),
            ("GpuPortalStage", std::mem::size_of::<GpuPortalStage>()),
            ("GpuPortalVolume", std::mem::size_of::<GpuPortalVolume>()),
        ] {
            let (handle, _) = module
                .types
                .iter()
                .find(|(_, ty)| ty.name.as_deref() == Some(name))
                .expect("declared");
            assert_eq!(layouter[handle].size as usize, size, "{name}");
        }
    }
}
