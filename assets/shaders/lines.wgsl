// Coastlines and country borders, projected onto the sphere and drawn as
// line segments. Colour is carried per-vertex so one pipeline serves both.

struct U {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    cam_pos: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) col: vec3<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @location(1) col: vec3<f32>) -> VOut {
    var o: VOut;
    let world = (u.model * vec4<f32>(pos, 1.0)).xyz;
    o.clip = u.view_proj * vec4<f32>(world, 1.0);
    o.col = col;
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.col, 1.0);
}

// Far-hemisphere variant: faint, alpha-blended so the back of the globe's
// coastlines/borders read in darker contrast through the translucent surface.
@fragment
fn fs_back(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.col, 0.28);
}
