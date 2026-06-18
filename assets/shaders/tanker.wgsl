// Tanker markers + tracks: flat geometry sitting just above the globe surface,
// built in the Earth-fixed frame and spun by the GMST `model` matrix like the
// coastlines. Each moving tanker is a small triangle pointing along its course;
// stationary ones are a small rect. Colour is carried per-vertex.
//
// Only `view_proj` and `model` are read, so this declares just that prefix of
// the shared uniform buffer.

struct U {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
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
    return vec4<f32>(in.col, 0.5);
}

// Tracks: same geometry path, drawn fainter still.
@fragment
fn fs_track(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.col, 0.35);
}
