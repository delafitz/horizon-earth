// HUD text labels: vector stroke font drawn as screen-space lines. Positions
// arrive already in NDC, so this is a straight pass-through.

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) col: vec3<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) col: vec3<f32>) -> VOut {
    var o: VOut;
    o.clip = vec4<f32>(pos, 0.0, 1.0);
    o.col = col;
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.col, 0.9);
}
