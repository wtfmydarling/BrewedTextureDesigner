// How a node's image is shown: by the kernels, which read their inputs through it, and by
// the designer's 2D and 3D views, which sample the node images themselves.

// A grayscale image is one channel: it is shown as gray (the channel in red, green and
// blue), and opaque.
fn widen(c: vec4<f32>, gray: bool) -> vec4<f32> {
    return select(c, vec4<f32>(c.r, c.r, c.r, 1.0), gray);
}

// What an output shows of an image in each of its formats
// (brewed_texture_core::nodes::OUTPUT_FORMATS: RGBA, RGB, RG, R, G, B, A).
fn as_output(c: vec4<f32>, format: u32) -> vec4<f32> {
    switch format {
        case 1u: { return vec4<f32>(c.rgb, 1.0); }
        case 2u: { return vec4<f32>(c.r, c.g, 0.0, 1.0); }
        case 3u: { return vec4<f32>(c.rrr, 1.0); }
        case 4u: { return vec4<f32>(c.ggg, 1.0); }
        case 5u: { return vec4<f32>(c.bbb, 1.0); }
        case 6u: { return vec4<f32>(c.aaa, 1.0); }
        default: { return c; }
    }
}
