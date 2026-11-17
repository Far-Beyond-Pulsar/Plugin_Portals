//! The projection's output as the frame takes it: the chains a host
//! publishes (`helio_portal_api::PORTAL_CHAIN_SET`), and the opening row the
//! portals' own overlay draws.
//!
//! A host projects its authored portals (`PortalProjectionBridge`) and
//! publishes [`chain_set`] of the result: every opening with its pose,
//! extent and view map, and every route through them (a portal seen through
//! portals) up to the depth and count its settings allow. The portals'
//! chains pass keeps the routes the camera sees each frame; every drawer
//! draws its content through them (`helio_portal_api`).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bytemuck::{Pod, Zeroable};
use glam::{DMat4, Vec2};
use helio_portal_api::{PortalChainSet, PortalOpening, PortalRoute};

use crate::PortalProjectionFrame;
use pulsar_scenedb::Entity;

/// [`GpuPortalView`] in WGSL (`//!use portal_view`): `struct GpuPortalView`,
/// field for field (144 bytes). Read by the portals' mask
/// (`portal_mask.wgsl`) and editor overlay (`portal_editor_overlay.wgsl`);
/// each declares only its own binding of the rows.
pub const PORTAL_VIEW_SNIPPET: helio_core::shader::ShaderSnippet =
    helio_core::wgsl_snippet!("//!use portal_view", "../shaders/portal_view.wgsl");

/// One opening as the portals' own passes draw it (its quad). 144 bytes.
///
/// # WGSL equivalent
/// ```wgsl
/// struct GpuPortalView {
///     transform:         mat4x4<f32>,  // 64 bytes
///     inverse_transform: mat4x4<f32>,  // 64 bytes
///     half_extent:       vec2<f32>,    // 8 bytes
///     coordinate_space:  u32,          // 4 bytes
///     _pad:               u32,          // 4 bytes
/// }
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct GpuPortalView {
    /// Portal-local -> frame space: the opening's own pose. The opening is
    /// local X/Y within `half_extent`; what it shows lies on its local +Z
    /// side.
    pub transform: [f32; 16],
    /// Frame space -> portal-local.
    pub inverse_transform: [f32; 16],
    /// Half-extent of the opening, in its own local X/Y.
    pub half_extent: [f32; 2],
    /// The opening's id (what the aperture mask stamps, plus one).
    pub coordinate_space: u32,
    /// [`crate::PortalViewRow::FLAG_MASK_HIDDEN`] for a row that is no
    /// physical opening.
    pub _pad: u32,
}

impl GpuPortalView {
    /// The quad of `opening`, with id `id`.
    pub fn of(opening: &PortalOpening, id: u32) -> Self {
        Self {
            transform: opening.transform.as_mat4().to_cols_array(),
            inverse_transform: opening.transform.inverse().as_mat4().to_cols_array(),
            half_extent: opening.half_extent.to_array(),
            coordinate_space: id,
            _pad: 0,
        }
    }
}

/// The chains of a projected frame: every view row as an opening (its view
/// map from the frame's coordinate-space table) and every chain as a route,
/// keyed by the stable keys of the rows it passes.
pub fn chain_set(frame: &PortalProjectionFrame) -> PortalChainSet {
    chain_set_with_poses(frame, |_| None)
}

/// [`chain_set`] with the host's f64 poses: `pose(entity)` is a portal's pose
/// relative to the frame's world origin where the host has it. An opening
/// whose portal and partner both have one gets its pose and map composed in
/// f64 (the resolver's rows are f32); one placed by a sublevel actor keeps the
/// resolver's.
pub fn chain_set_with_poses(
    frame: &PortalProjectionFrame,
    pose: impl Fn(Entity) -> Option<DMat4>,
) -> PortalChainSet {
    let mut projections = vec![None; frame.portal_views.len()];
    for (key, &index) in &frame.projection_indices {
        if let Some(slot) = projections.get_mut(index as usize) {
            *slot = Some(key);
        }
    }
    let mut keys = vec![0u64; frame.portal_views.len()];
    for (key, &index) in &frame.projection_indices {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        if let Some(slot) = keys.get_mut(index as usize) {
            *slot = hasher.finish();
        }
    }
    let host_pose = |key: &crate::RuntimePortalKey| {
        key.actor_path
            .is_empty()
            .then(|| pose(key.entity))
            .flatten()
    };
    let openings = frame
        .portal_views
        .iter()
        .zip(&projections)
        .map(|(row, key)| {
            let resolved = (
                glam::Mat4::from_cols_array(&row.transform).as_dmat4(),
                frame
                    .coordinate_spaces
                    .get(row.coordinate_space as usize)
                    .copied()
                    .unwrap_or(glam::Mat4::IDENTITY)
                    .as_dmat4(),
            );
            let (transform, content_to_drawn) = key
                .and_then(|key| Some((host_pose(&key.source)?, host_pose(&key.target)?)))
                .map_or(resolved, |(source, target)| {
                    (
                        source,
                        crate::portal_math::portal_view_map_f64(source, target),
                    )
                });
            PortalOpening {
                transform,
                half_extent: Vec2::from_array(row.half_extent),
                content_to_drawn,
            }
        })
        .collect();
    let mut chains: Vec<PortalRoute> = frame
        .portal_chains
        .iter()
        .filter(|chain| !chain.portals.is_empty())
        .map(|chain| {
            let mut hasher = DefaultHasher::new();
            for &view in &chain.portals {
                keys.get(view as usize)
                    .copied()
                    .unwrap_or(0)
                    .hash(&mut hasher);
            }
            PortalRoute {
                key: hasher.finish(),
                openings: chain.portals.clone(),
            }
        })
        .collect();
    // Shallowest first: a frame past its chain budget drops the deepest.
    chains.sort_by_key(|chain| chain.openings.len());
    PortalChainSet { openings, chains }
}
