// What a drawer includes to draw its content through a portal chain
// (`helio_portal_api::PORTAL_CHAIN_SNIPPET`, marker `portal_chain`): the
// rows the portals' chains pass uploads and the tests that clip a content
// point to what the chain shows. A drawer declares its own bindings of
// `array<GpuPortalChain>`, `array<GpuPortalStage>` and the mask
// (`texture_2d<u32>`) and loops over a chain's stages and holes with these.
//
// A chain maps content (in its own world: coordinate space and model
// applied) into the space it is drawn in with one rigid matrix,
// `content_to_drawn` (the portals' maps composed deepest first, on the CPU),
// projected once by the frame's camera. Its first stage is the opening the
// camera sees directly: the aperture mask holds `opening + 1` where it is
// visible. Each deeper stage is an opening seen through the ones before it,
// tested exactly per fragment: the camera's ray, in that opening's frame,
// crosses its rectangle and the point lies beyond it. A chain's holes are
// the openings deeper chains show through: what lies behind one is
// replaced by that chain's content, as the mask replaces the direct view's.

/// One live chain (`helio_portal_api::GpuPortalChain`, 160 bytes).
struct GpuPortalChain {
    /// Content (its own world) to where it is drawn (frame space).
    content_to_drawn:      mat4x4<f32>,
    /// The same map last frame (motion vectors).
    prev_content_to_drawn: mat4x4<f32>,
    /// The chain's stages, outermost first, in `array<GpuPortalStage>`.
    first_stage:           u32,
    stage_count:           u32,
    /// The openings of the deeper chains this one shows, in the same array.
    first_hole:            u32,
    hole_count:            u32,
    /// The outermost opening's id: the aperture mask holds `opening + 1`
    /// where it is visible.
    opening:               u32,
    /// The chain's remap (`helio_frame_api::lighting::VIEW_REMAPS`): its
    /// surfaces carry it in their tag (`gbuffer_remap_tag`).
    remap:                 u32,
    _pad0:                 u32,
    _pad1:                 u32,
}

/// One opening as a chain sees it (`helio_portal_api::GpuPortalStage`, 96
/// bytes).
struct GpuPortalStage {
    /// The chain's content to the opening's own frame (local Z negative on
    /// the camera's side, positive beyond).
    content_to_local: mat4x4<f32>,
    /// The camera in the opening's frame (xyz), w unused.
    eye:              vec4<f32>,
    /// The opening's half extent in its local X/Y (xy), zw unused.
    half_extent:      vec4<f32>,
}

/// The chain's rotation (rigid: normals and tangents turn by it alone).
fn portal_chain_rotation(chain: GpuPortalChain) -> mat3x3<f32> {
    let m = chain.content_to_drawn;
    return mat3x3<f32>(m[0].xyz, m[1].xyz, m[2].xyz);
}

/// Whether the chain moved since last frame (its content moves by itself on
/// screen).
fn portal_chain_moved(chain: GpuPortalChain) -> bool {
    let a = chain.content_to_drawn;
    let b = chain.prev_content_to_drawn;
    return any(a[0] != b[0]) || any(a[1] != b[1]) || any(a[2] != b[2]) || any(a[3] != b[3]);
}

/// Whether content point `p` lies beyond `stage`'s opening, seen through it:
/// the camera's ray to it crosses the opening's rectangle.
fn portal_stage_shows(stage: GpuPortalStage, p: vec3<f32>) -> bool {
    let point = (stage.content_to_local * vec4<f32>(p, 1.0)).xyz;
    let eye = stage.eye.xyz;
    if point.z < 0.0 || eye.z >= 0.0 {
        return false;
    }
    let ray = point - eye;
    let t = -eye.z / ray.z;
    let hit = eye.xy + ray.xy * t;
    return abs(hit.x) <= stage.half_extent.x && abs(hit.y) <= stage.half_extent.y;
}

/// Whether content point `p` lies beyond the chain's outermost opening
/// (`stage`, its first): the aperture mask (`mask`, read at the fragment's
/// pixel) holds that opening and the point is on its far side.
fn portal_outer_shows(chain: GpuPortalChain, stage: GpuPortalStage, p: vec3<f32>, mask: u32) -> bool {
    let point = (stage.content_to_local * vec4<f32>(p, 1.0)).xyz;
    return mask == chain.opening + 1u && point.z >= 0.0;
}

/// Whether content point `p` is behind `hole` (a deeper chain's opening in
/// this chain's world) as the camera sees it through the chain: the deeper
/// chain draws there instead. Points on the opening's plane count as behind
/// it (the wall it is set in).
fn portal_hole_covers(hole: GpuPortalStage, p: vec3<f32>) -> bool {
    let point = (hole.content_to_local * vec4<f32>(p, 1.0)).xyz;
    let eye = hole.eye.xyz;
    let slack = 1.0e-3 * max(1.0, abs(eye.z));
    if point.z < -slack || eye.z >= 0.0 {
        return false;
    }
    let ray = point - eye;
    let t = clamp(-eye.z / ray.z, 0.0, 1.0);
    let hit = eye.xy + ray.xy * t;
    return abs(hit.x) <= hole.half_extent.x && abs(hit.y) <= hole.half_extent.y;
}
