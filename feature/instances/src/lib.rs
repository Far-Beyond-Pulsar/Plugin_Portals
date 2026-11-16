//! The portals' drawer: what a portal shows, as an inline view of the frame
//! (`helio_core::view`), drawn by every drawer of the G-buffer phase with its
//! own program.
//!
//! [`PortalViewsPass`] is the G-buffer phase's last drawer, so the view's
//! depth is complete when it runs. For every opening the host published
//! (`helio_pass_portal_cull::PORTAL_OPENINGS`) that faces the view's eye and
//! lies in its frustum, it
//!
//! 1. stamps the opening into a mask, depth-tested against the view's depth
//!    (real occluders in front of a portal keep their pixels; overlapping
//!    openings resolve by depth);
//! 2. makes the inline view's depth from the mask: far inside the opening,
//!    near outside it, so every drawer's own depth test clips its content to
//!    the opening (Valve's stencil portals, without a stencil);
//! 3. asks the frame for the inline view: the view's camera seen through
//!    the opening's view map (`GpuCameraUniforms::through`), clipped at the
//!    destination's plane, drawing into this view's G-buffer. Meshes, foliage,
//!    decals and virtual geometry each cull and draw it with their own
//!    programs, at their cost per view (voxel terrain builds nothing in a
//!    secondary view yet);
//! 4. writes the inline view's depth back into the opening.
//!
//! A portal seen through a portal is this drawer running again inside the
//! inline view's graph, bounded by the frame's view budget
//! (`helio_core::ViewBudget::max_depth`): no chain enumeration, and each
//! level sees only what the level above it shows.

pub mod feature;
pub use feature::PortalFeature;

use std::collections::HashMap;

use glam::{Mat4, Vec4};
use helio_core::{PassContext, PrepareContext, RenderPass, Result as HelioResult};
use helio_pass_portal_cull::{portal_openings, GpuPortalView, PortalOpening};

mod editor_overlay;
pub use editor_overlay::{
    PortalDebugText, PortalEditorOverlayPass, PORTAL_DEBUG_GEOMETRY, PORTAL_DEBUG_TEXTS,
};

/// Openings one view draws through (the nearest first is not needed: the
/// stamp resolves overlaps by depth).
pub const MAX_OPENINGS_PER_VIEW: usize = 64;

/// Frames an opening's view is kept after it was last drawn through.
const KEEP_FRAMES: u64 = 120;

/// An opening's inline view: its depth (the mask, then what it drew) and
/// its view map last frame (its motion).
struct OpeningView {
    depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
    size: (u32, u32),
    merge_group: Option<wgpu::BindGroup>,
    previous: Mat4,
    last_frame: u64,
}

pub struct PortalViewsPass {
    stamp_pipeline: wgpu::RenderPipeline,
    stamp_layout: wgpu::BindGroupLayout,
    stamp_group: helio_core::CachedBindGroup,
    mask_pipeline: wgpu::RenderPipeline,
    mask_layout: wgpu::BindGroupLayout,
    mask_group: Option<wgpu::BindGroup>,
    merge_pipeline: wgpu::RenderPipeline,
    merge_layout: wgpu::BindGroupLayout,
    /// This frame's visible openings' rows (`GpuPortalView`).
    rows: wgpu::Buffer,
    /// `opening + 1` where each visible opening shows (R32Uint).
    mask: Option<(wgpu::Texture, wgpu::TextureView, (u32, u32))>,
    views: HashMap<u64, OpeningView>,
    /// Whether the frame publishes openings (a graph member only then).
    live: bool,
}

const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;

