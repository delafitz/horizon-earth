// Solid globe surface. Renders an opaque sphere with simple directional
// shading so the vector overlays read as sitting on a 3D ball.

struct U {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    cam_pos: vec4<f32>,
    params: vec4<f32>,
    style0: vec4<f32>,
    style1: vec4<f32>,
    style2: vec4<f32>,
    sun: vec4<f32>, // xyz = sun direction, w = night brightness floor
};
@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var land_mask: texture_2d<f32>;
@group(1) @binding(1) var land_samp: sampler;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) nrm: vec3<f32>,
    @location(1) geo: vec3<f32>, // earth-fixed (pre-model) normal, for lon/lat
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @location(1) nrm: vec3<f32>) -> VOut {
    var o: VOut;
    let world = (u.model * vec4<f32>(pos, 1.0)).xyz;
    o.nrm = normalize((u.model * vec4<f32>(nrm, 0.0)).xyz);
    o.geo = normalize(nrm);
    o.clip = u.view_proj * vec4<f32>(world, 1.0);
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let N = normalize(in.nrm);
    let L = normalize(u.sun.xyz);
    // Day/night terminator: night floor (sun.w) -> full across a soft band.
    let lit = mix(u.sun.w, 1.0, smoothstep(-0.12, 0.12, dot(N, L)));
    let base = vec3<f32>(0.231, 0.259, 0.322); // Nord1 #3B4252

    // Land fill from the equirectangular mask (earth-fixed lon/lat -> UV).
    let g = normalize(in.geo);
    let lon = atan2(-g.z, g.x);
    let lat = asin(clamp(g.y, -1.0, 1.0));
    let uv = vec2<f32>(lon / 6.2831853 + 0.5, 0.5 - lat / 3.14159265);
    let land = textureSample(land_mask, land_samp, uv).r;
    let land_col = vec3<f32>(0.263, 0.298, 0.369); // Nord3-ish land tint
    let surface = mix(base, land_col, land);
    // Ocean stays translucent (0.35); land opacity rides the fill slider so it
    // can read as solid. style0.y is the land-fill opacity (0..1).
    let alpha = mix(0.35, 0.95, land * u.style0.y);

    return vec4<f32>(surface * lit, alpha);
}
