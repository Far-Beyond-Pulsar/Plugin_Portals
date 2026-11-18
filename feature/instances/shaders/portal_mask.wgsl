//!use helio_prelude
//!use portal_view
//! The portals' aperture mask (`PortalMaskPass`), after every drawer of the
//! direct view, in two small passes:
//!
//! 1. **Stamp** (`vs_stamp`/`fs_stamp`): each visible outermost opening's
//!    quad, through the camera, depth-tested (read-only) against the frame's
//!    finished depth, writes `opening + 1` into the mask where the opening is
//!    visible: real occluders in front of it keep their pixels.
//! 2. **Clear behind** (`vs_full`/`fs_clear`): where the mask holds an
//!    opening, the depth goes back to the clear value (infinitely far), so
//!    what the chains draw through the opening depth-tests against itself
//!    and not against what lies behind the portal's plane. Where nothing
//!    draws through it, the background shows, as through an open window.

// ── Stamp ────────────────────────────────────────────────────────────────────

@group(0) @binding(0) var<storage, read> cameras: array<Camera, 2>;
@group(0) @binding(1) var<storage, read> openings: array<GpuPortalView>;

const LOCAL_CORNERS: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
);

struct StampOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(flat) id: u32,
    @location(1) depth_zw: vec2<f32>,
}

@vertex
fn vs_stamp(@builtin(vertex_index) vertex_index: u32, @builtin(instance_index) instance_index: u32) -> StampOutput {
    let opening = openings[instance_index];
    let local = LOCAL_CORNERS[vertex_index] * opening.half_extent;
    var out: StampOutput;
    let clip = cameras[0].view_proj * (opening.transform * vec4<f32>(local, 0.0, 1.0));
    // Rasterize the aperture without the near-plane clip. Keep X, Y and W
    // so the projected outline and the eye/side clipping stay intact.
    out.clip_position = vec4<f32>(clip.xy, 0.0, clip.w);
    out.depth_zw = clip.zw;
    // `coordinate_space` carries the opening's id (`GpuPortalView::of`).
    out.id = opening.coordinate_space;
    return out;
}

struct StampFragment {
    @location(0) aperture: vec2<u32>,
    @builtin(frag_depth) depth: f32,
}

@fragment
fn fs_stamp(input: StampOutput) -> StampFragment {
    // Interpolate the original homogeneous depth and divide at the pixel:
    // an oblique opening can have pixels on both sides of the near plane.
    let projected_depth = input.depth_zw.x / input.depth_zw.y;
    if projected_depth < 0.0 {
        discard;
    }
    let depth = min(projected_depth, 1.0);
    return StampFragment(vec2<u32>(input.id + 1u, bitcast<u32>(depth)), depth);
}

// ── Clear behind ─────────────────────────────────────────────────────────────

@group(0) @binding(0) var mask: texture_2d<u32>;

@vertex
fn vs_full(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((vertex_index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vertex_index & 2u) * 2.0 - 1.0;
    // Reversed Z: 0 is the clear value (infinitely far).
    return vec4<f32>(x, -y, 0.0, 1.0);
}

@fragment
fn fs_clear(@builtin(position) position: vec4<f32>) {
    if textureLoad(mask, vec2<i32>(position.xy), 0).r == 0u {
        discard;
    }
}
