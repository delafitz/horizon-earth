//! wgpu rendering: surface setup, pipelines, and the per-frame draw.

mod glyphs;
pub mod mesh;

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;
use winit::window::Window;

use horizon_core::frames::eci_to_world;
use horizon_core::orbit::sample_track;
use horizon_core::units::EARTH_RADIUS_KM;
use horizon_core::{CameraRig, Epoch, World};
use mesh::{MarkerInstance, VertexPC, VertexPN};

use crate::ui::RenderSettings;

/// [egui] One frame of tessellated egui geometry, painted over the 3D scene by
/// [`Renderer::render`]. `None` means no overlay this frame.
pub struct EguiFrame<'a> {
    pub primitives: &'a [egui::ClippedPrimitive],
    pub textures_delta: &'a egui::TexturesDelta,
    pub pixels_per_point: f32,
}

// Natural Earth vector data, embedded so the binary is self-contained.
const COASTLINE_GEOJSON: &str = include_str!("../../assets/earth/ne_110m_coastline.geojson");
const COUNTRIES_GEOJSON: &str =
    include_str!("../../assets/earth/ne_110m_admin_0_countries.geojson");

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

// Nord palette (linear-ish RGB, written directly to a non-sRGB target).
const COLOR_COAST: [f32; 3] = [0.533, 0.753, 0.816]; // Nord8 #88C0D0
const COLOR_BORDER: [f32; 3] = [0.298, 0.337, 0.416]; // Nord3 #4C566A
const COLOR_LAND: [f32; 3] = [0.263, 0.298, 0.369]; // Nord3-ish land fill (low alpha)
// Land-fill curvature tolerance: the max angle (degrees) a fill triangle edge
// may span before it's subdivided and snapped to the sphere. Smaller = smoother
// fill that hugs the curve (more triangles), larger = flatter/cheaper. ~2° keeps
// the sag well under a kilometre, invisible even in fly mode.
const FILL_SUBDIV_TOLERANCE_DEG: f64 = 2.0;
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
const ATTRS_CORNER: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
// loc 1 = center+size (vec4); loc 2 = color.rgb + kind (vec4).
const ATTRS_MARKER_INST: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![1 => Float32x4, 2 => Float32x4];
// Thick-line instance: loc 1 = p0, loc 2 = p1, loc 3 = rgb + layer.
const ATTRS_THICK_INST: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![1 => Float32x3, 2 => Float32x3, 3 => Float32x4];

// Ground anchors: nadir drop-lines + footprint rings, reusing the thick-line
// instance/shader. Width/alpha/far-side opacity are now egui-driven (style2.x/y
// and style2.z). The footprint is the physical horizon circle (angular radius
// from altitude); `RING_SEGMENTS` points per ring.
const RING_SEGMENTS: usize = 48;
// Surface radius for ground geometry: just above the globe and land fill.
const GROUND_RADIUS: f32 = 1.0015;
// Layer value selecting the ground width in the thick-line shader.
const LAYER_GROUND: f32 = 2.0;

// On-screen half-size of a body marker, in NDC units.
const MARKER_SIZE: f32 = 0.01;

// Vector label line vertex: NDC position + colour.
const ATTRS_LABEL: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x3];
// Glyph cell height (px) and the per-frame line-vertex cap.
const LABEL_PX: f32 = 24.0;
const MAX_LABEL_VERTS: usize = 16384;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    cam_pos: [f32; 4],
    params: [f32; 4], // params.x = viewport aspect
    // [egui] UI-driven style knobs, appended so shaders that don't read them are
    // unaffected (each shader declares only the prefix it needs).
    style0: [f32; 4], // x = far-side line alpha, y = land fill alpha,
    // z = orbit-track alpha, w = line brightness
    style1: [f32; 4], // x = atmosphere intensity, y = atmosphere outer radius,
    // z = coastline width px, w = border width px
    style2: [f32; 4], // x = ground-line width px, y = ground-line alpha,
    // z = far-side alpha for satellite layers (tracks/markers/ground)
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
    fill_pipeline: wgpu::RenderPipeline,
    lines_back_pipeline: wgpu::RenderPipeline,
    lines_pipeline: wgpu::RenderPipeline,
    ground_pipeline: wgpu::RenderPipeline,
    ground_back_pipeline: wgpu::RenderPipeline,
    atmosphere_pipeline: wgpu::RenderPipeline,
    track_back_pipeline: wgpu::RenderPipeline,
    track_pipeline: wgpu::RenderPipeline,
    markers_back_pipeline: wgpu::RenderPipeline,
    markers_pipeline: wgpu::RenderPipeline,
    label_pipeline: wgpu::RenderPipeline,
    label_vbuf: wgpu::Buffer,
    label_vcount: u32,

    sphere_vbuf: wgpu::Buffer,
    sphere_ibuf: wgpu::Buffer,
    sphere_icount: u32,

    line_quad_vbuf: wgpu::Buffer,
    line_vbuf: wgpu::Buffer,
    line_vcount: u32,

    // Ground anchors (nadir lines + footprint rings), rebuilt each frame since
    // the satellites move. Shares `line_quad_vbuf` for the expansion quad.
    ground_inst_buf: wgpu::Buffer,
    ground_count: u32,

    fill_vbuf: wgpu::Buffer,
    fill_vcount: u32,

    track_vbuf: wgpu::Buffer,
    track_vcount: u32,

    marker_quad_vbuf: wgpu::Buffer,
    marker_inst_buf: wgpu::Buffer,

    camera: CameraRig,
    world: World,

    // [egui] overlay painter + the live parameter values the UI drives.
    egui_renderer: egui_wgpu::Renderer,
    settings: RenderSettings,
    // [egui] which categories currently have orbit tracks in `track_vbuf`;
    // rebuilt when the per-type selection changes.
    track_mask: u32,
}

