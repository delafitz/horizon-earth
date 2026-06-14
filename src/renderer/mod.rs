//! wgpu rendering: surface setup, pipelines, and the per-frame draw.

pub mod mesh;

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;
use winit::window::Window;

use mesh::{VertexPC, VertexPN};

// Natural Earth vector data, embedded so the binary is self-contained.
const COASTLINE_GEOJSON: &str = include_str!("../../assets/earth/ne_110m_coastline.geojson");
const COUNTRIES_GEOJSON: &str =
    include_str!("../../assets/earth/ne_110m_admin_0_countries.geojson");

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

// Nord palette (linear-ish RGB, written directly to a non-sRGB target).
const COLOR_COAST: [f32; 3] = [0.533, 0.753, 0.816]; // Nord8 #88C0D0
const COLOR_BORDER: [f32; 3] = [0.298, 0.337, 0.416]; // Nord3 #4C566A
const COLOR_BG: wgpu::Color = wgpu::Color {
    r: 0.180,
    g: 0.204,
    b: 0.251,
    a: 1.0,
}; // Nord0 #2E3440

const ATTRS_PN: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
const ATTRS_PC: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
const ATTRS_POS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x3];

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    cam_pos: [f32; 4],
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,

    depth_view: wgpu::TextureView,

    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,

    starfield_pipeline: wgpu::RenderPipeline,
    globe_pipeline: wgpu::RenderPipeline,
    lines_pipeline: wgpu::RenderPipeline,
    atmosphere_pipeline: wgpu::RenderPipeline,

    sphere_vbuf: wgpu::Buffer,
    sphere_ibuf: wgpu::Buffer,
    sphere_icount: u32,

    line_vbuf: wgpu::Buffer,
    line_vcount: u32,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Renderer {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        let surface = instance
            .create_surface(window.clone())
            .expect("failed to create surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .expect("no suitable GPU adapter found");

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("horizon-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await
            .expect("failed to create device");

        let caps = surface.get_capabilities(&adapter);
        // Prefer a non-sRGB format so our palette values display as authored.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Fifo) {
            wgpu::PresentMode::Fifo
        } else {
            caps.present_modes[0]
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let depth_view = create_depth_view(&device, &config);

        // --- Uniforms / bind group ------------------------------------------
        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniform-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniform-bind-group"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline-layout"),
            bind_group_layouts: &[&bind_layout],
            push_constant_ranges: &[],
        });

        // --- Shaders ---------------------------------------------------------
        let starfield_sh = shader(&device, "starfield", include_str!("../../assets/shaders/starfield.wgsl"));
        let globe_sh = shader(&device, "globe", include_str!("../../assets/shaders/globe.wgsl"));
        let lines_sh = shader(&device, "lines", include_str!("../../assets/shaders/lines.wgsl"));
        let atmo_sh = shader(&device, "atmosphere", include_str!("../../assets/shaders/atmosphere.wgsl"));

        let pn_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<VertexPN>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRS_PN,
        };
        let pc_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<VertexPC>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRS_PC,
        };
        let pos_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<VertexPN>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRS_POS,
        };

        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };

        let starfield_pipeline = make_pipeline(
            &device, &pipeline_layout, &starfield_sh, format, &[],
            wgpu::PrimitiveTopology::TriangleList, None, false,
            wgpu::CompareFunction::Always,
        );
        let globe_pipeline = make_pipeline(
            &device, &pipeline_layout, &globe_sh, format, &[pn_layout],
            wgpu::PrimitiveTopology::TriangleList, None, true,
            wgpu::CompareFunction::Less,
        );
        let lines_pipeline = make_pipeline(
            &device, &pipeline_layout, &lines_sh, format, &[pc_layout],
            wgpu::PrimitiveTopology::LineList, None, true,
            wgpu::CompareFunction::LessEqual,
        );
        let atmosphere_pipeline = make_pipeline(
            &device, &pipeline_layout, &atmo_sh, format, &[pos_layout],
            wgpu::PrimitiveTopology::TriangleList, Some(additive), false,
            wgpu::CompareFunction::LessEqual,
        );

        // --- Geometry --------------------------------------------------------
        let (sverts, sidx) = mesh::uv_sphere(64, 96, 1.0);
        let sphere_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sphere-verts"),
            contents: bytemuck::cast_slice(&sverts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let sphere_ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sphere-indices"),
            contents: bytemuck::cast_slice(&sidx),
            usage: wgpu::BufferUsages::INDEX,
        });
        let sphere_icount = sidx.len() as u32;

        let coast = crate::data::extract_polylines(COASTLINE_GEOJSON);
        let countries = crate::data::extract_polylines(COUNTRIES_GEOJSON);
        let mut line_verts: Vec<VertexPC> = Vec::new();
        // Borders sit a hair below coastlines so coastlines win where they overlap.
        crate::earth::build_lines(&countries, COLOR_BORDER, 1.0020, &mut line_verts);
        crate::earth::build_lines(&coast, COLOR_COAST, 1.0030, &mut line_verts);
        log::info!(
            "loaded {} coastline + {} country polylines = {} line vertices",
            coast.len(),
            countries.len(),
            line_verts.len()
        );

        let line_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("line-verts"),
            contents: bytemuck::cast_slice(&line_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let line_vcount = line_verts.len() as u32;

        Renderer {
            surface,
            device,
            queue,
            config,
            depth_view,
            uniform_buf,
            bind_group,
            starfield_pipeline,
            globe_pipeline,
            lines_pipeline,
            atmosphere_pipeline,
            sphere_vbuf,
            sphere_ibuf,
            sphere_icount,
            line_vbuf,
            line_vcount,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, &self.config);
    }

    /// Update camera + model transform for the given elapsed time (seconds).
    pub fn update(&mut self, time: f32) {
        let aspect = self.config.width as f32 / self.config.height.max(1) as f32;
        let proj = Mat4::perspective_rh(45f32.to_radians(), aspect, 0.1, 100.0);
        let eye = Vec3::new(0.0, 0.7, 2.7);
        let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
        let model = Mat4::from_rotation_y(time * 0.12);

        let u = Uniforms {
            view_proj: (proj * view).to_cols_array_2d(),
            model: model.to_cols_array_2d(),
            cam_pos: [eye.x, eye.y, eye.z, 1.0],
        };
        self.queue
            .write_buffer(&self.uniform_buf, 0, bytemuck::cast_slice(&[u]));
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let frame = self.surface.get_current_texture()?;
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(COLOR_BG),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            rp.set_bind_group(0, &self.bind_group, &[]);

            // Background stars.
            rp.set_pipeline(&self.starfield_pipeline);
            rp.draw(0..3, 0..1);

            // Solid globe.
            rp.set_pipeline(&self.globe_pipeline);
            rp.set_vertex_buffer(0, self.sphere_vbuf.slice(..));
            rp.set_index_buffer(self.sphere_ibuf.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..self.sphere_icount, 0, 0..1);

            // Coastlines + borders.
            rp.set_pipeline(&self.lines_pipeline);
            rp.set_vertex_buffer(0, self.line_vbuf.slice(..));
            rp.draw(0..self.line_vcount, 0..1);

            // Atmospheric glow.
            rp.set_pipeline(&self.atmosphere_pipeline);
            rp.set_vertex_buffer(0, self.sphere_vbuf.slice(..));
            rp.set_index_buffer(self.sphere_ibuf.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..self.sphere_icount, 0, 0..1);
        }

        self.queue.submit(Some(enc.finish()));
        frame.present();
        Ok(())
    }
}

fn shader(device: &wgpu::Device, label: &str, src: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(src.into()),
    })
}

fn create_depth_view(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

#[allow(clippy::too_many_arguments)]
fn make_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    buffers: &[wgpu::VertexBufferLayout],
    topology: wgpu::PrimitiveTopology,
    blend: Option<wgpu::BlendState>,
    depth_write: bool,
    depth_compare: wgpu::CompareFunction,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: "vs_main",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers,
        },
        primitive: wgpu::PrimitiveState {
            topology,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: depth_write,
            depth_compare,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: "fs_main",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    })
}
