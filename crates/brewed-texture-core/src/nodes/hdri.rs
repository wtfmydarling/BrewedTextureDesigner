//! Nodes that make environments: panoramas of all around (across, once round; down, from
//! straight up to straight down, whatever the image's shape), as Substance Designer's HDRI tools make
//! them: a gradient from the sky to the floor, and lights — a softbox, a tube, a panel — each
//! drawn where it is in the scene, as the panorama sees it (round where it is far off its
//! middle), over the panorama it is given. Light is in exposure values (each one doubles it)
//! and in a color or a color temperature.
//!
//! Where a light is is a point of the panorama: across (0 to 1, once round; its middle
//! straight ahead) and down (0 straight up, 1 straight down).

use super::*;

/// The colors a light can be given in.
pub const LIGHT_COLOR_MODES: &[&str] = &["RGB", "Temperature"];
const LIGHT_COLOR_MODE_IDS: &[&str] = &["rgb", "temperature"];
pub const LIGHT_COLOR_TEMPERATURE: u32 = 1;

/// The shapes of a Shape Light.
pub const SHAPE_LIGHT_SHAPES: &[&str] = &["Rectangle", "Disc"];
const SHAPE_LIGHT_SHAPE_IDS: &[&str] = &["rectangle", "disc"];

/// Where a Line Light is, as Substance Designer's: on a ceiling or a floor, or as far from the
/// middle of the scene every way.
pub const LINE_LIGHT_POSITIONS: &[&str] = &["Ground / Ceiling", "Distance from Origin"];
const LINE_LIGHT_POSITION_IDS: &[&str] = &["ground_ceiling", "distance"];

/// How a Line Light is lit across: from its middle to its edges.
pub const LINE_LIGHT_PATTERNS: &[&str] = &["Smooth Square", "Sharp Square", "Cone", "Hemisphere"];
const LINE_LIGHT_PATTERN_IDS: &[&str] = &["smooth_square", "sharp_square", "cone", "hemisphere"];

/// Where a Plane Light is: facing the middle of the scene, or a ceiling or a floor.
pub const PLANE_LIGHT_PLACEMENTS: &[&str] = &["Facing the Center", "Ceiling", "Floor"];
const PLANE_LIGHT_PLACEMENT_IDS: &[&str] = &["facing", "ceiling", "floor"];

/// What a Plane Light is lit as: all over, in stripes, in a grid of cells.
pub const PLANE_LIGHT_PATTERNS: &[&str] = &["Plain", "Stripes", "Grid"];
const PLANE_LIGHT_PATTERN_IDS: &[&str] = &["plain", "stripes", "grid"];

const BACKGROUND: &[SocketDef] = &[color_image_opt("background", "Background")];

pub static HDRI_GRADIENT: NodeDef = NodeDef {
    id: "hdri_gradient",
    name: "Gradient Linear (HDRI)",
    category: Category::Hdri,
    description: "A panorama from one color, where a point of it is, to another, opposite: a sky over a floor, as the sphere has it.",
    keywords: "hdri environment panorama sky floor gradient linear studio light",
    inputs: NO_SOCKETS,
    outputs: COLOR_OUT,
    params: &[
        vec2("point", "Point", [0.5, 0.0], 0.0, 1.0)
            .tip("Where the Top Color is: across, once round (0.5 straight ahead); down, from straight up (0) to straight down (1)."),
        // (White to black: which color is where is plain to see.)
        color("top", "Top Color", [1.0, 1.0, 1.0, 1.0]).tip("The color at the point."),
        color("bottom", "Bottom Color", [0.0, 0.0, 0.0, 1.0]).tip("The color opposite it."),
        float("contrast", "Contrast", 0.5, 0.0, 1.0)
            .bounded()
            .tip("How sharply one color turns into the other: 0 all the way round, 1 at once, at the middle."),
        float("exposure", "Exposure (EV)", 0.0, -10.0, 10.0).tip("How bright both colors are: each step doubles them."),
    ],
    role: Role::Image,
};