impl PortalViewsPass {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = helio_core::shader::module_with(
            device,
            "PortalViews Shader",
            helio_core::include_wgsl!("../shaders/portal_views.wgsl"),
            &[helio_pass_portal_cull::PORTAL_VIEW_SNIPPET],
        );
        let storage = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let texture =
            |binding: u32, sample_type: wgpu::TextureSampleType| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            };
        let stamp_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("PortalViews Stamp BGL"),
            entries: &[storage(0), storage(1)],
        });
        let mask_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("PortalViews Mask BGL"),
            entries: &[texture(0, wgpu::TextureSampleType::Uint)],
        });
        let merge_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("PortalViews Merge BGL"),
            entries: &[
                texture(0, wgpu::TextureSampleType::Uint),
                texture(1, wgpu::TextureSampleType::Depth),
            ],
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
        let always = wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };
        let stamp_pipeline = pipeline(
            "PortalViews Stamp",
            &stamp_layout,
            "vs_stamp",
            "fs_stamp",
            Some(MASK_FORMAT),
            wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                // Openings order among themselves by depth (two faces of a
                // portal cube seen at once): the nearer stamps. What a stamp
                // writes is overwritten by the merge wherever its view draws.
                depth_write_enabled: Some(true),
                depth_compare: Some(helio_core::depth::NEARER_EQUAL),
                stencil: wgpu::StencilState::default(),
                // The opening is flush with the wall around it: nudge it
                // toward the eye so the coplanar wall never wins its border.
                bias: helio_core::depth::bias_toward_camera(2, 0.0, 0.0),
            },
        );
        let mask_pipeline = pipeline(
            "PortalViews Mask",
            &mask_layout,
            "vs_full",
            "fs_mask",
            None,
            always.clone(),
        );
        let merge_pipeline = pipeline(
            "PortalViews Merge",
            &merge_layout,
            "vs_full",
            "fs_merge",
            None,
            always,
        );
        let rows = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("PortalViews Openings"),
            size: (MAX_OPENINGS_PER_VIEW * std::mem::size_of::<GpuPortalView>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            stamp_pipeline,
            stamp_layout,
            stamp_group: helio_core::CachedBindGroup::default(),
            mask_pipeline,
            mask_layout,
            mask_group: None,
            merge_pipeline,
            merge_layout,
            rows,
            mask: None,
            views: HashMap::new(),
            live: false,
        }
    }

    /// The openings `views`' camera sees: facing it, in its frustum and not
    /// all behind its clip plane.
    fn visible(
        views: &helio_core::ViewContext<'_>,
        openings: &[PortalOpening],
    ) -> Vec<PortalOpening> {
        let camera = views.camera_data;
        let eye = glam::Vec3::from_slice(&camera.position_near[..3]);
        let view_proj = Mat4::from_cols_array(&camera.view_proj);
        let plane = Vec4::from_array(camera.clip_plane);
        let clipped = camera.view_info[1] & helio_core::view::VIEW_CLIPPED != 0;
        openings
            .iter()
            .filter(|opening| opening.faces(eye))
            .filter(|opening| {
                let corners = opening.corners();
                let clip = corners.map(|corner| view_proj * corner.extend(1.0));
                // Outside one frustum plane with every corner: not seen.
                let outside = |test: &dyn Fn(Vec4) -> bool| clip.iter().all(|c| test(*c));
                let culled = outside(&|c| c.x < -c.w)
                    || outside(&|c| c.x > c.w)
                    || outside(&|c| c.y < -c.w)
                    || outside(&|c| c.y > c.w)
                    || outside(&|c| c.w <= 0.0);
                let behind_plane = clipped
                    && corners
                        .iter()
                        .all(|corner| plane.truncate().dot(*corner) + plane.w < 0.0);
                !culled && !behind_plane
            })
            .take(MAX_OPENINGS_PER_VIEW)
            .copied()
            .collect()
    }

    fn ensure_mask(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self.mask.as_ref().is_some_and(|(_, _, s)| *s == size) {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("PortalViews Mask"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: MASK_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.mask_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("PortalViews Mask BG"),
            layout: &self.mask_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        }));
        // Every opening's merge group binds the mask.
        for opening in self.views.values_mut() {
            opening.merge_group = None;
        }
        self.mask = Some((texture, view, size));
    }

    fn opening_view(
        &mut self,
        device: &wgpu::Device,
        key: u64,
        content_to_drawn: Mat4,
        size: (u32, u32),
    ) -> &mut OpeningView {
        let view = self.views.entry(key).or_insert_with(|| {
            let depth = depth_texture(device, size);
            OpeningView {
                depth_view: depth.create_view(&wgpu::TextureViewDescriptor::default()),
                depth,
                size,
                merge_group: None,
                previous: content_to_drawn,
                last_frame: 0,
            }
        });
        if view.size != size {
            view.depth = depth_texture(device, size);
            view.depth_view = view
                .depth
                .create_view(&wgpu::TextureViewDescriptor::default());
            view.size = size;
            view.merge_group = None;
        }
        if view.merge_group.is_none() {
            let (_, mask, _) = self.mask.as_ref().expect("the mask is made first");
            view.merge_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("PortalViews Merge BG"),
                layout: &self.merge_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(mask),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view.depth_view),
                    },
                ],
            }));
        }
        view
    }
}

fn depth_texture(device: &wgpu::Device, size: (u32, u32)) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("PortalViews View Depth"),
        size: wgpu::Extent3d {
            width: size.0.max(1),
            height: size.1.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        // The inline view draws into it, reads it (its Hi-Z copies it) and
        // the merge samples it.
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// A full-screen pass over `depth` drawing `instance` with `pipeline`.
fn full_screen(
    encoder: &mut wgpu::CommandEncoder,
    label: &'static str,
    depth: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    group: &wgpu::BindGroup,
    instance: u32,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, instance..instance + 1);
}

