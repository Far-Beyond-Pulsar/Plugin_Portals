//!use helio_prelude
//!use portal_view
//! The portals' views drawer (`PortalViewsPass`): what it draws around each
//! portal's inline view (`helio_core::view`). Three small passes per view
//! of the frame, in the order the drawer records them:
//!
//! 1. **Stamp** (`vs_stamp`/`fs_stamp`): every visible opening's quad,
//!    through the view's camera, depth-tested (read-only) against the view's
//!    finished depth, writes `opening + 1` into the mask where the opening is
//!    visible: real occluders in front of it keep their pixels. What the
//!    view's own clip plane cuts (a portal behind the destination of the
//!    portal this view looks through) stamps nothing.
//! 2. **Mask** (`vs_full`/`fs_mask`): for one opening, the depth its inline
//!    view draws into: far (0, reversed Z) where the mask holds the opening,
//!    near (1) everywhere else. Every drawer's own depth test then keeps its
//!    content inside the opening and lets nearer content win there: the
//!    stencil portals' clip, with no drawer knowing it.
//! 3. **Merge** (`vs_full`/`fs_merge`): after the inline view drew, its
//!    depth inside the opening becomes the view's: what the portal shows
//!    occludes, reprojects and is lit like the rest of the frame. Where the
//!    view drew nothing the depth stays far: the background shows, as through
//!    an open window.

// ── Stamp ────────────────────────────────────────────────────────────────────

@group(0) @binding(0) var<storage, read> cameras: array<Camera, 2>;
@group(0) @binding(1) var<storage, read> openings: array<GpuPortalView>;

// Two triangles covering [-1,1]^2 in the opening's own local X/Y, scaled by
// half_extent in the vertex shader.
const LOCAL_CORNERS: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
);

struct StampOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) @interpolate(flat) opening: u32,
}

@vertex
fn vs_stamp(@builtin(vertex_index) vertex_index: u32, @builtin(instance_index) instance_index: u32) -> StampOutput {
    let opening = openings[instance_index];
    let local = LOCAL_CORNERS[vertex_index] * opening.half_extent;
    let world_pos = opening.transform * vec4<f32>(local, 0.0, 1.0);
    var out: StampOutput;
    out.clip_position = cameras[0].view_proj * world_pos;
    out.world_position = world_pos.xyz;
    out.opening = instance_index;
    return out;
}

@fragment
fn fs_stamp(input: StampOutput) -> @location(0) u32 {
    if helio_view_clipped(cameras[0], input.world_position) {
        discard;
    }
    return input.opening + 1u;
}

// ── Mask and merge ───────────────────────────────────────────────────────────

@group(0) @binding(0) var mask: texture_2d<u32>;
@group(0) @binding(1) var view_depth: texture_depth_2d;

struct FullOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) opening: u32,
}

// A full-screen triangle; the instance is the opening (`opening + 1` in the
// mask).
@vertex
fn vs_full(@builtin(vertex_index) vertex_index: u32, @builtin(instance_index) instance_index: u32) -> FullOutput {
    let x = f32((vertex_index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vertex_index & 2u) * 2.0 - 1.0;
    var out: FullOutput;
    out.position = vec4<f32>(x, -y, 0.0, 1.0);
    out.opening = instance_index;
    return out;
}

struct DepthOutput {
    @builtin(frag_depth) depth: f32,
}

@fragment
fn fs_mask(input: FullOutput) -> DepthOutput {
    let inside = textureLoad(mask, vec2<i32>(input.position.xy), 0).r == input.opening + 1u;
    // Reversed Z: 0 is infinitely far (content draws over it), 1 the near
    // plane (no content is nearer).
    return DepthOutput(select(1.0, 0.0, inside));
}

@fragment
fn fs_merge(input: FullOutput) -> DepthOutput {
    let pixel = vec2<i32>(input.position.xy);
    if textureLoad(mask, pixel, 0).r != input.opening + 1u {
        discard;
    }
    return DepthOutput(textureLoad(view_depth, pixel, 0));
}
