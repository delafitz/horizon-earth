// Bakes the land mask: country polygons rasterised in an equirectangular
// projection (lon -> x, lat -> y, already in clip space) into an R8 texture,
// sampled later on the globe so land fill conforms to the sphere with no
// triangulation seams.

@vertex
fn vs_main(@location(0) pos: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4<f32>(pos, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0, 0.0, 0.0, 1.0); // R = land
}

// --- Mipmap generation: downsample the previous level with a fullscreen pass ---
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;

struct BlitOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_blit(@builtin(vertex_index) vi: u32) -> BlitOut {
    // Oversized fullscreen triangle.
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    var o: BlitOut;
    o.uv = vec2<f32>(x, y);
    o.clip = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    return o;
}

@fragment
fn fs_blit(in: BlitOut) -> @location(0) vec4<f32> {
    return textureSample(src, src_samp, in.uv);
}