impl RenderPass for PortalViewsPass {
    fn name(&self) -> &'static str {
        "PortalViews"
    }

    /// A member while the frame publishes openings.
    fn is_member(&mut self, scene: &mut helio_core::SceneContent<'_>) -> bool {
        if let Some(buffers) = scene.scene_buffers() {
            self.live = portal_openings(buffers).is_some();
        }
        self.live
    }

    fn renders_views(&self) -> bool {
        true
    }

    fn render_views(&mut self, views: &mut helio_core::ViewContext<'_>) -> HelioResult<()> {
        let Some(openings) = portal_openings(views.scene_buffers) else {
            return Ok(());
        };
        let visible = Self::visible(views, &openings.0);
        if visible.is_empty() || !views.can_render() {
            return Ok(());
        }
        let size = (views.width.max(1), views.height.max(1));
        let rows: Vec<GpuPortalView> = visible.iter().map(|opening| opening.view).collect();
        views
            .queue
            .write_buffer(&self.rows, 0, bytemuck::cast_slice(&rows));
        self.ensure_mask(views.device, size);
        self.stamp_group.get_or_create(
            views.device,
            &wgpu::BindGroupDescriptor {
                label: Some("PortalViews Stamp BG"),
                layout: &self.stamp_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: views.camera.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.rows.as_entire_binding(),
                    },
                ],
            },
        );

        // 1. Where each opening shows, against the view's finished depth.
        {
            let (_, mask, _) = self.mask.as_ref().expect("made above");
            let depth = views.depth;
            let encoder = views.encoder();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("PortalViews Stamp"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: mask,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.stamp_pipeline);
            pass.set_bind_group(0, self.stamp_group.get().expect("made above"), &[]);
            pass.draw(0..6, 0..visible.len() as u32);
        }

        let frame = views.frame_num;
        for (index, opening) in visible.iter().enumerate() {
            let index = index as u32;
            let map = opening.content_to_drawn;
            let (depth_texture, depth_view, previous) = {
                let view = self.opening_view(views.device, opening.key, map, size);
                (
                    view.depth.clone(),
                    view.depth_view.clone(),
                    std::mem::replace(&mut view.previous, map),
                )
            };
            // 2. The inline view's depth: far inside the opening, near
            // outside.
            full_screen(
                views.encoder(),
                "PortalViews Mask",
                &depth_view,
                &self.mask_pipeline,
                self.mask_group.as_ref().expect("made with the mask"),
                index,
            );
            // 3. The view: this view's camera seen through the opening,
            // clipped at its destination, into this G-buffer.
            let camera = views
                .camera_data
                .through(map, previous)
                .with_clip_plane(opening.clip_plane());
            let target = views.target;
            let request = helio_core::ViewRequest {
                key: opening.key,
                kind: helio_core::ViewKind::Inline,
                mirrored: false,
                camera,
                content_to_drawn: map,
                width: size.0,
                height: size.1,
                target,
                depth: &depth_view,
                depth_texture: &depth_texture,
                imports: &helio_frame_api::view::INLINE_TARGETS,
                seeds: &[],
            };
            if !views.render_view(&request)? {
                // Past the frame's budget: the opening stays as the stamp
                // left it (closed).
                break;
            }
            // 4. What it drew, into this view's depth.
            let merge = self.views[&opening.key]
                .merge_group
                .clone()
                .expect("made with the view");
            let depth = views.depth;
            full_screen(
                views.encoder(),
                "PortalViews Merge",
                depth,
                &self.merge_pipeline,
                &merge,
                index,
            );
            if let Some(view) = self.views.get_mut(&opening.key) {
                view.last_frame = frame;
            }
        }
        self.views
            .retain(|_, view| frame.saturating_sub(view.last_frame) <= KEEP_FRAMES);
        Ok(())
    }

    fn prepare(&mut self, _ctx: &PrepareContext) -> HelioResult<()> {
        Ok(())
    }

    fn execute(&mut self, _ctx: &mut PassContext) -> HelioResult<()> {
        // Everything is done around the views (`render_views`).
        Ok(())
    }

    fn render_pass_descriptor<'a>(
        &'a self,
        _target: &'a wgpu::TextureView,
        _depth: &'a wgpu::TextureView,
        _resources: &'a helio_core::ResourceRegistry<'a>,
    ) -> Option<wgpu::RenderPassDescriptor<'a>> {
        None
    }
}
