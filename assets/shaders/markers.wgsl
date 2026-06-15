// Orbiting-body markers: instanced screen-facing billboards. Each body is a
// small square — filled or outline depending on its category (kind) — kept at a
// roughly constant on-screen size by expanding the quad in clip space.

struct U {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    cam_pos: vec4<f32>,
    params: vec4<f32>, // params.x = viewport aspect (width / height)
    style0: vec4<f32>,
    style1: vec4<f32>,
    style2: vec4<f32>, // z = far-side satellite-artifact alpha
    sun: vec4<f32>,    // xyz = sun direction, w = night brightness floor
};
@group(0) @binding(0) var<uniform> u: U;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) kind: f32,
    @location(3) world: vec3<f32>,
};

@vertex
fn vs_main(
    @location(0) corner: vec2<f32>,        // unit quad corner in [-1,1]
    @location(1) center_size: vec4<f32>,   // world center .xyz, half-size .w
    @location(2) color_kind: vec4<f32>,    // rgb + kind (0=outline, 1=filled)
) -> VOut {
    let center = center_size.xyz;
    let size = center_size.w;

    var c = u.view_proj * vec4<f32>(center, 1.0);
    let aspect = u.params.x;
    c.x += corner.x * size * c.w / aspect;
    c.y += corner.y * size * c.w;

    var o: VOut;
    o.clip = c;
    o.uv = corner;
    o.color = color_kind.xyz;
    o.kind = color_kind.w;
    o.world = center;
    return o;
}

// Surface-laid variant (cities): the quad lies in the tangent plane at `center`,
// oriented by the surface normal (radial), so dots hug the globe and foreshorten
// toward the limb instead of always facing the camera. `size` is in world units.
@vertex
fn vs_surface(
    @location(0) corner: vec2<f32>,
    @location(1) center_size: vec4<f32>,
    @location(2) color_kind: vec4<f32>,
) -> VOut {
    let center = center_size.xyz;
    let size = center_size.w;

    let n = normalize(center); // outward surface normal
    var up = vec3<f32>(0.0, 1.0, 0.0);
    if (abs(n.y) > 0.99) {
        up = vec3<f32>(1.0, 0.0, 0.0);
    }
    let t = normalize(cross(up, n)); // tangent basis in the surface plane
    let b = cross(n, t);
    let world = center + (corner.x * t + corner.y * b) * size;

    var o: VOut;
    o.clip = u.view_proj * vec4<f32>(world, 1.0);
    o.uv = corner;
    o.color = color_kind.xyz;
    o.kind = color_kind.w;
    o.world = world;
    return o;
}

// Distance from point `p` to segment `a`-`b`.
fn seg_dist(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h);
}

// Thin outline of an upward-pointing triangle inscribed in the [-1,1] quad.
fn triangle_alpha(uv: vec2<f32>) -> f32 {
    let a = vec2<f32>(0.0, 0.9);
    let b = vec2<f32>(-0.9, -0.7);
    let c = vec2<f32>(0.9, -0.7);
    let d = min(min(seg_dist(uv, a, b), seg_dist(uv, b, c)), seg_dist(uv, c, a));
    return 1.0 - smoothstep(0.06, 0.20, d);
}

// Marker coverage at corner `uv` for the given kind: 0 = outline box, 1 = filled
// square (solid with a soft edge), 2 = wire triangle, 3 = filled circle (cities).
fn marker_alpha(uv: vec2<f32>, kind: f32) -> f32 {
    if (kind > 2.5) {
        return 1.0 - smoothstep(0.78, 1.0, length(uv)); // filled circle
    }
    if (kind > 1.5) {
        return triangle_alpha(uv);
    }
    let m = max(abs(uv.x), abs(uv.y));
    if (kind > 0.5) {
        return 1.0 - smoothstep(0.82, 1.0, m);
    }
    let outer = 1.0 - smoothstep(0.92, 1.0, m);
    let inner = smoothstep(0.58, 0.72, m);
    return outer * inner;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let a = marker_alpha(in.uv, in.kind);
    if (a < 0.02) {
        discard;
    }
    return vec4<f32>(in.color, a);
}

// Far side (behind the translucent globe): dimmer, seen "through the glass".
@fragment
fn fs_back(in: VOut) -> @location(0) vec4<f32> {
    let a = marker_alpha(in.uv, in.kind) * u.style2.z;
    if (a < 0.02) {
        discard;
    }
    return vec4<f32>(in.color * 0.7, a);
}

// City markers: city lights — full bright on the night side of the terminator,
// faded in daylight (the inverse of how the rest of the ground dims).
@fragment
fn fs_city(in: VOut) -> @location(0) vec4<f32> {
    let a = marker_alpha(in.uv, in.kind) * u.style2.w;
    if (a < 0.02) {
        discard;
    }
    let day = smoothstep(-0.12, 0.12, dot(normalize(in.world), u.sun.xyz));
    let lit = mix(1.0, 0.4, day); // night = full glow, day = faded
    return vec4<f32>(in.color * lit, a);
}
