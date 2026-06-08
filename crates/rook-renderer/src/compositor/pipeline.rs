//! GPU compositor pipeline — renders FrameDescriptors using wgpu.
//!
//! Takes the type definitions from `frame.rs` and the WGSL shaders from
//! `layer.wgsl`/`blend.wgsl`/`mask.wgsl` and composites layers into
//! a single output texture.
//!
//! Vendored and adapted from koughen/Editor (MIT).

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::compositor::{BlendMode, FrameDescriptor, FrameItemDescriptor, LayerDescriptor};

// ── GPU uniform buffer types ──────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LayerUniformBuffer {
    resolution: [f32; 2],
    center: [f32; 2],
    size: [f32; 2],
    rotation_radians: f32,
    opacity: f32,
    flip_x: f32,
    flip_y: f32,
    _padding: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlendUniformBuffer {
    blend_mode: u32,
    _padding: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MaskUniformBuffer {
    inverted: f32,
    _padding: [f32; 3],
}

// ── Texture store (ID → wgpu::Texture) ────────────────────────────────────

/// Stores GPU textures keyed by ID string.
pub struct TextureStore {
    textures: HashMap<String, wgpu::Texture>,
}

impl TextureStore {
    pub fn new() -> Self {
        Self { textures: HashMap::new() }
    }

    pub fn upsert(&mut self, id: String, texture: wgpu::Texture) {
        self.textures.insert(id, texture);
    }

    pub fn get(&self, id: &str) -> Option<&wgpu::Texture> {
        self.textures.get(id)
    }

    pub fn remove(&mut self, id: &str) -> Option<wgpu::Texture> {
        self.textures.remove(id)
    }

    /// Release all textures.
    pub fn clear(&mut self) {
        self.textures.clear();
    }
}

// ── Compositor ────────────────────────────────────────────────────────────

/// Error type for compositor operations.
#[derive(Debug, thiserror::Error)]
pub enum CompositorError {
    #[error("Texture '{texture_id}' not found in store")]
    MissingTexture { texture_id: String },
    #[error("WGPU error: {0}")]
    Wgpu(#[from] wgpu::Error),
}

/// Full-screen quad vertex data (same as existing renderer).
const FULLSCREEN_VERTICES: [[f32; 2]; 6] = [
    [-1.0, -1.0],
    [1.0, -1.0],
    [-1.0, 1.0],
    [-1.0, 1.0],
    [1.0, -1.0],
    [1.0, 1.0],
];

/// The GPU compositor. Holds pipeline state, texture store, and samplers.
pub struct Compositor {
    pub textures: TextureStore,

    // Bind group layouts
    layer_uniform_bgl: wgpu::BindGroupLayout,
    blend_uniform_bgl: wgpu::BindGroupLayout,
    mask_uniform_bgl: wgpu::BindGroupLayout,

    // Render pipelines
    layer_pipeline: wgpu::RenderPipeline,
    blend_pipeline: wgpu::RenderPipeline,
    mask_pipeline: wgpu::RenderPipeline,

    // Shared resources
    fullscreen_quad: wgpu::Buffer,
    linear_sampler: wgpu::Sampler,
    texture_sampler_bgl: wgpu::BindGroupLayout,
    surface_format: wgpu::TextureFormat,
}

impl Compositor {
    /// Create a new compositor. Requires an already-initialized wgpu
    /// device and queue. `surface_format` is the format of the final
    /// render target (obtained from the surface capabilities).
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        // ── Fullscreen quad vertex buffer ──
        let fullscreen_quad = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("compositor-fullscreen-quad"),
            contents: bytemuck::cast_slice(&FULLSCREEN_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // ── Samplers ──
        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("compositor-linear-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        // ── Bind group layouts ──
        let texture_sampler_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("compositor-texture-sampler-bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let layer_uniform_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("compositor-layer-uniform-bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let blend_uniform_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("compositor-blend-uniform-bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let mask_uniform_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("compositor-mask-uniform-bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        // ── Shader modules ──
        let fullscreen_shader =
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("compositor-fullscreen"),
                source: wgpu::ShaderSource::Wgsl(
                    crate::compositor::shaders::LAYER_SHADER.chars().next().map_or_else(
                        || wgpu::ShaderSource::Wgsl("".into()),
                        |_| {
                            // We need a separate fullscreen vertex shader
                            // Re-use layer shader which has a vertex_main entry
                            wgpu::ShaderSource::Wgsl(
                                include_str!("shaders/fullscreen.wgsl").into()
                            )
                        }
                    )
                ),
            });

        // Actually, let's use the gpu crate's fullscreen shader pattern.
        // For now, we inline a minimal fullscreen vertex shader.
        let fs_shader_src = r"
@vertex
fn vertex_main(@location(0) position: vec2f) -> @builtin(position) vec4f {
    return vec4f(position, 0.0, 1.0);
}
";
        let fullscreen_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("compositor-fullscreen-vs"),
            source: wgpu::ShaderSource::Wgsl(fs_shader_src.into()),
        });

        let layer_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("compositor-layer-shader"),
            source: wgpu::ShaderSource::Wgsl(
                crate::compositor::shaders::LAYER_SHADER.into(),
            ),
        });

        let blend_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("compositor-blend-shader"),
            source: wgpu::ShaderSource::Wgsl(
                crate::compositor::shaders::BLEND_SHADER.into(),
            ),
        });

        let mask_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("compositor-mask-shader"),
            source: wgpu::ShaderSource::Wgsl(
                crate::compositor::shaders::MASK_SHADER.into(),
            ),
        });

        // ── Pipeline layouts ──
        let layer_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("compositor-layer-pipeline-layout"),
                bind_group_layouts: &[&texture_sampler_bgl, &layer_uniform_bgl],
                push_constant_ranges: &[],
            });

        let blend_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("compositor-blend-pipeline-layout"),
                bind_group_layouts: &[
                    &texture_sampler_bgl,
                    &texture_sampler_bgl,
                    &blend_uniform_bgl,
                ],
                push_constant_ranges: &[],
            });

        let mask_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("compositor-mask-pipeline-layout"),
                bind_group_layouts: &[
                    &texture_sampler_bgl,
                    &texture_sampler_bgl,
                    &mask_uniform_bgl,
                ],
                push_constant_ranges: &[],
            });

        let vertex_buffers = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 2]>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            }],
        }];

        // ── Pipelines ──
        let layer_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("compositor-layer-pipeline"),
                layout: Some(&layer_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &fullscreen_shader,
                    entry_point: Some("vertex_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &vertex_buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &layer_shader,
                    entry_point: Some("fragment_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        let blend_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("compositor-blend-pipeline"),
                layout: Some(&blend_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &fullscreen_shader,
                    entry_point: Some("vertex_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &vertex_buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &blend_shader,
                    entry_point: Some("fragment_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        let mask_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("compositor-mask-pipeline"),
                layout: Some(&mask_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &fullscreen_shader,
                    entry_point: Some("vertex_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &vertex_buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &mask_shader,
                    entry_point: Some("fragment_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        Self {
            textures: TextureStore::new(),
            layer_uniform_bgl,
            blend_uniform_bgl,
            mask_uniform_bgl,
            layer_pipeline,
            blend_pipeline,
            mask_pipeline,
            fullscreen_quad,
            linear_sampler,
            texture_sampler_bgl,
            surface_format,
        }
    }

    /// Create a render texture with RGBA8 format.
    pub fn create_render_texture(
        &self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        label: &'static str,
    ) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    }

    /// Composite a full frame descriptor into an output texture.
    ///
    /// This is the main entry point. It takes a `FrameDescriptor`,
    /// processes each layer (render with quad transform, apply effects,
    /// apply mask, blend onto scene), and returns the final texture.
    pub fn render_frame(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &FrameDescriptor,
    ) -> Result<wgpu::Texture, CompositorError> {
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("compositor-frame-encoder"),
            });

        // 1. Create cleared scene texture
        let mut scene = self.create_render_texture(device, frame.width, frame.height, "scene");
        self.clear_texture(device, &mut encoder, &scene, frame.clear.color);

        // 2. Process each layer
        for item in &frame.items {
            match item {
                FrameItemDescriptor::Layer(layer) => {
                    let layer_tex = self.render_layer(
                        device, &mut encoder, frame.width, frame.height, layer,
                    )?;
                    scene = self.blend(
                        device, &mut encoder, &scene, &layer_tex,
                        layer.blend_mode, frame.width, frame.height,
                    );
                }
                FrameItemDescriptor::SceneEffect { .. } => {
                    // Scene-wide effects are composed later in the pipeline.
                    // For now, skip them (no effect pipeline wired yet).
                }
            }
        }

        queue.submit([encoder.finish()]);
        Ok(scene)
    }

    /// Clear a texture to a solid color.
    fn clear_texture(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        color: [f32; 4],
    ) {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("compositor-clear-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: color[0] as f64,
                        g: color[1] as f64,
                        b: color[2] as f64,
                        a: color[3] as f64,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
    }

    /// Render a single layer: transform texture → optional mask → blend.
    fn render_layer(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        width: u32,
        height: u32,
        layer: &LayerDescriptor,
    ) -> Result<wgpu::Texture, CompositorError> {
        // Get source texture from store
        let source = self.textures.get(&layer.texture_id).ok_or_else(|| {
            CompositorError::MissingTexture {
                texture_id: layer.texture_id.clone(),
            }
        })?;

        // Create output texture
        let output = self.create_render_texture(device, width, height, "layer-output");

        self.render_quad_transform(
            device, encoder, source, &output, width, height,
            &layer.transform.center_x,
            &layer.transform.center_y,
            layer.transform.width,
            layer.transform.height,
            layer.transform.rotation_degrees,
            layer.opacity,
            layer.transform.flip_x,
            layer.transform.flip_y,
        );

        // If there's a mask, apply it
        if let Some(mask) = &layer.mask {
            let mask_source = self.textures.get(&mask.texture_id).ok_or_else(|| {
                CompositorError::MissingTexture {
                    texture_id: mask.texture_id.clone(),
                }
            })?;
            let masked = self.create_render_texture(device, width, height, "layer-masked");
            self.apply_mask(device, encoder, &output, mask_source, &masked, mask.inverted, width, height);
            return Ok(masked);
        }

        Ok(output)
    }

    /// Render a texture with quad transform (position, scale, rotation, flip, opacity).
    fn render_quad_transform(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Texture,
        target: &wgpu::Texture,
        width: u32,
        height: u32,
        center_x: &f32,
        center_y: &f32,
        size_w: f32,
        size_h: f32,
        rotation_degrees: f32,
        opacity: f32,
        flip_x: bool,
        flip_y: bool,
    ) {
        let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let source_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compositor-layer-source-bg"),
            layout: &self.texture_sampler_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&source_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
            ],
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("compositor-layer-uniform"),
            contents: bytemuck::bytes_of(&LayerUniformBuffer {
                resolution: [width as f32, height as f32],
                center: [*center_x, *center_y],
                size: [size_w, size_h],
                rotation_radians: rotation_degrees.to_radians(),
                opacity,
                flip_x: if flip_x { 1.0 } else { 0.0 },
                flip_y: if flip_y { 1.0 } else { 0.0 },
                _padding: [0.0; 2],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compositor-layer-uniform-bg"),
            layout: &self.layer_uniform_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("compositor-layer-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            render_pass.set_pipeline(&self.layer_pipeline);
            render_pass.set_vertex_buffer(0, self.fullscreen_quad.slice(..));
            render_pass.set_bind_group(0, &source_bind_group, &[]);
            render_pass.set_bind_group(1, &uniform_bind_group, &[]);
            render_pass.draw(0..6, 0..1);
        }
    }

    /// Blend two textures using the specified blend mode.
    fn blend(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        base: &wgpu::Texture,
        layer: &wgpu::Texture,
        blend_mode: BlendMode,
        width: u32,
        height: u32,
    ) -> wgpu::Texture {
        let target = self.create_render_texture(device, width, height, "blend-output");
        let base_view = base.create_view(&wgpu::TextureViewDescriptor::default());
        let layer_view = layer.create_view(&wgpu::TextureViewDescriptor::default());
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let base_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compositor-blend-base-bg"),
            layout: &self.texture_sampler_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&base_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
            ],
        });

        let layer_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compositor-blend-layer-bg"),
            layout: &self.texture_sampler_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&layer_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
            ],
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("compositor-blend-uniform"),
            contents: bytemuck::bytes_of(&BlendUniformBuffer {
                blend_mode: blend_mode.shader_code(),
                _padding: [0; 3],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compositor-blend-uniform-bg"),
            layout: &self.blend_uniform_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("compositor-blend-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            render_pass.set_pipeline(&self.blend_pipeline);
            render_pass.set_vertex_buffer(0, self.fullscreen_quad.slice(..));
            render_pass.set_bind_group(0, &base_bind_group, &[]);
            render_pass.set_bind_group(1, &layer_bind_group, &[]);
            render_pass.set_bind_group(2, &uniform_bind_group, &[]);
            render_pass.draw(0..6, 0..1);
        }

        target
    }

    /// Apply a mask to a layer.
    fn apply_mask(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        layer_tex: &wgpu::Texture,
        mask_tex: &wgpu::Texture,
        target: &wgpu::Texture,
        inverted: bool,
        width: u32,
        height: u32,
    ) {
        let _w = width; // used for consistent sizing
        let _h = height;
        let layer_view = layer_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let mask_view = mask_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let layer_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compositor-mask-layer-bg"),
            layout: &self.texture_sampler_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&layer_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
            ],
        });

        let mask_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compositor-mask-mask-bg"),
            layout: &self.texture_sampler_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&mask_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
            ],
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("compositor-mask-uniform"),
            contents: bytemuck::bytes_of(&MaskUniformBuffer {
                inverted: if inverted { 1.0 } else { 0.0 },
                _padding: [0.0; 3],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compositor-mask-uniform-bg"),
            layout: &self.mask_uniform_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("compositor-mask-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            render_pass.set_pipeline(&self.mask_pipeline);
            render_pass.set_vertex_buffer(0, self.fullscreen_quad.slice(..));
            render_pass.set_bind_group(0, &layer_bind_group, &[]);
            render_pass.set_bind_group(1, &mask_bind_group, &[]);
            render_pass.set_bind_group(2, &uniform_bind_group, &[]);
            render_pass.draw(0..6, 0..1);
        }
    }
}

impl Default for TextureStore {
    fn default() -> Self {
        Self::new()
    }
}
