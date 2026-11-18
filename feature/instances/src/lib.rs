//! The portals' passes: what every drawer draws its content through
//! (`helio_portal_api`), and the editor's overlay on the openings.
//!
//! - [`PortalChainsPass`] (scene prep): keeps the routes of the host's
//!   [`helio_portal_api::PortalChainSet`] the camera can see this frame,
//!   composes each into one map, and uploads them with their stages and
//!   holes ([`helio_portal_api::PORTAL_CHAINS`],
//!   [`helio_portal_api::PORTAL_STAGES`]) and each chain's drawn-to-content
//!   map as its remap (`helio_frame_api::lighting::VIEW_REMAPS`).
//! - [`PortalMaskPass`] (the G-buffer phase, after the direct drawers):
//!   stamps each visible outermost opening into the aperture mask
//!   ([`helio_portal_api::PORTAL_MASK`]) and clears the depth behind it.
//!
//! Each drawer then draws its own content again through the chains, with
//! its own program: a portal costs those occurrences and the two passes
//! above, whatever its depth. Lights reach what a chain shows through the
//! lighting's remap (its own world's lights); what it shows casts no shadow
//! into the frame and receives none from it.

pub mod feature;
pub use feature::PortalFeature;

use std::collections::HashMap;

use glam::{DMat4, DVec3, Mat4, Vec3, Vec4};
use helio_core::graph::{ResourceBuilder, ResourceSize};
use helio_core::{PassContext, PrepareContext, RenderPass, Result as HelioResult};
use helio_pass_portal_cull::GpuPortalView;
use helio_portal_api::{
    GpuPortalChain, GpuPortalStage, PortalChainSet, PortalOpening, MAX_CHAINS, PORTAL_CHAINS,
    PORTAL_CHAIN_COUNT, PORTAL_MASK, PORTAL_MASK_FORMAT, PORTAL_STAGES,
};

mod editor_overlay;
pub use editor_overlay::{
    PortalDebugText, PortalEditorOverlayPass, PORTAL_DEBUG_GEOMETRY, PORTAL_DEBUG_TEXTS,
};

/// The outermost openings the mask stamps this frame (`GpuPortalView`
/// rows): the portals' own, from the chains pass to the mask pass.
const MASK_OPENINGS: &str = "portal_mask_openings";
const MASK_OPENING_COUNT: helio_core::ResourceKey<u32> =
    helio_core::ResourceKey::new("portal_mask_opening_count");

/// One route, kept for this frame.
struct LiveChain {
    key: u64,
    openings: Vec<u32>,
    /// Composed in f64 about the frame origin; f32 only once uploaded.
    content_to_drawn: DMat4,
    /// The camera in the content space of each stage's source (before the
    /// stage's map): stage `s`'s opening is expressed there.
    eyes: Vec<DVec3>,
}

/// Whether the quad `corners` (frame space) lies in the frustum of
/// `view_proj`: not every corner outside one clip plane.
fn in_frustum(view_proj: Mat4, corners: &[Vec3; 4]) -> bool {
    let clip = corners.map(|corner| view_proj * corner.extend(1.0));
    let outside = |test: &dyn Fn(Vec4) -> bool| clip.iter().all(|c| test(*c));
    !(outside(&|c| c.x < -c.w)
        || outside(&|c| c.x > c.w)
        || outside(&|c| c.y < -c.w)
        || outside(&|c| c.y > c.w)
        || outside(&|c| c.w <= 0.0))
}

fn corners(opening: &PortalOpening) -> [DVec3; 4] {
    let [hx, hy] = opening.half_extent.as_dvec2().to_array();
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| {
        opening
            .transform
            .transform_point3(DVec3::new(x * hx, y * hy, 0.0))
    })
}

