// Coastlines and country borders, projected onto the sphere and drawn as
// line segments. Colour is carried per-vertex so one pipeline serves both.

struct U {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    cam_pos: vec4<f32>,
    params: vec4<f32>,
    // [egui] x = far-side line alpha, y = land fill alpha, z = track alpha,
    // w = line brightness.
    style0: vec4<f32>,
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
    return vec4<f32>(in.col * u.style0.w, 1.0);
}

// Far-hemisphere variant: faint, alpha-blended so the back of the globe's
// coastlines/borders read in darker contrast through the translucent surface.
@fragment
fn fs_back(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.col * u.style0.w, u.style0.x);
}

// Land fill: the closed country rings drawn as triangles at low opacity, giving
// the continents a faint translucent body under the coastlines.
//
// The triangles are flat chords inscribed in the sphere, so a large triangle's
// interior dips below the surface. Rather than depth-test against the globe
// (which would clip those sagging interiors and leave only the edges filled),
// we draw with depth testing off and discard the far hemisphere per fragment.
struct FillOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) col: vec3<f32>,
    @location(1) world: vec3<f32>,
};

@vertex
fn vs_fill(@location(0) pos: vec3<f32>, @location(1) col: vec3<f32>) -> FillOut {
    var o: FillOut;
    let world = (u.model * vec4<f32>(pos, 1.0)).xyz;
    o.clip = u.view_proj * vec4<f32>(world, 1.0);
    o.col = col;
    o.world = world;
    return o;
}

@fragment
fn fs_fill(in: FillOut) -> @location(0) vec4<f32> {
    // Keep only the hemisphere facing the camera (outward normal toward eye).
    let n = normalize(in.world);
    let v = normalize(u.cam_pos.xyz - in.world);
    if (dot(n, v) <= 0.0) {
        discard;
    }
    return vec4<f32>(in.col, u.style0.y);
}
