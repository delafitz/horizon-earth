// Atmospheric rim glow. A slightly larger sphere whose contribution is a
// view-dependent fresnel term, additively blended so only the limb glows.

struct U {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    cam_pos: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) nrm: vec3<f32>,
    @location(1) world: vec3<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>) -> VOut {
    var o: VOut;
    let world = pos * 1.15;
    o.world = world;
    o.nrm = normalize(pos);
    o.clip = u.view_proj * vec4<f32>(world, 1.0);
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let N = normalize(in.nrm);
    let V = normalize(u.cam_pos.xyz - in.world);
    let f = pow(1.0 - max(dot(N, V), 0.0), 3.0);
    let col = vec3<f32>(0.506, 0.631, 0.757); // Nord9 #81A1C1
    return vec4<f32>(col * f, f);
}