pub static HDRI_SHAPE_LIGHT: NodeDef = NodeDef {
    id: "hdri_shape_light",
    name: "Shape Light",
    category: Category::Hdri,
    description: "A softbox: a rectangle or a disc of light facing the middle of the scene, with a hotspot, drawn over the panorama it is given.",
    keywords: "hdri environment panorama softbox rectangle disc light studio hotspot",
    inputs: BACKGROUND,
    outputs: COLOR_OUT,
    params: &[
        choice("shape", "Shape", 0, SHAPE_LIGHT_SHAPES, SHAPE_LIGHT_SHAPE_IDS),
        vec2("position", "Position", [0.5, 0.25], 0.0, 1.0)
            .tip("Where its middle is: across, once round (0.5 straight ahead); down, from straight up (0) to straight down (1)."),
        vec2("size", "Size", [0.3, 0.2], 0.0, 2.0).tip("How wide and high it is, from its middle, as far as it is from the middle of the scene."),
        angle("rotation", "Rotation", 0.0, -180.0, 180.0).tip("Degrees, counter-clockwise, about the line from it to the middle."),
        float("hardness", "Hardness", 0.6, 0.0, 1.0).bounded().tip("How sharp its edges are: 0 soft all the way in, 1 a hard line."),
        choice("color_mode", "Color Mode", 0, LIGHT_COLOR_MODES, LIGHT_COLOR_MODE_IDS).fixed(),
        color("color", "Color", [1.0, 1.0, 1.0, 1.0]).when("color_mode", &[0]),
        float("temperature", "Temperature (K)", 6500.0, 1700.0, 20000.0)
            .clamped(1000.0, 40000.0)
            .when("color_mode", &[1])
            .tip("The color of a light this hot, in kelvins: 2700 a bulb, 5500 the sun at noon, 6500 daylight, 10000 a blue sky."),
        float("exposure", "Exposure (EV)", 2.0, 0.0, 10.0).tip("How bright it is: 0 as bright as white, each step twice as bright."),
        float("hotspot_exposure", "Hotspot Exposure (EV)", 4.0, 0.0, 10.0).tip("How bright its hotspot is."),
        float("hotspot_size", "Hotspot Size", 0.5, 0.0, 1.0).bounded().tip("How much of the light the hotspot is: 0 none, 1 all of it."),
        float("hotspot_falloff", "Hotspot Falloff", 0.5, 0.0, 1.0).bounded().tip("How gently the hotspot fades into the rest: 0 at once, 1 all the way."),
        vec2("hotspot_position", "Hotspot Position", [0.0, 0.0], -1.0, 1.0).tip("Where the hotspot is in the light: from edge (-1) to edge (1)."),
    ],
    role: Role::Image,
};

pub static HDRI_LINE_LIGHT: NodeDef = NodeDef {
    id: "hdri_line_light",
    name: "Line Light",
    category: Category::Hdri,
    description: "A tube of light from one point of the panorama to another (a strip light, a fluorescent tube), where it is in the scene: thicker where it is nearer. Drawn over the panorama it is given.",
    keywords: "hdri environment panorama tube strip fluorescent line light studio",
    inputs: BACKGROUND,
    outputs: COLOR_OUT,
    params: &[
        vec2("point_1", "Point 1", [0.4, 0.3], 0.0, 1.0).tip("Where it begins: across, once round; down, from straight up (0) to straight down (1)."),
        vec2("point_2", "Point 2", [0.6, 0.3], 0.0, 1.0).tip("Where it ends."),
        // (A mode, as Color Mode is: what the other parameters mean, set rather than given by a
        // socket.)
        choice("position_mode", "Position Mode", 0, LINE_LIGHT_POSITIONS, LINE_LIGHT_POSITION_IDS).fixed().tip(
            "Where its ends are in the scene: on a ceiling a step above the middle of the scene (where they are seen above \
             the horizon) or on a floor a step below it, so that it is nearer and thicker overhead and thinner toward the \
             horizon, as a tube on a ceiling is; or both a step from the middle.",
        ),
        float("thickness", "Thickness", 0.05, 0.0, 0.5)
            .at_least(0.0)
            .tip("How thick it is (its radius), as far as the ceiling is above the middle of the scene: it looks thicker where it is nearer."),
        choice("pattern", "Pattern", 0, LINE_LIGHT_PATTERNS, LINE_LIGHT_PATTERN_IDS)
            .tip("How it is lit across: evenly to soft edges, evenly to sharp ones, brightest along its middle (a cone), or as a round tube (a hemisphere)."),
        float("hardness", "Hardness", 0.5, 0.0, 1.0).bounded().tip("How sharp its edges are."),
        choice("color_mode", "Color Mode", 0, LIGHT_COLOR_MODES, LIGHT_COLOR_MODE_IDS).fixed(),
        color("color", "Color", [1.0, 1.0, 1.0, 1.0]).when("color_mode", &[0]),
        float("temperature", "Temperature (K)", 6500.0, 1700.0, 20000.0)
            .clamped(1000.0, 40000.0)
            .when("color_mode", &[1])
            .tip("The color of a light this hot, in kelvins: 2700 a bulb, 6500 daylight."),
        float("exposure", "Exposure (EV)", 3.0, 0.0, 10.0).tip("How bright it is: 0 as bright as white, each step twice as bright."),
    ],
    role: Role::Image,
};

