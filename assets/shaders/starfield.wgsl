// Fullscreen starfield background. Draws a single oversized triangle and
// scatters sparse stars over the Nord background colour.

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VOut {
    var o: VOut;
    // Oversized triangle covering the whole screen.
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    o.uv = vec2<f32>(x, y);
    o.clip = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    return o;
}

fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(123.34, 345.45));
    q += dot(q, q + 34.345);
    return fract(q.x * q.y);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let bg = vec3<f32>(0.180, 0.204, 0.251); // Nord0 #2E3440
    let g = in.uv * 80.0;
    let cell = floor(g);
    let fpos = fract(g);
    let center = vec2<f32>(hash21(cell), hash21(cell + 1.7));
    let d = distance(fpos, center);
    let present = step(0.90, hash21(cell + 5.1));
    let bright = smoothstep(0.06, 0.0, d) * present * (0.3 + 0.7 * hash21(cell + 9.3));
    return vec4<f32>(bg + vec3<f32>(bright), 1.0);
}