/// The routes of `set` the camera (`eye`, `view_proj`, frame space) can see,
/// shallowest first, at most `cap`: each opening faced from its front and
/// in the frustum where it is drawn, and its shorter prefix kept too (the
/// prefix draws the wall the deeper opening is set in).
fn visible_chains(set: &PortalChainSet, eye: DVec3, view_proj: Mat4, cap: usize) -> Vec<LiveChain> {
    let mut live: Vec<LiveChain> = Vec::new();
    let mut by_route: HashMap<Vec<u32>, usize> = HashMap::new();
    for route in &set.chains {
        if live.len() >= cap {
            break;
        }
        let depth = route.openings.len();
        if depth == 0
            || route
                .openings
                .iter()
                .any(|&i| i as usize >= set.openings.len())
        {
            continue;
        }
        if depth > 1 && !by_route.contains_key(&route.openings[..depth - 1]) {
            continue;
        }
        // Walk outermost first: `outer` maps the current stage's source
        // space into the frame (the stages before it composed).
        let mut outer = DMat4::IDENTITY;
        let mut eyes = Vec::with_capacity(depth);
        let mut seen = true;
        let mut space = set.openings[route.openings[0] as usize].source_space;
        for &index in &route.openings {
            let opening = &set.openings[index as usize];
            if opening.source_space != space {
                seen = false;
                break;
            }
            let eye_here = outer.inverse().transform_point3(eye);
            let local = opening.transform.inverse().transform_point3(eye_here);
            // Drawn corners are where the camera sees them: near the frame
            // origin, f32 for the frustum test.
            let drawn = corners(opening).map(|corner| outer.transform_point3(corner).as_vec3());
            if local.z >= 0.0 || !in_frustum(view_proj, &drawn) {
                seen = false;
                break;
            }
            eyes.push(eye_here);
            outer *= opening.content_to_drawn;
            space = opening.destination_space;
        }
        if !seen {
            continue;
        }
        by_route.insert(route.openings.clone(), live.len());
        live.push(LiveChain {
            key: route.key,
            openings: route.openings.clone(),
            content_to_drawn: outer,
            eyes,
        });
    }
    live
}

/// What `chain` can show (`helio_portal_api::GpuPortalVolume`), in its content's
/// world, composed in f64: the frustum of `view_proj` mapped back through the
/// chain, the pyramid from the camera's place in the content (`eye`, frame
/// space) through the chain's deepest opening, and that opening's plane.
fn chain_volume(
    set: &PortalChainSet,
    chain: &LiveChain,
    eye: DVec3,
    view_proj: Mat4,
) -> helio_portal_api::GpuPortalVolume {
    let mut volume = helio_portal_api::GpuPortalVolume::default();
    let mut planes: Vec<glam::DVec4> = Vec::with_capacity(helio_portal_api::PORTAL_VOLUME_PLANES);
    let normalized = |plane: glam::DVec4| {
        let length = plane.truncate().length();
        (length > 0.0).then(|| plane / length)
    };
    // The frustum's five planes (it has no far one), back into the content.
    let to_content = chain.content_to_drawn.transpose();
    for plane in &helio_core::depth::frustum_planes(view_proj.to_cols_array())[..5] {
        let plane = glam::Vec4::from_array(*plane).as_dvec4();
        if let Some(plane) = normalized(to_content * plane) {
            planes.push(plane);
        }
    }
    // The deepest opening, in the content: what lies beyond it, seen from the
    // camera's place there.
    let deepest = &set.openings[*chain.openings.last().expect("a route") as usize];
    let local_to_content = (deepest.transform.inverse() * deepest.content_to_drawn).inverse();
    let [hx, hy] = deepest.half_extent.as_dvec2().to_array();
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .map(|(x, y)| local_to_content.transform_point3(DVec3::new(x * hx, y * hy, 0.0)));
    let center = local_to_content.transform_point3(DVec3::ZERO);
    let eye_in_content = chain.content_to_drawn.inverse().transform_point3(eye);
    let inside = eye_in_content + 2.0 * (center - eye_in_content);
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        let normal = (a - eye_in_content).cross(b - eye_in_content);
        let normal = if normal.dot(inside - eye_in_content) < 0.0 {
            -normal
        } else {
            normal
        };
        if let Some(plane) = normalized(normal.extend(-normal.dot(eye_in_content))) {
            planes.push(plane);
        }
    }
    let beyond = local_to_content
        .transform_vector3(DVec3::Z)
        .normalize_or_zero();
    if let Some(plane) = normalized(beyond.extend(-beyond.dot(center))) {
        planes.push(plane);
    }
    for (slot, plane) in volume.planes.iter_mut().zip(&planes) {
        *slot = plane.as_vec4().to_array();
    }
    volume.plane_count = planes.len().min(helio_portal_api::PORTAL_VOLUME_PLANES) as u32;
    volume
}

