// Orbiting-body markers: instanced screen-facing billboards drawn as soft
// round dots. One instance per body; the quad is expanded in clip space so
// each dot keeps a roughly constant on-screen size regardless of distance.

struct U {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    cam_pos: vec4<f32>,
    params: vec4<f32>, // params.x = viewport aspect (width / height)
};
@group(0) @binding(0) var<uniform> u: U;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
};

@vertex
fn vs_main(
    @location(0) corner: vec2<f32>,        // unit quad corner in [-1,1]
    @location(1) center_size: vec4<f32>,   // world center .xyz, half-size .w
    @location(2) color: vec3<f32>,
) -> VOut {
    let center = center_size.xyz;
    let size = center_size.w;

    var c = u.view_proj * vec4<f32>(center, 1.0);
    let aspect = u.params.x;
    // Offset in clip space, scaled by w so the perspective divide yields a
    // constant NDC (hence constant pixel) size; divide x by aspect to stay round.
    c.x += corner.x * size * c.w / aspect;
    c.y += corner.y * size * c.w;

    var o: VOut;
    o.clip = c;
    o.uv = corner;
    o.color = color;
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let d = length(in.uv);
    if (d > 1.0) {
        discard;
    }
    // Bright core fading to a soft edge.
    let a = smoothstep(1.0, 0.55, d);
    let core = 0.6 + 0.4 * smoothstep(0.5, 0.0, d);
    return vec4<f32>(in.color * core, a);
}