impl Renderer {
    pub async fn new(window: Arc<Window>, world: World) -> Renderer {
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
        let thick_lines_sh = shader(
            &device,
            "thick_lines",
            include_str!("../../assets/shaders/thick_lines.wgsl"),
        );
        let atmo_sh = shader(&device, "atmosphere", include_str!("../../assets/shaders/atmosphere.wgsl"));
        let track_sh = shader(&device, "track", include_str!("../../assets/shaders/track.wgsl"));
        let markers_sh = shader(&device, "markers", include_str!("../../assets/shaders/markers.wgsl"));

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
        let track_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<VertexPC>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRS_PC,
        };
        let corner_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 2]>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRS_CORNER,
        };
        let inst_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MarkerInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRS_MARKER_INST,
        };
        let thick_inst_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<mesh::ThickLineInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRS_THICK_INST,
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
            wgpu::CompareFunction::Always, "fs_main",
        );
        let globe_pipeline = make_pipeline(
            &device, &pipeline_layout, &globe_sh, format, &[pn_layout],
            wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::BlendState::ALPHA_BLENDING), true,
            wgpu::CompareFunction::Less, "fs_main",
        );
        // Translucent land fill: closed country rings as triangles. Depth test
        // disabled (compare Always, no write) so the flat triangles' sagging
        // interiors aren't clipped by the globe; the shader discards the far
        // hemisphere per fragment instead. Its own dedicated vs/fs entries pass
        // world position through for that test.
        let fill_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("land-fill"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &lines_sh,
                entry_point: "vs_fill",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[pc_layout.clone()],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &lines_sh,
                entry_point: "fs_fill",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        // Far-hemisphere lines: only where they sit behind the globe surface
        // (depth Greater), faint and non-depth-writing. Coastlines/borders are
        // instanced thick-line quads (one instance per segment), expanded to a
        // constant pixel width in the vertex shader.
        let lines_back_pipeline = make_pipeline(
            &device, &pipeline_layout, &thick_lines_sh, format,
            &[corner_layout.clone(), thick_inst_layout.clone()],
            wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::BlendState::ALPHA_BLENDING), false,
            wgpu::CompareFunction::Greater, "fs_back",
        );
        let lines_pipeline = make_pipeline(
            &device, &pipeline_layout, &thick_lines_sh, format,
            &[corner_layout.clone(), thick_inst_layout.clone()],
            wgpu::PrimitiveTopology::TriangleList, None, true,
            wgpu::CompareFunction::LessEqual, "fs_main",
        );
        // Ground anchors (nadir lines + footprint rings): same thick-line shader,
        // alpha-blended, no depth write (so they never occlude markers/labels).
        // Near pass shows the front side; far pass is dimmed "through the glass".
        let ground_back_pipeline = make_pipeline(
            &device, &pipeline_layout, &thick_lines_sh, format,
            &[corner_layout.clone(), thick_inst_layout.clone()],
            wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::BlendState::ALPHA_BLENDING), false,
            wgpu::CompareFunction::Greater, "fs_ground_back",
        );
        let ground_pipeline = make_pipeline(
            &device, &pipeline_layout, &thick_lines_sh, format,
            &[corner_layout.clone(), thick_inst_layout],
            wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::BlendState::ALPHA_BLENDING), false,
            wgpu::CompareFunction::LessEqual, "fs_ground",
        );
        let atmosphere_pipeline = make_pipeline(
            &device, &pipeline_layout, &atmo_sh, format, &[pos_layout],
            wgpu::PrimitiveTopology::TriangleList, Some(additive), false,
            wgpu::CompareFunction::LessEqual, "fs_main",
        );
        // Orbit tracks and body markers each get a near pass (depth LessEqual,
        // full) and a far pass (depth Greater, faint) so whatever sits behind
        // the translucent globe still shows through "the glass".
        let track_back_pipeline = make_pipeline(
            &device, &pipeline_layout, &track_sh, format, &[track_layout.clone()],
            wgpu::PrimitiveTopology::LineList,
            Some(wgpu::BlendState::ALPHA_BLENDING), false,
            wgpu::CompareFunction::Greater, "fs_back",
        );
        let track_pipeline = make_pipeline(
            &device, &pipeline_layout, &track_sh, format, &[track_layout],
            wgpu::PrimitiveTopology::LineList,
            Some(wgpu::BlendState::ALPHA_BLENDING), false,
            wgpu::CompareFunction::LessEqual, "fs_main",
        );
        // Body markers: instanced billboards (quad buffer + instance buffer).
        let markers_back_pipeline = make_pipeline(
            &device, &pipeline_layout, &markers_sh, format,
            &[corner_layout.clone(), inst_layout.clone()],
            wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::BlendState::ALPHA_BLENDING), false,
            wgpu::CompareFunction::Greater, "fs_back",
        );
        let markers_pipeline = make_pipeline(
            &device, &pipeline_layout, &markers_sh, format, &[corner_layout, inst_layout],
            wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::BlendState::ALPHA_BLENDING), false,
            wgpu::CompareFunction::LessEqual, "fs_main",
        );

        // --- HUD label text -------------------------------------------------
        // Vector stroke font drawn as screen-space lines (positions already NDC,
        // so no uniforms/bind groups are needed).
        let label_sh = shader(&device, "label", include_str!("../../assets/shaders/label.wgsl"));
        let label_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("label-pipeline-layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let label_vbuf_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<mesh::LabelVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRS_LABEL,
        };
        let label_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("label"),
            layout: Some(&label_layout),
            vertex: wgpu::VertexState {
                module: &label_sh,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[label_vbuf_layout],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &label_sh,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let label_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("label-verts"),
            size: (MAX_LABEL_VERTS * std::mem::size_of::<mesh::LabelVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

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
        // One thick-line instance per segment. layer 1.0 = border, 0.0 = coast;
        // borders sit a hair below coastlines so coastlines win where they overlap.
        let mut line_insts: Vec<mesh::ThickLineInstance> = Vec::new();
        crate::earth::build_thick_lines(&countries, COLOR_BORDER, 1.0, 1.0020, &mut line_insts);
        crate::earth::build_thick_lines(&coast, COLOR_COAST, 0.0, 1.0030, &mut line_insts);
        log::info!(
            "loaded {} coastline + {} country polylines = {} line segments",
            coast.len(),
            countries.len(),
            line_insts.len()
        );

        let line_quad_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("line-quad"),
            contents: bytemuck::cast_slice(&mesh::LINE_QUAD),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let line_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("line-instances"),
            contents: bytemuck::cast_slice(&line_insts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let line_vcount = line_insts.len() as u32;

        // Translucent land fill from the closed country rings, sitting just above
        // the globe surface and below the borders/coastlines.
        let rings = crate::data::extract_polygon_rings(COUNTRIES_GEOJSON);
        let mut fill_verts: Vec<VertexPC> = Vec::new();
        crate::earth::build_fill(
            &rings,
            COLOR_LAND,
            1.0010,
            FILL_SUBDIV_TOLERANCE_DEG.to_radians(),
            &mut fill_verts,
        );
        log::info!("triangulated {} land rings = {} fill vertices", rings.len(), fill_verts.len());
        let fill_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("fill-verts"),
            contents: bytemuck::cast_slice(&fill_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let fill_vcount = fill_verts.len() as u32;

        // --- Orbiting bodies -------------------------------------------------
        let camera = CameraRig::default();
        // `world` is supplied by the caller (real tracked objects or the demo
        // constellation).

        // One-time ground-track diagnostic. Reports each body's sub-satellite
        // point two independent ways: the canonical astrodynamics computation
        // (ECI -> ECEF via -GMST) and one derived from the *render* pipeline
        // (eci_to_world + the same GMST spin the globe uses). They must agree
        // for ground tracks to line up with the drawn geography.
        if log::log_enabled!(log::Level::Info) {
            use glam::DMat3;
            let g = world.earth_rotation();
            let unix = (world.current().jd - horizon_core::time::UNIX_EPOCH_JD) * 86400.0;
            log::info!("ground-track check @ unix {unix:.0} (gmst {g:.4} rad):");
            for (i, b) in world.bodies.iter().enumerate() {
                let p = world.body_position_eci(i);
                let r = p.length();
                if r < 1.0 {
                    continue; // failed propagation collapses to the origin
                }
                let ecef = DMat3::from_rotation_z(-g) * p;
                let lat_c = (ecef.z / r).asin().to_degrees();
                let lon_c = ecef.y.atan2(ecef.x).to_degrees();
                let sat = eci_to_world(p);
                let er = DMat3::from_rotation_y(-g) * sat;
                let lat_r = (er.y / er.length()).asin().to_degrees();
                let lon_r = (-er.z).atan2(er.x).to_degrees();
                let alt = r - horizon_core::units::EARTH_RADIUS_KM;
                log::info!(
                    "  {:<18} sub=({lat_c:6.1},{lon_c:7.1})  render=({lat_r:6.1},{lon_r:7.1})  alt={alt:6.0}km",
                    b.name
                );
            }
        }

        // Static orbit paths (the body slides along them each frame). Sampled in
        // ECI/km over one period, then mapped into the render frame.
        let mut track_verts: Vec<VertexPC> = Vec::new();
        for body in &world.bodies {
            let col = [body.color[0] * 0.85, body.color[1] * 0.85, body.color[2] * 0.85];
            let pts = sample_track(body.motion.as_ref(), 128);
            for w in pts.windows(2) {
                let a = eci_to_world(w[0]).as_vec3().to_array();
                let b = eci_to_world(w[1]).as_vec3().to_array();
                track_verts.push(VertexPC { pos: a, col });
                track_verts.push(VertexPC { pos: b, col });
            }
        }
        let track_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("track-verts"),
            contents: bytemuck::cast_slice(&track_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let track_vcount = track_verts.len() as u32;

        let marker_quad_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("marker-quad"),
            contents: bytemuck::cast_slice(&mesh::MARKER_QUAD),
            usage: wgpu::BufferUsages::VERTEX,
        });
        // Filled each frame in `update`; one MarkerInstance per body.
        let marker_inst_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("marker-instances"),
            size: (world.bodies.len().max(1) * std::mem::size_of::<MarkerInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // One nadir line + a footprint ring per body, refilled in `update`.
        let ground_per_body = 1 + RING_SEGMENTS;
        let ground_inst_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ground-instances"),
            size: (world.bodies.len().max(1)
                * ground_per_body
                * std::mem::size_of::<mesh::ThickLineInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // [egui] Painter targeting the same surface format; no depth, no MSAA.
        let egui_renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);

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
            fill_pipeline,
            lines_back_pipeline,
            lines_pipeline,
            ground_pipeline,
            ground_back_pipeline,
            atmosphere_pipeline,
            track_back_pipeline,
            track_pipeline,
            markers_back_pipeline,
            markers_pipeline,
            label_pipeline,
            label_vbuf,
            label_vcount: 0,
            sphere_vbuf,
            sphere_ibuf,
            sphere_icount,
            line_quad_vbuf,
            line_vbuf,
            line_vcount,
            ground_inst_buf,
            ground_count: 0,
            fill_vbuf,
            fill_vcount,
            track_vbuf,
            track_vcount,
            marker_quad_vbuf,
            marker_inst_buf,
            camera,
            world,
            egui_renderer,
            settings: RenderSettings::default(),
            // The track buffer above is built for every body (all categories on).
            track_mask: RenderSettings::default().track_mask(),
        }
    }

    /// [egui] Rebuild the orbit-track vertex buffer, including only categories
    /// whose per-type "orbit track" toggle is on. Called when that set changes.
    fn rebuild_tracks(&mut self) {
        let mut verts: Vec<VertexPC> = Vec::new();
        for body in &self.world.bodies {
            if !self.settings.track_visible(body.category) {
                continue;
            }
            let col = [body.color[0] * 0.85, body.color[1] * 0.85, body.color[2] * 0.85];
            let pts = sample_track(body.motion.as_ref(), 128);
            for w in pts.windows(2) {
                verts.push(VertexPC { pos: eci_to_world(w[0]).as_vec3().to_array(), col });
                verts.push(VertexPC { pos: eci_to_world(w[1]).as_vec3().to_array(), col });
            }
        }
        self.track_vcount = verts.len() as u32;
        // Keep the old buffer when empty (zero-size buffers are invalid); the
        // zero vcount already suppresses the draw.
        if !verts.is_empty() {
            self.track_vbuf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("track-verts"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
        }
    }

    /// [egui] Copy the current UI-driven parameters in; read during update/render.
    pub fn set_settings(&mut self, settings: RenderSettings) {
        self.settings = settings;
    }

    /// [egui] Swap in a new set of bodies (e.g. after re-sampling the group),
    /// resizing the per-body marker buffer and rebuilding the orbit tracks. The
    /// marker instances themselves are refilled by the next `update`.
    pub fn set_world(&mut self, world: World) {
        self.world = world;
        self.marker_inst_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("marker-instances"),
            size: (self.world.bodies.len().max(1) * std::mem::size_of::<MarkerInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.rebuild_tracks();
    }

    /// Read-only access to the simulated world (for the UI panels).
    pub fn world(&self) -> &World {
        &self.world
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

    /// Orbit the fixed camera by pixel-scaled yaw/pitch deltas (Fixed mode).
    pub fn orbit_camera(&mut self, dyaw: f32, dpitch: f32) {
        self.camera.orbit.orbit(dyaw as f64, dpitch as f64);
    }

    /// Dolly the fixed camera in (positive) or out (negative).
    pub fn zoom_camera(&mut self, factor: f32) {
        self.camera.orbit.zoom(factor as f64);
    }


    /// Toggle between the Fixed (Earth-centred) and Fly (orbit-riding) cameras.
    pub fn toggle_camera(&mut self) {
        self.camera.toggle();
        log::info!("camera mode: {:?}", self.camera.mode);
    }

    /// True when the fly camera is active (so the app can route attitude keys).
    pub fn is_fly_mode(&self) -> bool {
        self.camera.mode == horizon_core::CameraMode::Fly
    }

    /// Advance the camera (only the fly camera moves) by `dt` seconds.
    pub fn advance_camera(&mut self, dt: f32) {
        self.camera.advance(dt as f64);
    }

    /// Adjust fly-orbit speed (rad/s along the orbit).
    pub fn fly_adjust_speed(&mut self, delta: f32) {
        self.camera.fly.adjust_speed(delta as f64);
    }

    /// Adjust fly-orbit altitude (km).
    pub fn fly_adjust_altitude(&mut self, delta_km: f32) {
        self.camera.fly.adjust_altitude(delta_km as f64);
    }

    /// Adjust fly-orbit inclination (radians).
    pub fn fly_adjust_inclination(&mut self, delta: f32) {
        self.camera.fly.adjust_inclination(delta as f64);
    }

    /// Adjust fly-orbit RAAN (radians).
    pub fn fly_adjust_raan(&mut self, delta: f32) {
        self.camera.fly.adjust_raan(delta as f64);
    }

    /// Nudge the fly camera's attitude (yaw/pitch/roll, radians).
    pub fn fly_look(&mut self, dyaw: f32, dpitch: f32, droll: f32) {
        self.camera.fly.yaw += dyaw as f64;
        self.camera.fly.pitch += dpitch as f64;
        self.camera.fly.roll += droll as f64;
    }

    /// Set the world to time `now` and refresh per-frame GPU state.
    pub fn update(&mut self, now: Epoch) {
        self.world.set_time(now);

        let width = self.config.width as f32;
        let height = self.config.height.max(1) as f32;
        let aspect = width / height;
        let view_proj = self.camera.view_proj(aspect as f64).as_mat4();
        let eye = self.camera.eye().as_vec3();
        // The Earth's orientation is GMST(now): the rotation carrying the
        // Earth-fixed coastlines into the inertial frame the satellites live in.
        let spin = self.world.earth_rotation().rem_euclid(std::f64::consts::TAU) as f32;
        let model = Mat4::from_rotation_y(spin);

        let s = &self.settings;
        let u = Uniforms {
            view_proj: view_proj.to_cols_array_2d(),
            model: model.to_cols_array_2d(),
            cam_pos: [eye.x, eye.y, eye.z, 1.0],
            params: [aspect, width, height, 0.0],
            style0: [s.line_back_alpha, s.fill_alpha, s.track_alpha, s.line_brightness],
            // z = coastline width px, w = border width px (0 hides the layer).
            style1: [
                s.atmo_intensity,
                1.0 + s.atmo_thickness,
                s.coast_width_px(),
                s.border_width_px(),
            ],
            style2: [s.ground_width_px(), s.ground_alpha, s.sat_back_alpha, 0.0],
        };
        self.queue
            .write_buffer(&self.uniform_buf, 0, bytemuck::cast_slice(&[u]));

        // Refresh marker instances from the current orbit positions (ECI/km ->
        // render frame).
        let instances: Vec<MarkerInstance> = (0..self.world.bodies.len())
            .map(|i| {
                let p = eci_to_world(self.world.body_position_eci(i)).as_vec3();
                let b = &self.world.bodies[i];
                MarkerInstance {
                    center_size: [
                        p.x,
                        p.y,
                        p.z,
                        MARKER_SIZE
                            * b.category.size_scale()
                            * self.settings.marker_size
                            * self.settings.marker_scale(b.category),
                    ],
                    color: b.color,
                    kind: self.settings.symbol_kind(b.category),
                }
            })
            .collect();
        self.queue
            .write_buffer(&self.marker_inst_buf, 0, bytemuck::cast_slice(&instances));

        // Ground anchors: a nadir drop-line + footprint ring per body, in the
        // body's category colour. Built in the inertial frame like the markers,
        // then pre-rotated by -spin so the shared thick-line shader's model
        // (GMST) spin cancels — keeping them pinned directly under each body.
        let unspin = glam::Mat3::from_rotation_y(-spin);
        let mut ground: Vec<mesh::ThickLineInstance> = Vec::new();
        for i in 0..self.world.bodies.len() {
            // Hidden types suppress every artifact, ground anchors included.
            if !self.settings.type_visible(self.world.bodies[i].category) {
                continue;
            }
            let p = eci_to_world(self.world.body_position_eci(i)).as_vec3();
            let r = p.length();
            if r <= 1.0 {
                continue; // body at/under the surface: nothing to anchor
            }
            let c = self.world.bodies[i].color;
            let col_layer = [c[0], c[1], c[2], LAYER_GROUND];
            let n = p / r;
            let push = |out: &mut Vec<mesh::ThickLineInstance>, a: Vec3, b: Vec3| {
                out.push(mesh::ThickLineInstance {
                    p0: (unspin * a).to_array(),
                    p1: (unspin * b).to_array(),
                    col_layer,
                });
            };
            // Nadir drop-line: from the satellite to the surface point below it.
            push(&mut ground, p, n * GROUND_RADIUS);
            // Footprint ring: the horizon circle, angular radius acos(R/r) about
            // the nadir direction — so higher orbits draw visibly larger rings.
            let theta = (1.0 / r).clamp(-1.0, 1.0).acos();
            let seed = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
            let t = (seed - n * n.dot(seed)).normalize();
            let bvec = n.cross(t);
            let (st, ct) = theta.sin_cos();
            let ring_pt = |phi: f32| {
                let (sp, cp) = phi.sin_cos();
                (n * ct + (t * cp + bvec * sp) * st) * GROUND_RADIUS
            };
            let mut prev = ring_pt(0.0);
            for k in 1..=RING_SEGMENTS {
                let phi = k as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
                let cur = ring_pt(phi);
                push(&mut ground, prev, cur);
                prev = cur;
            }
        }
        self.ground_count = ground.len() as u32;
        if !ground.is_empty() {
            self.queue
                .write_buffer(&self.ground_inst_buf, 0, bytemuck::cast_slice(&ground));
        }

        // [egui] Rebuild orbit tracks only when the per-type selection changes.
        let mask = self.settings.track_mask();
        if mask != self.track_mask {
            self.track_mask = mask;
            self.rebuild_tracks();
        }

        // HUD labels: the fly-mode controls banner first (so it always shows),
        // then per-body labels (name/altitude/lat-lon), projected to screen.
        let mut verts: Vec<mesh::LabelVertex> = Vec::new();
        if self.is_fly_mode() {
            emit_fly_banner(&mut verts, width, height);
        }
        if self.settings.show_labels {
            verts.extend(self.build_labels(view_proj, eye, width, height));
        }
        verts.truncate(MAX_LABEL_VERTS);
        self.label_vcount = verts.len() as u32;
        if !verts.is_empty() {
            self.queue
                .write_buffer(&self.label_vbuf, 0, bytemuck::cast_slice(&verts));
        }
    }

    /// Build line vertices for visible body labels (name + altitude), in the
    /// body's category colour. Bodies hidden behind the globe are skipped, and
    /// labels that would overlap an already-placed one are dropped (nearest
    /// body wins).
    fn build_labels(
        &self,
        view_proj: Mat4,
        eye: Vec3,
        width: f32,
        height: f32,
    ) -> Vec<mesh::LabelVertex> {
        let unit = LABEL_PX / glyphs::GH; // px per grid unit (square)
        let line_h = LABEL_PX + 5.0; // px between the two text lines
        let off_x = LABEL_PX; // px from the marker to the text
        let min_sep = LABEL_PX * 1.6; // declutter radius, px

        // Project every visible body; collect (camera distance, screen px, index).
        let mut cands: Vec<(f32, [f32; 2], usize)> = Vec::new();
        for i in 0..self.world.bodies.len() {
            // Skip types the user has hidden (their markers are hidden too).
            if !self.settings.label_visible(self.world.bodies[i].category) {
                continue;
            }
            let p = eci_to_world(self.world.body_position_eci(i)).as_vec3();
            if occluded_by_globe(eye, p) {
                continue;
            }
            let clip = view_proj * Vec4::new(p.x, p.y, p.z, 1.0);
            if clip.w <= 0.0 {
                continue; // behind the camera
            }
            let nx = clip.x / clip.w;
            let ny = clip.y / clip.w;
            if nx.abs() > 1.1 || ny.abs() > 1.1 {
                continue; // off screen
            }
            let px = [(nx * 0.5 + 0.5) * width, (1.0 - (ny * 0.5 + 0.5)) * height];
            cands.push(((p - eye).length(), px, i));
        }
        cands.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut placed: Vec<[f32; 2]> = Vec::new();
        let mut out: Vec<mesh::LabelVertex> = Vec::new();
        for (_, px, i) in cands {
            if placed.iter().any(|q| (q[0] - px[0]).hypot(q[1] - px[1]) < min_sep) {
                continue;
            }
            placed.push(px);

            let b = &self.world.bodies[i];
            let ox = px[0] + off_x;
            let oy = px[1] - LABEL_PX * 0.5;
            let alt = (self.world.body_position_eci(i).length() - EARTH_RADIUS_KM).max(0.0);
            let (lat, lon) = self.world.body_latlon(i);
            let ns = if lat >= 0.0 { 'N' } else { 'S' };
            let ew = if lon >= 0.0 { 'E' } else { 'W' };
            emit_text(&mut out, &b.name, ox, oy, b.color, unit, width, height);
            emit_text(&mut out, &format!("{alt:.0} KM"), ox, oy + line_h, b.color, unit, width, height);
            emit_text(
                &mut out,
                &format!("{:.1}{ns} {:.1}{ew}", lat.abs(), lon.abs()),
                ox,
                oy + line_h * 2.0,
                b.color,
                unit,
                width,
                height,
            );
            if out.len() + 512 > MAX_LABEL_VERTS {
                break;
            }
        }
        out
    }

    pub fn render(&mut self, egui: Option<EguiFrame>) -> Result<(), wgpu::SurfaceError> {
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

            let body_count = self.world.bodies.len() as u32;

            // --- Far side (behind the globe surface): faint, "through glass" --
            // Coastlines + borders (instanced thick-line quads).
            if self.line_vcount > 0 {
                rp.set_pipeline(&self.lines_back_pipeline);
                rp.set_vertex_buffer(0, self.line_quad_vbuf.slice(..));
                rp.set_vertex_buffer(1, self.line_vbuf.slice(..));
                rp.draw(0..6, 0..self.line_vcount);
            }

            // Orbit tracks.
            if self.track_vcount > 0 && self.settings.show_tracks {
                rp.set_pipeline(&self.track_back_pipeline);
                rp.set_vertex_buffer(0, self.track_vbuf.slice(..));
                rp.draw(0..self.track_vcount, 0..1);
            }

            // Bodies behind the globe.
            if body_count > 0 {
                rp.set_pipeline(&self.markers_back_pipeline);
                rp.set_vertex_buffer(0, self.marker_quad_vbuf.slice(..));
                rp.set_vertex_buffer(1, self.marker_inst_buf.slice(..));
                rp.draw(0..6, 0..body_count);
            }

            // Ground anchors behind the globe (faint, "through the glass").
            if self.ground_count > 0 {
                rp.set_pipeline(&self.ground_back_pipeline);
                rp.set_vertex_buffer(0, self.line_quad_vbuf.slice(..));
                rp.set_vertex_buffer(1, self.ground_inst_buf.slice(..));
                rp.draw(0..6, 0..self.ground_count);
            }

            // --- Near side: full intensity -----------------------------------
            // Translucent land fill, under the coastlines/borders.
            if self.fill_vcount > 0 {
                rp.set_pipeline(&self.fill_pipeline);
                rp.set_vertex_buffer(0, self.fill_vbuf.slice(..));
                rp.draw(0..self.fill_vcount, 0..1);
            }

            // Coastlines + borders (instanced thick-line quads).
            if self.line_vcount > 0 {
                rp.set_pipeline(&self.lines_pipeline);
                rp.set_vertex_buffer(0, self.line_quad_vbuf.slice(..));
                rp.set_vertex_buffer(1, self.line_vbuf.slice(..));
                rp.draw(0..6, 0..self.line_vcount);
            }

            // Ground anchors (nadir drop-lines + footprint rings), near side.
            if self.ground_count > 0 {
                rp.set_pipeline(&self.ground_pipeline);
                rp.set_vertex_buffer(0, self.line_quad_vbuf.slice(..));
                rp.set_vertex_buffer(1, self.ground_inst_buf.slice(..));
                rp.draw(0..6, 0..self.ground_count);
            }

            // Orbit tracks.
            if self.track_vcount > 0 && self.settings.show_tracks {
                rp.set_pipeline(&self.track_pipeline);
                rp.set_vertex_buffer(0, self.track_vbuf.slice(..));
                rp.draw(0..self.track_vcount, 0..1);
            }

            // Atmospheric glow.
            if self.settings.show_atmosphere {
                rp.set_pipeline(&self.atmosphere_pipeline);
                rp.set_vertex_buffer(0, self.sphere_vbuf.slice(..));
                rp.set_index_buffer(self.sphere_ibuf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..self.sphere_icount, 0, 0..1);
            }

            // Bodies in front, drawn last so they read crisply.
            if body_count > 0 {
                rp.set_pipeline(&self.markers_pipeline);
                rp.set_vertex_buffer(0, self.marker_quad_vbuf.slice(..));
                rp.set_vertex_buffer(1, self.marker_inst_buf.slice(..));
                rp.draw(0..6, 0..body_count);
            }

            // HUD labels on top of everything (group 0 = uniforms already set).
            if self.label_vcount > 0 {
                rp.set_pipeline(&self.label_pipeline);
                rp.set_vertex_buffer(0, self.label_vbuf.slice(..));
                rp.draw(0..self.label_vcount, 0..1);
            }
        }

        // [egui] Overlay pass: load (don't clear) the 3D scene, then paint the
        // panels on top. Buffer/texture prep records into the same encoder; any
        // returned user command buffers must run before it.
        let mut egui_cmds = Vec::new();
        if let Some(eg) = &egui {
            let desc = egui_wgpu::ScreenDescriptor {
                size_in_pixels: [self.config.width, self.config.height],
                pixels_per_point: eg.pixels_per_point,
            };
            for (id, delta) in &eg.textures_delta.set {
                self.egui_renderer
                    .update_texture(&self.device, &self.queue, *id, delta);
            }
            egui_cmds = self.egui_renderer.update_buffers(
                &self.device,
                &self.queue,
                &mut enc,
                eg.primitives,
                &desc,
            );

            let mut rp = enc
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
            self.egui_renderer.render(&mut rp, eg.primitives, &desc);
        }

        egui_cmds.push(enc.finish());
        self.queue.submit(egui_cmds);
        frame.present();

        // Free textures egui retired this frame (after submit, per egui's API).
        if let Some(eg) = &egui {
            for id in &eg.textures_delta.free {
                self.egui_renderer.free_texture(id);
            }
        }
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

/// Is `p` (render units) hidden behind the unit globe as seen from `eye`?
fn occluded_by_globe(eye: Vec3, p: Vec3) -> bool {
    let d = p - eye;
    let len = d.length();
    if len < 1e-6 {
        return false;
    }
    let dir = d / len;
    let b = dir.dot(eye);
    let c = eye.length_squared() - 1.0; // globe radius = 1
    let disc = b * b - c;
    if disc <= 0.0 {
        return false; // ray misses the globe
    }
    let t = -b - disc.sqrt(); // near intersection
    t > 1e-3 && t < len - 1e-3 // globe sits between the eye and the body
}

/// Append vector-stroke line vertices for one line of `text`, with its top-left
/// at screen pixel `(ox, oy)`. `unit` is pixels per glyph grid unit.
#[allow(clippy::too_many_arguments)]
/// Draw the fly-camera controls legend across the top of the screen. The font
/// is auto-sized so the whole line fits the viewport width.
fn emit_fly_banner(out: &mut Vec<mesh::LabelVertex>, width: f32, height: f32) {
    const TEXT: &str =
        "FLY  F:EXIT  ARROWS:LOOK  Q/E:ROLL  Z/X:SPEED  G/H:ALT  C/V:INCL  B/N:RAAN";
    const MARGIN: f32 = 20.0;
    const BANNER_PX_MAX: f32 = 16.0;

    // advance per char = (GW + 1) * unit, with unit = px / GH. Solve the unit
    // that makes the line span at most (width - 2*margin), capped at the max.
    let advance_units = (glyphs::GW + 1.0) * TEXT.len() as f32;
    let fit_unit = (width - 2.0 * MARGIN).max(1.0) / advance_units;
    let unit = fit_unit.min(BANNER_PX_MAX / glyphs::GH);
    let color = [0.925, 0.937, 0.957]; // Nord6 snow

    emit_text(out, TEXT, MARGIN, 14.0, color, unit, width, height);
}

fn emit_text(
    out: &mut Vec<mesh::LabelVertex>,
    text: &str,
    ox: f32,
    oy: f32,
    color: [f32; 3],
    unit: f32,
    width: f32,
    height: f32,
) {
    let to_ndc = |px: f32, py: f32| [px / width * 2.0 - 1.0, 1.0 - py / height * 2.0];
    let advance = (glyphs::GW + 1.0) * unit; // glyph cell + one-unit gap
    let mut cx = ox;
    for ch in text.bytes().map(|c| c.to_ascii_uppercase()) {
        for s in glyphs::strokes(ch) {
            let p0 = to_ndc(cx + s[0] as f32 * unit, oy + s[1] as f32 * unit);
            let p1 = to_ndc(cx + s[2] as f32 * unit, oy + s[3] as f32 * unit);
            out.push(mesh::LabelVertex { pos: p0, col: color });
            out.push(mesh::LabelVertex { pos: p1, col: color });
        }
        cx += advance;
    }
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
    fs_entry: &str,
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
            entry_point: fs_entry,
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
