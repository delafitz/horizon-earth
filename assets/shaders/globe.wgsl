// Solid globe surface. Renders an opaque sphere with simple directional
// shading so the vector overlays read as sitting on a 3D ball.

struct U {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    cam_pos: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) nrm: vec3<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @location(1) nrm: vec3<f32>) -> VOut {
    var o: VOut;
    let world = (u.model * vec4<f32>(pos, 1.0)).xyz;
    o.nrm = normalize((u.model * vec4<f32>(nrm, 0.0)).xyz);
    o.clip = u.view_proj * vec4<f32>(world, 1.0);
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let N = normalize(in.nrm);
    let L = normalize(vec3<f32>(0.5, 0.6, 0.5));
    let d = max(dot(N, L), 0.0);
    let base = vec3<f32>(0.231, 0.259, 0.322); // Nord1 #3B4252
    let col = base * (0.30 + 0.70 * d);
    return vec4<f32>(col, 0.45);
}
