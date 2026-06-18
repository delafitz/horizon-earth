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
