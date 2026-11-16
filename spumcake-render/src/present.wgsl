// Writes the finished colours into the picture a window draws.

struct Params {
    width: u32,
    height: u32,
    buckets_x: u32,
    background: u32,
    half_width: f32,
    half_height: f32,
    focal: f32,
    edge: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> colour: array<u32>;
@group(0) @binding(2) var picture: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8)
fn present(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x;
    let y = id.y;
    if (x >= params.width || y >= params.height) {
        return;
    }
    let c = colour[y * params.width + x];
    let rgb = vec3<f32>(f32(c & 255u), f32((c >> 8u) & 255u), f32((c >> 16u) & 255u)) / 255.0;
    textureStore(picture, vec2<i32>(i32(x), i32(y)), vec4<f32>(rgb, 1.0));
}