fn storage_buffer(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(16).next_power_of_two(),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Grows `buffer` to hold `bytes`.
fn fit(device: &wgpu::Device, buffer: &mut wgpu::Buffer, bytes: u64, label: &str) {
    if buffer.size() < bytes {
        *buffer = storage_buffer(device, label, bytes);
    }
}

/// Keeps the routes the camera sees and uploads them ([`crate`]).
pub struct PortalChainsPass {
    chains: wgpu::Buffer,
    stages: wgpu::Buffer,
    /// [`helio_portal_api::PORTAL_CHAIN_VOLUMES`].
    volumes: wgpu::Buffer,
    remaps: wgpu::Buffer,
    remap_spaces: wgpu::Buffer,
    openings: wgpu::Buffer,
    chain_count: u32,
    opening_count: u32,
    /// Each live route's map last frame, by key (motion vectors).
    previous: HashMap<u64, DMat4>,
    /// The most routes a frame draws (`PortalSettings::max_chains`).
    cap: usize,
    /// Whether the frame publishes chains (a graph member only then).
    live: bool,
    /// The live chains for the CPU ([`helio_portal_api::PORTAL_LIVE_CHAINS`]).
    live_chains: Vec<helio_portal_api::LiveChain>,
}

impl PortalChainsPass {
    pub fn new(device: &wgpu::Device, max_chains: usize) -> Self {
        Self {
            chains: storage_buffer(device, "Portal Chains", 160),
            stages: storage_buffer(device, "Portal Stages", 96),
            volumes: storage_buffer(device, "Portal Chain Volumes", 176),
            remaps: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Portal Remaps"),
                size: helio_frame_api::lighting::VIEW_REMAPS_BYTES,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            remap_spaces: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Portal Remap Spaces"),
                size: helio_frame_api::lighting::VIEW_REMAP_SPACES_BYTES,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            openings: storage_buffer(device, "Portal Mask Openings", 144),
            chain_count: 0,
            opening_count: 0,
            previous: HashMap::new(),
            cap: max_chains.clamp(1, MAX_CHAINS as usize),
            live: false,
            live_chains: Vec::new(),
        }
    }

    /// This frame's live chains.
    pub fn chain_count(&self) -> u32 {
        self.chain_count
    }
}

impl RenderPass for PortalChainsPass {
    fn name(&self) -> &'static str {
        "PortalChains"
    }

    fn is_member(&mut self, scene: &mut helio_core::SceneContent<'_>) -> bool {
        if let Some(buffers) = scene.scene_buffers() {
            self.live = helio_portal_api::portal_chain_set(buffers).is_some();
        }
        self.live
    }

    fn writes(&self) -> &'static [&'static str] {
        &[
            PORTAL_CHAINS,
            PORTAL_STAGES,
            helio_portal_api::PORTAL_CHAIN_VOLUMES,
            helio_frame_api::lighting::VIEW_REMAPS,
            helio_frame_api::lighting::VIEW_REMAP_SPACES,
        ]
    }

    fn render_pass_descriptor<'a>(
        &'a self,
        _target: &'a wgpu::TextureView,
        _depth: &'a wgpu::TextureView,
        _resources: &'a helio_core::ResourceRegistry<'a>,
    ) -> Option<wgpu::RenderPassDescriptor<'a>> {
        None
    }

    fn prepare(&mut self, ctx: &PrepareContext) -> HelioResult<()> {
        let Some(set) = helio_portal_api::portal_chain_set(ctx.scene_buffers) else {
            self.chain_count = 0;
            self.opening_count = 0;
            self.live_chains.clear();
            return Ok(());
        };
        let eye = Vec3::from_slice(&ctx.camera_data.position_near[..3]).as_dvec3();
        let view_proj = Mat4::from_cols_array(&ctx.camera_data.view_proj);
        let live = visible_chains(&set, eye, view_proj, self.cap);

        let mut chains = Vec::with_capacity(live.len());
        let volumes: Vec<_> = live
            .iter()
            .map(|chain| chain_volume(&set, chain, eye, view_proj))
            .collect();
        let mut stages = Vec::new();
        let mut remaps = Vec::with_capacity(live.len());
        let mut remap_spaces = Vec::with_capacity(live.len());
        let mut previous = HashMap::with_capacity(live.len());
        let mut openings: Vec<GpuPortalView> = Vec::new();
        let mut stamped: HashMap<u32, ()> = HashMap::new();
        for (remap, chain) in live.iter().enumerate() {
            let first_stage = stages.len() as u32;
            // Stage `s` sees content through the maps from `s` on.
            let mut inner = DMat4::IDENTITY;
            let mut stage_rows = Vec::with_capacity(chain.openings.len());
            for (s, &index) in chain.openings.iter().enumerate().rev() {
                let opening = &set.openings[index as usize];
                inner = opening.content_to_drawn * inner;
                let to_local = opening.transform.inverse();
                let eye = to_local.transform_point3(chain.eyes[s]);
                stage_rows.push(GpuPortalStage {
                    content_to_local: (to_local * inner).as_mat4().to_cols_array(),
                    eye: eye.as_vec3().extend(0.0).to_array(),
                    half_extent: opening.half_extent.extend(0.0).extend(0.0).to_array(),
                });
            }
            stage_rows.reverse();
            stages.extend(stage_rows);
            // The holes: the next opening of each kept route one deeper,
            // in this chain's content space (where it is set).
            let first_hole = stages.len() as u32;
            let eye_in_content = chain.content_to_drawn.inverse().transform_point3(eye);
            for deeper in live.iter().filter(|other| {
                other.openings.len() == chain.openings.len() + 1
                    && other.openings.starts_with(&chain.openings)
            }) {
                let opening = &set.openings[*deeper.openings.last().expect("deeper") as usize];
                let to_local = opening.transform.inverse();
                stages.push(GpuPortalStage {
                    content_to_local: to_local.as_mat4().to_cols_array(),
                    eye: to_local
                        .transform_point3(eye_in_content)
                        .as_vec3()
                        .extend(0.0)
                        .to_array(),
                    half_extent: opening.half_extent.extend(0.0).extend(0.0).to_array(),
                });
            }
            let hole_count = stages.len() as u32 - first_hole;
            let outer = chain.openings[0];
            if stamped.insert(outer, ()).is_none() {
                openings.push(GpuPortalView::of(&set.openings[outer as usize], outer));
            }
            let prev = self
                .previous
                .get(&chain.key)
                .copied()
                .unwrap_or(chain.content_to_drawn);
            previous.insert(chain.key, chain.content_to_drawn);
            let destination = *chain.openings.last().expect("visible route is nonempty");
            let destination_space = set.openings[destination as usize].destination_space;
            chains.push(GpuPortalChain {
                content_to_drawn: chain.content_to_drawn.as_mat4().to_cols_array(),
                prev_content_to_drawn: prev.as_mat4().to_cols_array(),
                first_stage,
                stage_count: chain.openings.len() as u32,
                first_hole,
                hole_count,
                opening: outer,
                remap: remap as u32,
                source_space: set.openings[outer as usize].source_space,
                destination_space,
            });
            remaps.push(chain.content_to_drawn.inverse().as_mat4().to_cols_array());
            remap_spaces.push(destination_space);
        }
        self.previous = previous;
        self.live_chains = live
            .iter()
            .zip(&chains)
            .map(|(chain, gpu)| helio_portal_api::LiveChain {
                key: chain.key,
                depth: chain.openings.len() as u32,
                destination_space: gpu.destination_space,
                content_to_drawn: chain.content_to_drawn.as_mat4(),
                content_eye: chain
                    .content_to_drawn
                    .inverse()
                    .transform_point3(eye)
                    .as_vec3(),
            })
            .collect();
        self.chain_count = chains.len() as u32;
        self.opening_count = openings.len() as u32;
        fit(
            ctx.device,
            &mut self.chains,
            (chains.len() * 160) as u64,
            "Portal Chains",
        );
        fit(
            ctx.device,
            &mut self.stages,
            (stages.len() * 96) as u64,
            "Portal Stages",
        );
        fit(
            ctx.device,
            &mut self.volumes,
            (volumes.len() * 176) as u64,
            "Portal Chain Volumes",
        );
        fit(
            ctx.device,
            &mut self.openings,
            (openings.len() * 144) as u64,
            "Portal Mask Openings",
        );
        if !chains.is_empty() {
            ctx.write_buffer(&self.chains, 0, bytemuck::cast_slice(&chains));
            ctx.write_buffer(&self.stages, 0, bytemuck::cast_slice(&stages));
            ctx.write_buffer(&self.volumes, 0, bytemuck::cast_slice(&volumes));
            ctx.write_buffer(&self.remaps, 0, bytemuck::cast_slice(&remaps));
            ctx.write_buffer(&self.remap_spaces, 0, bytemuck::cast_slice(&remap_spaces));
            ctx.write_buffer(&self.openings, 0, bytemuck::cast_slice(&openings));
        }
        Ok(())
    }

    fn execute(&mut self, _ctx: &mut PassContext) -> HelioResult<()> {
        Ok(())
    }

    fn publish<'a>(&self, frame: &mut helio_core::ResourceRegistry<'a>) {
        // SAFETY: the buffers are this pass's, owned by the graph past the
        // frame's registry, as every pass's publication.
        let own =
            |buffer: &wgpu::Buffer| -> &'a wgpu::Buffer { unsafe { std::mem::transmute(buffer) } };
        let own_chains =
            |chains: &[helio_portal_api::LiveChain]| -> &'a [helio_portal_api::LiveChain] {
                unsafe { std::mem::transmute(chains) }
            };
        frame.write(PORTAL_CHAIN_COUNT, self.chain_count, "PortalChains");
        frame.write(MASK_OPENING_COUNT, self.opening_count, "PortalChains");
        frame.write(
            helio_portal_api::PORTAL_LIVE_CHAINS,
            own_chains(&self.live_chains),
            "PortalChains",
        );
        if self.chain_count == 0 {
            return;
        }
        for (name, buffer) in [
            (PORTAL_CHAINS, own(&self.chains)),
            (PORTAL_STAGES, own(&self.stages)),
            (helio_portal_api::PORTAL_CHAIN_VOLUMES, own(&self.volumes)),
            (helio_frame_api::lighting::VIEW_REMAPS, own(&self.remaps)),
            (
                helio_frame_api::lighting::VIEW_REMAP_SPACES,
                own(&self.remap_spaces),
            ),
            (MASK_OPENINGS, own(&self.openings)),
        ] {
            frame.write_buffer_binding(name, buffer, "PortalChains");
            frame.write(
                helio_core::ResourceKey::<&wgpu::Buffer>::new(name),
                buffer,
                "PortalChains",
            );
        }
    }
}

