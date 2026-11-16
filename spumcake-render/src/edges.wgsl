// A pixel whose object differs from the pixel to its left or the one above becomes an edge, as in the builders.

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
@group(0) @binding(1) var<storage, read> objects: array<u32>;
@group(0) @binding(2) var<storage, read_write> colour: array<u32>;

@compute @workgroup_size(8, 8)
fn edges(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x;
    let y = id.y;
    if (x >= params.width || y >= params.height) {
        return;
    }
    let k = y * params.width + x;
    let o = objects[k];
    if ((x > 0u && objects[k - 1u] != o) || (y > 0u && objects[k - params.width] != o)) {
        colour[k] = params.edge;
    }
}
