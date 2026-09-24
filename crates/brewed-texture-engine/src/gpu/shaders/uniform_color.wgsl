struct Params {
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> P: Params;

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    return P.color;
}
