// Draws the picture over the pass's whole viewport. Where the viewport is the picture's size, each pixel is
// sampled at its texel's centre, so the picture's pixels arrive unchanged.

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var picture_sampler: sampler;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vertex(@builtin(vertex_index) i: u32) -> Out {
    // One triangle covering the viewport.
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: Out;
    out.position = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(p.x, 1.0 - p.y);
    return out;
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    return textureSample(picture, picture_sampler, in.uv);
}
