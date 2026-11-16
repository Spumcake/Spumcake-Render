// Settles each pixel as the pre-vis builders do: the faces covering it are tried in drawing order, and a face
// wins only where it is nearer than the one already there by more than 1e-4 m. edges.wgsl then draws the edges.

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

struct Face {
    normal_x: f32,
    normal_y: f32,
    normal_z: f32,
    plane: f32,
    colour: u32,
    object: u32,
    pad0: u32,
    pad1: u32,
}

struct Run {
    x0: i32,
    x1: i32,
    face: u32,
}

const NONE: u32 = 0xffffffffu;
const FAR: f32 = 3.0e38;
const BUCKET: u32 = 32u;

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> faces: array<Face>;
@group(0) @binding(2) var<storage, read> runs: array<Run>;
@group(0) @binding(3) var<storage, read> bucket_start: array<u32>;
@group(0) @binding(4) var<storage, read> items: array<u32>;
@group(0) @binding(5) var<storage, read_write> objects: array<u32>;
@group(0) @binding(6) var<storage, read_write> depth: array<f32>;
@group(0) @binding(7) var<storage, read_write> colour: array<u32>;

@compute @workgroup_size(8, 8)
fn resolve(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x;
    let y = id.y;
    if (x >= params.width || y >= params.height) {
        return;
    }
    let rx = (f32(x) + 0.5 - params.half_width) / params.focal;
    let ry = -(f32(y) + 0.5 - params.half_height) / params.focal;
    let bucket = y * params.buckets_x + x / BUCKET;
    var nearest = FAR;
    var object = NONE;
    var shade = params.background;
    for (var i = bucket_start[bucket]; i < bucket_start[bucket + 1u]; i = i + 1u) {
        let run = runs[items[i]];
        if (i32(x) < run.x0 || i32(x) > run.x1) {
            continue;
        }
        let face = faces[run.face];
        let den = face.normal_x * rx + face.normal_y * ry + face.normal_z;
        if (den == 0.0) {
            continue;
        }
        let z = face.plane / den;
        if (z > 0.0 && z < nearest - 1e-4) {
            nearest = z;
            object = face.object;
            shade = face.colour;
        }
    }
    let k = y * params.width + x;
    objects[k] = object;
    depth[k] = nearest;
    colour[k] = shade;
}
