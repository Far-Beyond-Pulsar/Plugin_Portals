//! What the portals' drawer reads each frame: the openings and where each
//! looks.
//!
//! A host projects its authored portals (`PortalProjectionBridge`) and
//! publishes the frame's direct openings as [`PORTAL_OPENINGS`]. The portals'
//! views drawer (`helio_pass_portal_instances::PortalViewsPass`) renders each
//! visible opening as an inline view of the frame (`helio_core::view`): the
//! frame's camera seen through the opening's view map, drawn by every drawer
//! of the G-buffer phase into the opening. Recursion (a portal seen through a
//! portal) is the drawer running again inside that view, so the host
//! publishes only the direct openings.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};

use crate::PortalProjectionFrame;

/// [`GpuPortalView`] in WGSL (`//!use portal_view`): `struct GpuPortalView`,
/// field for field (144 bytes). Read by the views drawer's stamp
/// (`portal_stamp.wgsl`) and the editor overlay
/// (`portal_editor_overlay.wgsl`); each declares only its own binding of
/// the rows.
pub const PORTAL_VIEW_SNIPPET: helio_core::shader::ShaderSnippet =
    helio_core::wgsl_snippet!("//!use portal_view", "../shaders/portal_view.wgsl");

/// One opening as the GPU reads it. 144 bytes.
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
    /// Unused by the views drawer (the view map travels with the opening);
    /// kept for the row's layout.
    pub coordinate_space: u32,
    /// [`crate::PortalViewRow::FLAG_MASK_HIDDEN`] for a row that is no
    /// physical opening.
    pub _pad: u32,
}

/// One opening of the frame: where it is and what it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PortalOpening {
    /// Stable while the opening exists (its source and target occurrences):
    /// the frame retains the opening's view, and its history, under it.
    pub key: u64,
    /// The opening's row (pose, extent).
    pub view: GpuPortalView,
    /// The opening's view map (`crate::portal_view_map`): from the space it
    /// shows to the space it is in. The opening's view is the camera seen
    /// through it (`GpuCameraUniforms::through`).
    pub content_to_drawn: Mat4,
}

impl PortalOpening {
    /// The opening's pose.
    pub fn transform(&self) -> Mat4 {
        Mat4::from_cols_array(&self.view.transform)
    }

    /// The plane of the opening in the space it shows, as a view's clip
    /// plane (`GpuCameraUniforms::clip_plane`): what lies between the
    /// opening's far side and the view's eye is not drawn.
    pub fn clip_plane(&self) -> Vec4 {
        let pose = self.transform();
        let normal = pose.z_axis.truncate().normalize_or_zero();
        let plane = normal.extend(-normal.dot(pose.w_axis.truncate()));
        // A plane maps by the transpose of the point map.
        self.content_to_drawn.transpose() * plane
    }

    /// Whether `eye` (frame space) sees the opening from its front (it shows
    /// its far side to eyes on its local -Z side).
    pub fn faces(&self, eye: Vec3) -> bool {
        let local = Mat4::from_cols_array(&self.view.inverse_transform).transform_point3(eye);
        local.z < 0.0
    }

    /// The opening's corners in frame space.
    pub fn corners(&self) -> [Vec3; 4] {
        let pose = self.transform();
        let [hx, hy] = self.view.half_extent;
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, y)| pose.transform_point3(Vec3::new(x * hx, y * hy, 0.0)))
    }
}

/// The frame's direct openings ([`PORTAL_OPENINGS`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PortalOpenings(pub Vec<PortalOpening>);

impl PortalOpenings {
    /// The direct openings of a projected frame: every physical view row
    /// (not a nested one), with its view map from the frame's own
    /// coordinate-space table (`PortalProjectionBridge::build`).
    pub fn from_frame(frame: &PortalProjectionFrame) -> Self {
        let mut keys = vec![0u64; frame.portal_views.len()];
        for (key, &index) in &frame.projection_indices {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            key.hash(&mut hasher);
            if let Some(slot) = keys.get_mut(index as usize) {
                *slot = hasher.finish();
            }
        }
        Self(
            frame
                .portal_views
                .iter()
                .zip(keys)
                .filter(|(row, _)| row._pad & crate::PortalViewRow::FLAG_MASK_HIDDEN == 0)
                .map(|(row, key)| PortalOpening {
                    key,
                    view: GpuPortalView::from(*row),
                    content_to_drawn: frame
                        .coordinate_spaces
                        .get(row.coordinate_space as usize)
                        .copied()
                        .unwrap_or(Mat4::IDENTITY),
                })
                .collect(),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The frame's openings, published by the host that projects its portals
/// every frame. Without it nothing is drawn through a portal.
pub const PORTAL_OPENINGS: helio_core::PayloadContract<PortalOpenings> =
    helio_core::PayloadContract::new("portal_openings");

/// The openings published in `scene` this frame.
pub fn portal_openings(scene: &helio_core::SceneBufferProjection) -> Option<Arc<PortalOpenings>> {
    scene
        .payload(&PORTAL_OPENINGS)
        .filter(|openings| !openings.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opening(content_to_drawn: Mat4) -> PortalOpening {
        let pose = Mat4::from_translation(Vec3::new(0.0, 1.0, -5.0));
        PortalOpening {
            key: 1,
            view: GpuPortalView {
                transform: pose.to_cols_array(),
                inverse_transform: pose.inverse().to_cols_array(),
                half_extent: [1.0, 1.0],
                coordinate_space: 0,
                _pad: 0,
            },
            content_to_drawn,
        }
    }

    #[test]
    fn the_clip_plane_keeps_what_lies_beyond_the_destination() {
        // The opening at z = -5 shows +Z of itself; its content sits 100 m
        // further along +X.
        let map = Mat4::from_translation(Vec3::new(-100.0, 0.0, 0.0));
        let plane = opening(map).clip_plane();
        let side = |p: Vec3| plane.truncate().dot(p) + plane.w;
        // Content beyond the destination (drawn beyond the opening) is kept,
        // content on the eye's side of it is clipped.
        assert!(side(Vec3::new(100.0, 1.0, -2.0)) > 0.0);
        assert!(side(Vec3::new(100.0, 1.0, -8.0)) < 0.0);
        // Through the map, kept content lands beyond the opening.
        assert!(map.transform_point3(Vec3::new(100.0, 1.0, -2.0)).z > -5.0);
    }

    #[test]
    fn an_opening_faces_eyes_on_its_near_side() {
        let opening = opening(Mat4::IDENTITY);
        assert!(opening.faces(Vec3::new(0.0, 1.0, -10.0)));
        assert!(!opening.faces(Vec3::ZERO));
    }
}
