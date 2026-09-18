//! Graph outputs.

use super::*;

pub const OUTPUT_FORMATS: &[&str] = &["RGBA", "RGB", "RG", "R", "G", "B", "A"];

/// Channels (indices into RGBA) written for each output format.
pub fn output_format_channels(format: u32) -> &'static [usize] {
    match format {
        0 => &[0, 1, 2, 3],
        1 => &[0, 1, 2],
        2 => &[0, 1],
        3 => &[0],
        4 => &[1],
        5 => &[2],
        _ => &[3],
    }
}

pub static OUTPUT: NodeDef = NodeDef {
    id: "output",
    name: "Output",
    category: Category::Output,
    description: "A graph output that is exported to an image file.",
    keywords: "export result texture save",
    inputs: IMAGE_IN,
    outputs: NO_SOCKETS,
    params: &[
        text("name", "Name", "output").tip("File name of the exported image (without extension)."),
        choice("format", "Format", 0, OUTPUT_FORMATS).fixed().tip("Channels written by the export."),
        choice("bit_depth", "Bit Depth", 0, &["8 bit", "16 bit", "32 bit float"])
            .fixed()
            .tip("8 and 16 bit are written as PNG, 32 bit float as OpenEXR."),
        choice(
            "usage",
            "Usage",
            0,
            &["Custom", "Base Color", "Normal", "Height", "Roughness", "Metallic", "Ambient Occlusion", "Emissive"],
        )
        .fixed()
        .tip("What the texture is used for in the 3D view."),
    ],
    role: Role::Output,
};

/// Output usages known to the 3D view, in the order of the "usage" parameter.
pub const USAGE_BASE_COLOR: u32 = 1;
pub const USAGE_NORMAL: u32 = 2;
pub const USAGE_HEIGHT: u32 = 3;
pub const USAGE_ROUGHNESS: u32 = 4;
pub const USAGE_METALLIC: u32 = 5;
pub const USAGE_AO: u32 = 6;
pub const USAGE_EMISSIVE: u32 = 7;
