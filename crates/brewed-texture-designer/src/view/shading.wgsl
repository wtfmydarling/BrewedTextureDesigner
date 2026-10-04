// Making a material ready for the 3D view (shading.rs): its uniform maps read into the row of
// values, a pixel each; the maps past the shader's slots copied into the layers of one
// texture, and halved for their mips.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// A triangle over the whole target (or the one pixel of the viewport).
@vertex
fn full(@builtin(vertex_index) i: u32) -> Out {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Out;
    o.clip = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

@fragment
fn gather_color(i: Out) -> @location(0) vec4<f32> {
    return textureLoad(src, vec2<i32>(0, 0), 0);
}

@fragment
fn gather_gray(i: Out) -> @location(0) vec4<f32> {
    return widen(textureLoad(src, vec2<i32>(0, 0), 0), true);
}

@fragment
fn copy_color(i: Out) -> @location(0) vec4<f32> {
    return textureSample(src, src_samp, i.uv);
}

@fragment
fn copy_gray(i: Out) -> @location(0) vec4<f32> {
    return widen(textureSample(src, src_samp, i.uv), true);
}

// Between four pixels of the level above: their mean.
@fragment
fn halve(i: Out) -> @location(0) vec4<f32> {
    return textureSampleLevel(src, src_samp, i.uv, 0.0);
}
