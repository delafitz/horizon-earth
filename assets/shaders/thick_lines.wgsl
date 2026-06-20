// Coastlines and country borders as thick, variable-width lines. WebGPU has no
// line-width control, so each segment is drawn as an instanced quad expanded in
// screen space: the vertex shader offsets the endpoints perpendicular to the
// segment by a constant pixel width, independent of camera distance.

struct U {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    cam_pos: vec4<f32>,
    params: vec4<f32>, // x = aspect, y = viewport width px, z = height px
    // [egui] x = far-side line alpha, ..., w = line brightness.
    style0: vec4<f32>,
    // [egui] z = coastline width (px), w = border width (px).
    style1: vec4<f32>,
    // [egui] x = ground-line width (px), y = ground-line alpha.
    style2: vec4<f32>,
    sun: vec4<f32>, // xyz = sun direction, w = night brightness floor
    // [egui] z = reference-graticule width (px).
    style3: vec4<f32>,
    // [egui] y = far-side ground-effects alpha.
    style4: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) col: vec3<f32>,
    @location(1) world: vec3<f32>,
};

// Day/night dimming factor for a surface point (night floor -> full at the
// terminator), shared by the near-side fragment entries.
fn shade(world: vec3<f32>) -> f32 {
    return mix(u.sun.w, 1.0, smoothstep(-0.12, 0.12, dot(normalize(world), u.sun.xyz)));
}

@vertex
fn vs_main(
    @location(0) corner: vec2<f32>,    // x = t (0 at p0, 1 at p1), y = side (-1/+1)
    @location(1) p0: vec3<f32>,
    @location(2) p1: vec3<f32>,
    @location(3) col_layer: vec4<f32>, // rgb + layer (0 = coast, 1 = border)
) -> VOut {
    let w0 = (u.model * vec4<f32>(p0, 1.0)).xyz;
    let w1 = (u.model * vec4<f32>(p1, 1.0)).xyz;
    let c0 = u.view_proj * vec4<f32>(w0, 1.0);
    let c1 = u.view_proj * vec4<f32>(w1, 1.0);

    // Endpoints in pixel-proportional space (NDC * viewport). The constant
    // factor cancels in normalize, leaving a unit pixel-space direction.
    let res = vec2<f32>(u.params.y, u.params.z);
    let s0 = (c0.xy / c0.w) * res;
    let s1 = (c1.xy / c1.w) * res;
    var d = s1 - s0;
    if (dot(d, d) < 1e-9) {
        d = vec2<f32>(1.0, 0.0); // degenerate segment guard
    }
    let dir = normalize(d);
    let perp = vec2<f32>(-dir.y, dir.x);

    // layer: 0 = coastline, 1 = border, 2 = ground anchor, 3 = reference graticule.
    var width_px = u.style1.z;
    if (col_layer.w > 2.5) {
        width_px = u.style3.z;
    } else if (col_layer.w > 1.5) {
        width_px = u.style2.x;
    } else if (col_layer.w > 0.5) {
        width_px = u.style1.w;
    }
    var c = select(c0, c1, corner.x > 0.5);
    // Pixel offset -> NDC (componentwise 2/res), carried into clip by * c.w.
    let offset_ndc = perp * corner.y * (width_px * 0.5) * 2.0 / res;
    c = vec4<f32>(c.xy + offset_ndc * c.w, c.z, c.w);

    var o: VOut;
    o.clip = c;
    o.col = col_layer.xyz;
    o.world = select(w0, w1, corner.x > 0.5);
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    // Alpha carries the detail-tier cross-fade weight (params.w): the renderer
    // draws the low tier with 1-blend and the high tier with blend so they
    // dissolve into each other as the camera crosses the LOD distance.
    return vec4<f32>(in.col * u.style0.w * shade(in.world), u.params.w);
}

// Far-hemisphere variant: faint, alpha-blended (seen "through the glass").
@fragment
fn fs_back(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.col * u.style0.w, u.style0.x * u.params.w);
}

// Ground anchors (nadir lines + footprint rings): alpha-blended in the body's
// category colour. Near pass at full ground alpha, far pass dimmed.
@fragment
fn fs_ground(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.col * shade(in.world), u.style2.y);
}

@fragment
fn fs_ground_back(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.col, u.style2.y * u.style4.y);
}
