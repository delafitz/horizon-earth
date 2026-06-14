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
};
@group(0) @binding(0) var<uniform> u: U;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) col: vec3<f32>,
};

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

    // layer: 0 = coastline, 1 = border, 2 = ground anchor.
    var width_px = u.style1.z;
    if (col_layer.w > 1.5) {
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
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.col * u.style0.w, 1.0);
}

// Far-hemisphere variant: faint, alpha-blended (seen "through the glass").
@fragment
fn fs_back(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.col * u.style0.w, u.style0.x);
}

// Ground anchors (nadir lines + footprint rings): alpha-blended in the body's
// category colour. Near pass at full ground alpha, far pass dimmed.
@fragment
fn fs_ground(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.col, u.style2.y);
}

@fragment
fn fs_ground_back(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.col, u.style2.y * u.style2.z);
}