/// Stamps the visible outermost openings and clears the depth behind them
/// ([`crate`]).
pub struct PortalMaskPass {
    stamp_pipeline: wgpu::RenderPipeline,
    stamp_layout: wgpu::BindGroupLayout,
    stamp_group: helio_core::CachedBindGroup,
    clear_pipeline: wgpu::RenderPipeline,
    clear_layout: wgpu::BindGroupLayout,
    clear_group: helio_core::CachedBindGroup,
    live: bool,
}

impl PortalMaskPass {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = helio_core::shader::module_with(
            device,
            "PortalMask Shader",
            helio_core::include_wgsl!("../shaders/portal_mask.wgsl"),
            &[helio_pass_portal_cull::PORTAL_VIEW_SNIPPET],
        );
        let storage = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let stamp_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("PortalMask Stamp BGL"),
            entries: &[storage(0), storage(1)],
        });
        let clear_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("PortalMask Clear BGL"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline = |label: &'static str,
                        layout: &wgpu::BindGroupLayout,
                        vertex: &'static str,
                        fragment: &'static str,
                        color: Option<wgpu::TextureFormat>,
                        depth: wgpu::DepthStencilState| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            let targets = [color.map(|format| wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })];
            helio_core::shader::create_render_pipeline(
                device,
                wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some(vertex),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some(fragment),
                        compilation_options: Default::default(),
                        targets: if color.is_some() { &targets } else { &[] },
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: Some(depth),
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                },
            )
        };
        let stamp_pipeline = pipeline(
            "PortalMask Stamp",
            &stamp_layout,
            "vs_stamp",
            "fs_stamp",
            Some(PORTAL_MASK_FORMAT),
            wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                // Read-only: the clear below rewrites the depth where it
                // stamped. Openings overlapping on screen stamp in turn; the
                // nearer one's chains draw over the farther's by depth.
                depth_write_enabled: Some(false),
                depth_compare: Some(helio_core::depth::NEARER_EQUAL),
                stencil: wgpu::StencilState::default(),
                // The opening is flush with the wall it is set in: nudged
                // toward the eye so the coplanar wall never wins its border.
                bias: helio_core::depth::bias_toward_camera(2, 0.0, 0.0),
            },
        );
        let clear_pipeline = pipeline(
            "PortalMask Clear Behind",
            &clear_layout,
            "vs_full",
            "fs_clear",
            None,
            wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            },
        );
        Self {
            stamp_pipeline,
            stamp_layout,
            stamp_group: helio_core::CachedBindGroup::default(),
            clear_pipeline,
            clear_layout,
            clear_group: helio_core::CachedBindGroup::default(),
            live: false,
        }
    }
}

