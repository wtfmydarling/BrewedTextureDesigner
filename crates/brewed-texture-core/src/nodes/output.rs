//! Graph outputs.

use super::*;

pub const OUTPUT_FORMATS: &[&str] = &["RGBA", "RGB", "RG", "R", "G", "B", "A"];

/// How an Output writes its file (the "bit_depth" parameter), as Substance Designer's outputs
/// offer: PNG of 8 or 16 bits, OpenEXR of half floats or floats, and DDS compressed as games
/// read textures (BC1 to BC3, of 8 bits; DXT2 and DXT4 with the colors multiplied by the alpha).
pub const OUTPUT_DEPTHS: &[&str] = &[
    "8 bits",
    "16 bits",
    "16 bits float",
    "32 bits float",
    "DXT1 / BC1",
    "DXT2 / BC2",
    "DXT3 / BC2",
    "DXT4 / BC3",
    "DXT5 / BC3",
];
pub const DEPTH_8: u32 = 0;
pub const DEPTH_16: u32 = 1;
pub const DEPTH_16F: u32 = 2;
pub const DEPTH_32F: u32 = 3;
pub const DEPTH_DXT1: u32 = 4;
pub const DEPTH_DXT2: u32 = 5;
pub const DEPTH_DXT3: u32 = 6;
pub const DEPTH_DXT4: u32 = 7;
pub const DEPTH_DXT5: u32 = 8;

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
        choice("format", "Channels", 0, OUTPUT_FORMATS).fixed().tip("Channels written by the export."),
        choice("bit_depth", "Format", 0, OUTPUT_DEPTHS).fixed().tip(
            "How the file is written: 8 and 16 bits as PNG, 16 and 32 bits float as OpenEXR, and DXT (BC) as DDS, \
             compressed as game engines read textures, with its mipmaps. The output keeps its image as it writes it, \
             as the 2D and 3D views show it.",
        ),
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