pub static HDRI_PLANE_LIGHT: NodeDef = NodeDef {
    id: "hdri_plane_light",
    name: "Plane Light",
    category: Category::Hdri,
    description: "A panel of light: facing the middle of the scene, or a ceiling or a floor; plain, in stripes or a grid. Drawn over the panorama it is given.",
    keywords: "hdri environment panorama panel plane ceiling floor window grid light studio",
    inputs: BACKGROUND,
    outputs: COLOR_OUT,
    params: &[
        // (A mode, as Color Mode is.)
        choice("placement", "Placement", 0, PLANE_LIGHT_PLACEMENTS, PLANE_LIGHT_PLACEMENT_IDS)
            .fixed()
            .tip("Facing the middle of the scene, wherever it is; or flat overhead (a ceiling) or underfoot (a floor)."),
        vec2("position", "Position", [0.5, 0.4], 0.0, 1.0).tip(
            "Where its middle is seen: across, once round; down, from straight up (0) to straight down (1). A ceiling is above the horizon, a floor below it.",
        ),
        vec2("size", "Size", [0.5, 0.5], 0.0, 4.0)
            .tip("How wide and long it is, from its middle, as far as it is from the middle of the scene (or as high above it)."),
        angle("rotation", "Rotation", 0.0, -180.0, 180.0).tip("Degrees, counter-clockwise."),
        float("hardness", "Hardness", 0.25, 0.0, 1.0).bounded().tip("How sharp its edges are."),
        choice("pattern", "Pattern", 0, PLANE_LIGHT_PATTERNS, PLANE_LIGHT_PATTERN_IDS),
        int("cells", "Cells", 4, 1, 32).when("pattern", &[1, 2]).tip("How many stripes or cells a side."),
        float("gap", "Gap", 0.1, 0.0, 0.5).bounded().when("pattern", &[1, 2]).tip("How much of each cell is dark, between them."),
        choice("color_mode", "Color Mode", 0, LIGHT_COLOR_MODES, LIGHT_COLOR_MODE_IDS).fixed(),
        color("color", "Color", [1.0, 1.0, 1.0, 1.0]).when("color_mode", &[0]),
        float("temperature", "Temperature (K)", 6500.0, 1700.0, 20000.0)
            .clamped(1000.0, 40000.0)
            .when("color_mode", &[1])
            .tip("The color of a light this hot, in kelvins: 2700 a bulb, 6500 daylight."),
        float("exposure", "Exposure (EV)", 1.0, 0.0, 10.0).tip("How bright it is: 0 as bright as white, each step twice as bright."),
    ],
    role: Role::Image,
};

/// The color of a black body `kelvin` hot, as linear RGB as bright as white (luminance 1):
/// its chromaticity on the Planckian locus as Kim et al. fit it (1667 K to 25000 K, held
/// there), in sRGB's primaries.
pub fn temperature_color(kelvin: f32) -> [f32; 3] {
    let t = f64::from(kelvin).clamp(1667.0, 25000.0);
    let x = if t <= 4000.0 {
        -0.266_123_9e9 / (t * t * t) - 0.234_358_9e6 / (t * t) + 0.877_695_6e3 / t + 0.179_910
    } else {
        -3.025_846_9e9 / (t * t * t) + 2.107_037_9e6 / (t * t) + 0.222_634_7e3 / t + 0.240_390
    };
    let y = if t <= 2222.0 {
        -1.106_381_4 * x * x * x - 1.348_110_20 * x * x + 2.185_558_32 * x - 0.202_196_83
    } else if t <= 4000.0 {
        -0.954_947_6 * x * x * x - 1.374_185_93 * x * x + 2.091_370_15 * x - 0.167_488_67
    } else {
        3.081_758_0 * x * x * x - 5.873_386_70 * x * x + 3.751_129_97 * x - 0.370_014_83
    };
    // XYZ of luminance 1, then sRGB's linear primaries.
    let (cx, cy, cz) = (x / y, 1.0, (1.0 - x - y) / y);
    let r = 3.2406 * cx - 1.5372 * cy - 0.4986 * cz;
    let g = -0.9689 * cx + 1.8758 * cy + 0.0415 * cz;
    let b = 0.0557 * cx - 0.2040 * cy + 1.0570 * cz;
    [r.max(0.0) as f32, g.max(0.0) as f32, b.max(0.0) as f32]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What decides what the other parameters of a light mean — how its color is given, where its
    /// position is, what its points are — is set, not given by a socket; the rest of what a light
    /// is (its shape and pattern too, as other nodes' are) can be.
    #[test]
    fn the_modes_of_the_lights_are_set_not_driven() {
        let modes = ["color_mode", "placement", "position_mode"];
        for def in [&HDRI_GRADIENT, &HDRI_SHAPE_LIGHT, &HDRI_LINE_LIGHT, &HDRI_PLANE_LIGHT] {
            for p in def.params {
                assert_eq!(p.can_expose(), !modes.contains(&p.key), "{} {}", def.name, p.key);
            }
        }
    }

    /// A bulb is orange, daylight white, a blue sky blue; each as bright as white.
    #[test]
    fn temperatures_have_their_colors() {
        let lum = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let bulb = temperature_color(2700.0);
        let day = temperature_color(6500.0);
        let sky = temperature_color(12000.0);
        assert!(bulb[0] > bulb[1] && bulb[1] > bulb[2], "{bulb:?}");
        assert!(day.iter().all(|c| (c - 1.0).abs() < 0.06), "{day:?}");
        assert!(sky[2] > sky[1] && sky[1] > sky[0], "{sky:?}");
        for c in [bulb, day, sky, temperature_color(500.0), temperature_color(90000.0)] {
            assert!((lum(c) - 1.0).abs() < 0.02 && c.iter().all(|v| v.is_finite() && *v >= 0.0), "{c:?}");
        }
    }
}