impl RenderPass for PortalMaskPass {
    fn name(&self) -> &'static str {
        "PortalMask"
    }

    fn is_member(&mut self, scene: &mut helio_core::SceneContent<'_>) -> bool {
        if let Some(buffers) = scene.scene_buffers() {
            self.live = helio_portal_api::portal_chain_set(buffers).is_some();
        }
        self.live
    }

    fn declare_resources(&self, builder: &mut ResourceBuilder) {
        builder.write_color_raw(PORTAL_MASK, PORTAL_MASK_FORMAT, ResourceSize::MatchSurface);
        builder.with_extra_usage(wgpu::TextureUsages::TEXTURE_BINDING);
    }

    fn render_pass_descriptor<'a>(
        &'a self,
        _target: &'a wgpu::TextureView,
        _depth: &'a wgpu::TextureView,
        _resources: &'a helio_core::ResourceRegistry<'a>,
    ) -> Option<wgpu::RenderPassDescriptor<'a>> {
        None
    }

    fn prepare(&mut self, _ctx: &PrepareContext) -> HelioResult<()> {
        Ok(())
    }

    fn execute(&mut self, ctx: &mut PassContext) -> HelioResult<()> {
        let count = ctx.registry.get(MASK_OPENING_COUNT).unwrap_or(0);
        let Some(mask) = ctx.resource_pool.get_view(PORTAL_MASK) else {
            return Ok(());
        };
        let mask = mask.clone();
        let openings = ctx.registry.buffer_binding(MASK_OPENINGS);
        let stamp = match (count, openings) {
            (0, _) | (_, None) => None,
            (_, Some(openings)) => Some(
                self.stamp_group
                    .get_or_create(
                        ctx.device,
                        &wgpu::BindGroupDescriptor {
                            label: Some("PortalMask Stamp BG"),
                            layout: &self.stamp_layout,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: ctx.camera.as_entire_binding(),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: openings.as_entire_binding(),
                                },
                            ],
                        },
                    )
                    .clone(),
            ),
        };
        let clear = self
            .clear_group
            .get_or_create(
                ctx.device,
                &wgpu::BindGroupDescriptor {
                    label: Some("PortalMask Clear BG"),
                    layout: &self.clear_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&mask),
                    }],
                },
            )
            .clone();
        // The mask is cleared every frame, stamped or not, so the drawers'
        // occurrences never read a stale opening.
        {
            let attachments = [Some(wgpu::RenderPassColorAttachment {
                view: &mask,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })];
            let desc = wgpu::RenderPassDescriptor {
                label: Some("PortalMask Stamp"),
                color_attachments: &attachments,
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: ctx.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            };
            let mut pass = ctx.begin_render_pass(&desc);
            if let Some(stamp) = stamp.as_ref() {
                pass.set_pipeline(&self.stamp_pipeline);
                pass.set_bind_group(0, stamp, &[]);
                pass.draw(0..6, 0..count);
            }
        }
        if stamp.is_some() {
            let desc = wgpu::RenderPassDescriptor {
                label: Some("PortalMask Clear Behind"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: ctx.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            };
            let mut pass = ctx.begin_render_pass(&desc);
            pass.set_pipeline(&self.clear_pipeline);
            pass.set_bind_group(0, &clear, &[]);
            pass.draw(0..3, 0..1);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;
    use helio_portal_api::PortalRoute;

    /// A pair: A at the origin turned to face +Z (an opening's local -Z is
    /// the side it is seen from), B 100 m along X facing -Z; A shows what is
    /// beside B, behind A.
    fn pair() -> PortalChainSet {
        let a = DMat4::from_rotation_y(std::f64::consts::PI);
        let b = DMat4::from_translation(DVec3::new(100.0, 0.0, 0.0));
        let map_a = a * DMat4::from_rotation_y(std::f64::consts::PI) * b.inverse();
        let opening = |transform: DMat4, content_to_drawn: DMat4| PortalOpening {
            transform,
            half_extent: Vec2::splat(1.0),
            content_to_drawn,
            source_space: 0,
            destination_space: 0,
        };
        PortalChainSet {
            openings: vec![opening(a, map_a), opening(b, map_a.inverse())],
            chains: vec![
                PortalRoute {
                    key: 1,
                    openings: vec![0],
                },
                PortalRoute {
                    key: 2,
                    openings: vec![1],
                },
                PortalRoute {
                    key: 3,
                    openings: vec![0, 1],
                },
            ],
        }
    }

    fn camera(eye: Vec3, target: Vec3) -> Mat4 {
        helio_core::depth::perspective(1.0, 1.0, 0.1)
            * glam::camera::rh::view::look_at_mat4(eye, target, Vec3::Y)
    }

    /// The camera in front of A sees A's route; B, behind it and facing away,
    /// is not drawn through, nor any route through B.
    #[test]
    fn only_routes_through_faced_openings_in_view_are_kept() {
        let set = pair();
        let eye = Vec3::new(0.0, 0.0, 5.0);
        let live = visible_chains(&set, eye.as_dvec3(), camera(eye, Vec3::ZERO), 300);
        assert_eq!(live.iter().map(|c| c.key).collect::<Vec<_>>(), [1]);
        // A route's map puts what is beside B behind A.
        let beside_b = DVec3::new(100.0, 0.0, -2.0);
        let drawn = live[0].content_to_drawn.transform_point3(beside_b);
        assert!(drawn.distance(DVec3::new(0.0, 0.0, -2.0)) < 1e-9, "{drawn}");
    }

    #[test]
    fn nested_routes_require_matching_coordinate_spaces() {
        let mut set = pair();
        set.openings[0].content_to_drawn = DMat4::IDENTITY;
        set.openings[0].destination_space = 1;
        set.openings[1] = PortalOpening {
            source_space: 1,
            destination_space: 2,
            ..set.openings[0]
        };
        set.chains.remove(1); // Test only the route entering opening 0.
        let eye = Vec3::new(0.0, 0.0, 5.0);
        let vp = camera(eye, Vec3::ZERO);
        assert_eq!(visible_chains(&set, eye.as_dvec3(), vp, 300).len(), 2);

        // The matrices stay identical. Only the next aperture's space changes.
        set.openings[1].source_space = 2;
        assert_eq!(visible_chains(&set, eye.as_dvec3(), vp, 300).len(), 1);
    }

    /// A pair 6000 km apart (a planet's span) composes in f64: content right
    /// beside the far portal lands at the near one to well under a millimetre,
    /// where an f32 composition misses by centimetres.
    #[test]
    fn far_apart_portals_compose_in_f64() {
        let near = DMat4::from_rotation_y(std::f64::consts::PI);
        let far = DMat4::from_translation(DVec3::new(6.0e6, 1234.5, -3.0e6));
        let map = near * DMat4::from_rotation_y(std::f64::consts::PI) * far.inverse();
        let set = PortalChainSet {
            openings: vec![PortalOpening {
                transform: near,
                half_extent: Vec2::splat(1.0),
                content_to_drawn: map,
                source_space: 0,
                destination_space: 0,
            }],
            chains: vec![PortalRoute {
                key: 1,
                openings: vec![0],
            }],
        };
        let eye = Vec3::new(0.0, 0.0, 5.0);
        let live = visible_chains(&set, eye.as_dvec3(), camera(eye, Vec3::ZERO), 300);
        let beside_far = DVec3::new(6.0e6, 1234.5, -3.0e6 - 2.0);
        let drawn = live[0].content_to_drawn.transform_point3(beside_far);
        assert!(drawn.distance(DVec3::new(0.0, 0.0, -2.0)) < 1e-6, "{drawn}");
    }

    /// A chain's volume holds content beyond its opening, seen through it, and
    /// not content beside the far portal on the near side, nor behind the
    /// camera's view.
    #[test]
    fn a_chain_volume_holds_what_its_opening_shows() {
        let set = pair();
        let eye = Vec3::new(0.0, 0.0, 5.0);
        let live = visible_chains(&set, eye.as_dvec3(), camera(eye, Vec3::ZERO), 300);
        let volume = chain_volume(&set, &live[0], eye.as_dvec3(), camera(eye, Vec3::ZERO));
        assert_eq!(
            volume.plane_count as usize,
            helio_portal_api::PORTAL_VOLUME_PLANES
        );
        let holds = |p: DVec3| {
            volume.planes[..volume.plane_count as usize]
                .iter()
                .all(|plane| {
                    glam::Vec4::from_array(*plane).truncate().dot(p.as_vec3()) + plane[3] >= -1e-4
                })
        };
        // B's far side (what A shows: drawn at z < 0, behind A).
        assert!(holds(DVec3::new(100.0, 0.0, -2.0)));
        // On the camera's side of B: drawn in front of A, not through it.
        assert!(!holds(DVec3::new(100.0, 0.0, 2.0)));
        // Beyond B but far outside A's opening.
        assert!(!holds(DVec3::new(100.0, 40.0, -2.0)));
    }

    /// A route past the budget is dropped, shallowest kept first.
    #[test]
    fn the_budget_keeps_the_shallowest_routes() {
        let set = pair();
        let eye = Vec3::new(0.0, 0.0, 5.0);
        let live = visible_chains(&set, eye.as_dvec3(), camera(eye, Vec3::ZERO), 1);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].openings.len(), 1);
    }
}
