// Atmospheric glow. A slightly larger sphere shell, additively blended. The
// glow is brightest hugging the globe's surface and gradients out to fully
// transparent at the shell's outer edge — driven by the view ray's closest
// approach to the globe centre (the "impact parameter") so it stays put as the
// camera orbits and zooms.

struct U {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    cam_pos: vec4<f32>,
    params: vec4<f32>,
    style0: vec4<f32>,
    // [egui] x = glow intensity, y = outer shell radius (atmosphere "depth").
    style1: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;

// Globe radius; the atmosphere's outer reach is UI-driven (u.style1.y).
const SURFACE: f32 = 1.0;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) nrm: vec3<f32>,
    @location(1) world: vec3<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>) -> VOut {
    var o: VOut;
    let world = pos * u.style1.y;
    o.world = world;
    o.nrm = normalize(pos);
    o.clip = u.view_proj * vec4<f32>(world, 1.0);
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let N = normalize(in.nrm);
    let to_cam = normalize(u.cam_pos.xyz - in.world);
    // Only the near hemisphere of the shell contributes (avoids a doubled glow
    // from the far side adding through).
    if (dot(N, to_cam) <= 0.0) {
        discard;
    }

    // Impact parameter: how close the view ray through this fragment passes to
    // the globe centre. ~SURFACE at the limb, up to OUTER at the shell edge.
    let ray = normalize(in.world - u.cam_pos.xyz);
    let b = length(cross(in.world, ray));

    // Glow hugs the surface (bright near b = SURFACE), ramps in gently just
    // inside the limb, and fades to transparent at the outer edge.
    let inner = smoothstep(0.985, SURFACE, b);
    let fade = 1.0 - smoothstep(SURFACE, u.style1.y, b);
    let glow = inner * fade * u.style1.x;

    let col = vec3<f32>(0.506, 0.631, 0.757); // Nord9 #81A1C1
    return vec4<f32>(col * glow, glow);
}
