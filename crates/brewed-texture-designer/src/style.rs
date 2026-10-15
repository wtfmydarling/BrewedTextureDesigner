//! Colors, fonts and widget styles of the dark theme.
//!
//! Neutral grays only (R = G = B) in a few steps: window, panel, raised, hover. Hairline
//! borders, small radii. What is active or selected is lighter, not colored; color is left
//! to what carries meaning (data types, node categories, errors) and to the images.

use iced::advanced::graphics::text::cosmic_text::{self, Fallback as _, fontdb};
use iced::advanced::graphics::text::to_attributes;
use iced::border::{self, Border};
use iced::theme::Palette;
use iced::widget::button as button_w;
use iced::widget::container;
use iced::widget::pick_list as pick_list_w;
use iced::widget::scrollable as scrollable_w;
use iced::widget::text_editor as text_editor_w;
use iced::widget::text_input as text_input_w;
use iced::widget::toggler as toggler_w;
use iced::{Background, Color, Font, Shadow, Theme, Vector};

use brewed_texture_core::nodes::ImageKind;
use brewed_texture_core::{Category, DataType};

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

/// A neutral gray: every surface and text color of the interface is one (R = G = B).
pub const fn gray(v: u8) -> Color {
    rgb(v, v, v)
}

// Surfaces, darkest to lightest.
/// Behind the panels: the gaps between them, the menu and status bars.
pub const WINDOW: Color = gray(18);
/// The graph and the 2D view.
pub const CANVAS: Color = gray(23);
pub const GRID_MINOR: Color = gray(29);
pub const GRID_MAJOR: Color = gray(37);
pub const FIELD: Color = gray(21);
pub const PANEL_DARK: Color = gray(26);
pub const PANEL: Color = gray(31);
/// Pane headers and cards.
pub const HEADER: Color = gray(37);
pub const RAISED: Color = gray(47);
pub const HOVER: Color = gray(58);
pub const BORDER: Color = gray(12);
pub const BORDER_LIGHT: Color = gray(68);
/// Hairlines between rows and sections.
pub const LINE: Color = gray(43);

pub const TEXT: Color = gray(222);
/// Headings, and what is chosen: white.
pub const TEXT_BRIGHT: Color = gray(255);
pub const TEXT_DIM: Color = gray(150);
pub const TEXT_FAINT: Color = gray(102);

/// What is selected, focused or active. Neutral like the rest: white on the dark grays.
pub const ACCENT: Color = gray(236);
pub const ACCENT_DIM: Color = gray(120);
/// Active text (tabs, toggles).
pub const ACCENT_TEXT: Color = gray(255);
/// The border of a focused field.
pub const FOCUS: Color = gray(150);
pub const ERROR: Color = rgb(232, 88, 76);
pub const WARNING: Color = rgb(228, 186, 72);
pub const SUCCESS: Color = rgb(120, 196, 134);

/// Filled part of number fields (how far the value is in its range).
pub const VALUE_FILL: Color = gray(62);
pub const VALUE_FILL_HOT: Color = gray(76);
/// The bar of a field whose value is no longer the one it came with.
pub const VALUE_FILL_CHANGED: Color = gray(104);
pub const VALUE_FILL_CHANGED_HOT: Color = gray(120);

pub const NODE_BODY: Color = gray(46);
pub const NODE_HEADER: Color = gray(58);
pub const NODE_BYPASSED: Color = gray(36);
pub const NODE_OUTLINE: Color = gray(14);
pub const THUMB_BACKGROUND: Color = gray(19);
pub const SOCKET_EMPTY: Color = gray(30);
/// Text on nodes (socket names, values).
pub const NODE_TEXT: Color = gray(212);
/// Expressions shown on nodes.
pub const EXPRESSION: Color = gray(190);

pub fn type_color(ty: DataType) -> Color {
    match ty {
        DataType::Image => rgb(200, 202, 206),
        DataType::Float => rgb(96, 176, 246),
        DataType::Int => rgb(88, 200, 140),
        DataType::Bool => rgb(236, 110, 110),
        DataType::Vec2 => rgb(236, 196, 96),
        DataType::Color => rgb(194, 146, 228),
    }
}

/// Image sockets and links, by what the image is: grayscale images gray, color images
/// orange, as in Substance Designer. (Either kind, before it is known, is gray.)
pub fn image_color(kind: Option<ImageKind>) -> Color {
    match kind {
        Some(ImageKind::Color) => rgb(236, 142, 64),
        _ => rgb(200, 202, 206),
    }
}

/// The one color in the interface: the strip along the top of a node, and the chip next
/// to its name in the lists. Bright enough to tell the categories apart at a glance on
/// the gray nodes.
pub fn category_color(c: Category) -> Color {
    match c {
        Category::Values => rgb(198, 152, 62),
        Category::Generators => rgb(74, 164, 112),
        Category::Filters => rgb(72, 132, 204),
        Category::Effects => rgb(56, 160, 170),
        Category::Adjustments => rgb(148, 110, 198),
        // Light: pale gold.
        Category::Hdri => rgb(222, 206, 128),
        Category::Output => rgb(200, 86, 86),
        // A graph used as a node: of none of the kinds, and any of them.
        Category::Graphs => gray(168),
    }
}

pub fn alpha(c: Color, a: f32) -> Color {
    Color { a: c.a * a, ..c }
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }
}

/// Display color of an RGBA value (values are shown as stored, like the 2D view).
pub fn color_of(c: [f32; 4]) -> Color {
    let q = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    Color { r: q(c[0]), g: q(c[1]), b: q(c[2]), a: q(c[3]) }
}

/// The fonts of the interface, the same on every system: Inter, one file of every weight and
/// one of its italics (SIL Open Font License 1.1, see assets/fonts/Inter-LICENSE.txt). A
/// system's own fonts differ from system to system, and one had no bold of every size.
const UI_FONTS: [&[u8]; 2] = [include_bytes!("../assets/fonts/InterVariable.ttf"), include_bytes!("../assets/fonts/InterVariable-Italic.ttf")];

/// The family the files of [`UI_FONTS`] name.
const UI_FAMILY: &str = "Inter Variable";

/// Gives the interface's fonts to the font system its text is laid out and drawn with, before any
/// text is: as the program starts, and in the tests' harness. (Once: given, they stay.)
pub fn load_fonts() {
    static LOADED: std::sync::Once = std::sync::Once::new();
    LOADED.call_once(|| {
        let mut fonts = iced::advanced::graphics::text::font_system().write().expect("the font system");
        // (As iced loads a font: text laid out before is laid out again.)
        for bytes in UI_FONTS {
            fonts.load_font(std::borrow::Cow::Borrowed(bytes));
        }
        // (Made again of the same faces: what a font system falls back to is given as it is made.)
        let fonts = fonts.raw();
        *fonts = interface_fonts(fonts.locale().to_owned(), fonts.db().clone());
    });
}

/// The font system of the interface's text, of the faces in `db` (the system's, and the files of
/// [`UI_FONTS`]): its Inter at every weight ([`own_family`]), and what Inter has not in the
/// system's fonts, drawn as [`fallbacks`] has it, in those of them that can be drawn
/// ([`undrawn_emoji`]).
fn interface_fonts(locale: String, mut db: fontdb::Database) -> cosmic_text::FontSystem {
    undrawn_emoji(&mut db);
    own_family(&mut db);
    let fallbacks = fallbacks(&mut db);
    cosmic_text::FontSystem::new_with_locale_and_db_and_fallback(locale, db, fallbacks)
}

/// Takes out of `db` the fonts of emoji the interface's text cannot be drawn in.
///
/// Its glyphs are drawn by swash, which paints one in color as layers of flat colors (COLR
/// version 0) or as a bitmap (CBDT, sbix), and fills its outline otherwise. A font whose emoji are
/// painted as COLR version 1 says alone (gradients), and have no outlines, is drawn as nothing:
/// Fedora's Noto Color Emoji is one (Noto-COLRv1.ttf), and an emoji in a name was an empty place
/// the width of one there. cosmic-text falls back to a font for the characters it has, whatever
/// can be drawn of them. Without such a font, an emoji is drawn in the next one that has it
/// (in the outlines of Noto Emoji, in the text's color, where that is installed), or as a
/// character no font has is.
fn undrawn_emoji(db: &mut fontdb::Database) {
    // (The faces cosmic-text takes for emoji are told by their names; so are these.)
    let of_emoji = |face: &&fontdb::FaceInfo| face.post_script_name.contains("Emoji") || face.families.iter().any(|(name, _)| name.contains("Emoji"));
    let faces: Vec<fontdb::ID> = db.faces().filter(of_emoji).map(|face| face.id).collect();
    for id in faces {
        if db.with_face_data(id, drawn_as_nothing).unwrap_or(false) {
            db.remove_face(id);
        }
    }
}

/// Whether swash draws nothing of the emoji of the face `index` of the font file `data`: it has
/// no layers of version 0 nor bitmaps of them, and no outline of the first of a few it has.
fn drawn_as_nothing(data: &[u8], index: u32) -> bool {
    use cosmic_text::skrifa::instance::{LocationRef, Size};
    use cosmic_text::skrifa::outline::{DrawSettings, OutlinePen};
    use cosmic_text::skrifa::{FontRef, MetadataProvider as _, Tag};
    /// Whether anything was drawn of an outline.
    struct Drawn(bool);
    impl OutlinePen for Drawn {
        fn move_to(&mut self, _: f32, _: f32) {}
        fn line_to(&mut self, _: f32, _: f32) {
            self.0 = true;
        }
        fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
            self.0 = true;
        }
        fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
            self.0 = true;
        }
        fn close(&mut self) {}
    }
    let Ok(font) = FontRef::from_index(data, index) else { return false };
    let table = |tag: &[u8; 4]| font.table_data(Tag::new(tag));
    // (COLR: its version, then how many glyphs have layers of version 0.)
    let layers = table(b"COLR").is_some_and(|colr| colr.as_bytes().get(2..4).is_some_and(|glyphs| glyphs != [0, 0]));
    if layers || table(b"CBDT").is_some() || table(b"sbix").is_some() {
        return false;
    }
    let Some(glyph) = ['😀', '🙂', '❤', '⭐', '☺'].into_iter().find_map(|emoji| font.charmap().map(emoji)) else { return false };
    let Some(outline) = font.outline_glyphs().get(glyph) else { return true };
    let mut drawn = Drawn(false);
    let whole = outline.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::default()), &mut drawn);
    whole.is_ok() && !drawn.0
}

/// Whether a font the interface's text may be drawn in, its own or the system's, has `c` (for
/// the tests: what no font has is drawn nowhere).
#[cfg(test)]
pub fn some_font_has(c: char) -> bool {
    use cosmic_text::skrifa::{FontRef, MetadataProvider as _};
    load_fonts();
    let mut fonts = iced::advanced::graphics::text::font_system().write().expect("the font system");
    let db = fonts.raw().db();
    let has = |data: &[u8], index| FontRef::from_index(data, index).is_ok_and(|font| font.charmap().map(c).is_some());
    db.faces().any(|face| db.with_face_data(face.id, has).unwrap_or(false))
}

/// Makes the files of [`UI_FONTS`] the only faces of their family in `db`, each one at every
/// weight it has.
///
/// The font database lists a variable font once, at the weight of its default instance (400), and
/// cosmic-text draws a weight in a face of exactly that weight first: of the family asked for, else
/// of a family the system's text falls back to (Noto Sans, DejaVu Sans, FreeSans on Linux, Segoe UI
/// on Windows, .SF NS and Menlo on macOS), and only then in the family asked for at another
/// weight. So the interface's semibold was FreeSans Bold where FreeFont is installed, and its bold
/// DejaVu Sans Bold; and a system with an Inter Variable of its own (of its own version) drew the
/// regular in that one. Listed at each weight, the files are drawn at each; what Inter has not
/// (CJK, emoji) is still drawn in the system's fonts (see [`fallbacks`]).
fn own_family(db: &mut fontdb::Database) {
    use fontdb::{FaceInfo, ID, Weight};
    let named = |face: &&FaceInfo| face.families.iter().any(|(name, _)| name == UI_FAMILY);
    let family: Vec<FaceInfo> = db.faces().filter(named).cloned().collect();
    for face in family {
        if !embedded(&face) {
            db.remove_face(face.id);
            continue;
        }
        // (Inter's axis of weight runs from 100 to 900: every weight a font is asked in, Thin to Black.)
        for weight in (100..=900).step_by(100).filter(|&weight| weight != face.weight.0) {
            db.push_face_info(FaceInfo { id: ID::dummy(), weight: Weight(weight), ..face.clone() });
        }
    }
}

/// Whether `face` is of one of the files of [`UI_FONTS`] (iced keeps a copy of each).
fn embedded(face: &fontdb::FaceInfo) -> bool {
    matches!(&face.source, fontdb::Source::Binary(data) if UI_FONTS.contains(&(**data).as_ref()))
}

/// Lists every family of the system in `db` the interface's text may fall back to at every weight
/// from 100 to 900, upright and in italics, and makes each one cosmic-text falls back to, in the
/// order the regular weight comes to them: what the interface's Inter has not (CJK, emoji,
/// Hebrew…) is drawn at every weight and in italics in the family the regular weight draws it in,
/// in that family's face nearest the weight.
///
/// cosmic-text draws a character the family asked for has not in a family the system's text falls
/// back to (by script, CJK in Noto Sans CJK on Linux; then the common ones, emoji in Noto Color
/// Emoji) in a face of exactly the weight asked and of its style; and only then in any face of the
/// style (or named "Emoji"), the nearest in weight first, of two as near the lighter, then the
/// first listed. No font of CJK has italics: CJK in italics was drawn as empty boxes, Inter
/// Italic's .notdef. And at a weight but 400 the face was all but chance: in an Ubuntu with
/// WenQuanYi Zen Hei (which says it is of weight 500) and IPAGothic, CJK was IPAGothic up to 400
/// and WenQuanYi from 500 on; Hebrew was Liberation Serif, DejaVu Sans, FreeSans Bold or Liberation
/// Mono Bold, by weight.
///
/// Where a family has no face of a weight and style, it is listed there as its face of that style
/// nearest in weight (in italics, its italics, else its obliques, else its upright faces: drawn
/// upright), the bytes shared. Then every family the regular weight comes to is one cosmic-text
/// falls back to, after the platform's own, in that order: the same at every weight.
fn fallbacks(db: &mut fontdb::Database) -> Fallbacks {
    use fontdb::{FaceInfo, ID, Style, Weight};
    fn family(face: &FaceInfo) -> &str {
        face.families.first().map_or("", |(name, _)| name)
    }
    let platform = cosmic_text::PlatformFallback;
    let regular = to_attributes(ui_font());
    let named = |face: &FaceInfo, names: &[&str]| face.families.iter().any(|(name, _)| names.contains(&name.as_str()));
    // The system's faces of the interface's width (its text has none of another): not of its own
    // family, listed at every weight by `own_family`, nor of one never to be fallen back to.
    let theirs: Vec<FaceInfo> = db
        .faces()
        .filter(|face| face.stretch == regular.stretch)
        .filter(|face| !named(face, &[UI_FAMILY]) && !named(face, platform.forbidden_fallback()))
        .cloned()
        .collect();
    // Their families as the regular weight comes to them: of the faces cosmic-text takes for it
    // (upright, or named "Emoji", which it takes in any style), the nearest in weight first, of two
    // as near the lighter, then as the database lists them.
    let mut regulars: Vec<&FaceInfo> = theirs.iter().filter(|face| regular.matches(face)).collect();
    regulars.sort_by_key(|face| (face.weight.0.abs_diff(regular.weight.0), face.weight.0, face.id));
    let mut families: Vec<&str> = Vec::new();
    for face in regulars {
        if !families.contains(&family(face)) {
            families.push(family(face));
        }
    }
    // (Not monospaced, whatever the face is: monospaced text looks through every monospaced face
    // for each line it lays out, and keeps to the system's own.)
    let mut list = |face: &FaceInfo, style, weight| {
        let listed = FaceInfo { id: ID::dummy(), style, weight: Weight(weight), monospaced: false, ..face.clone() };
        db.push_face_info(listed);
    };
    for &name in &families {
        let faces: Vec<&FaceInfo> = theirs.iter().filter(|face| family(face) == name).collect();
        let of = |style: Style| faces.iter().copied().filter(move |face| face.style == style);
        for style in [Style::Normal, Style::Italic] {
            // (Upright text keeps to upright faces, as it did.)
            let liked: &[Style] = if style == Style::Italic { &[Style::Italic, Style::Oblique, Style::Normal] } else { &[Style::Normal] };
            let near = liked.iter().map(|&wanted| of(wanted).collect::<Vec<_>>()).find(|near| !near.is_empty());
            let Some(near) = near else { continue };
            for weight in (100..=900).step_by(100).filter(|&weight| !of(style).any(|face| face.weight.0 == weight)) {
                // (The nearest as cosmic-text has it: of two as near, the lighter.)
                let nearest = near.iter().map(|face| face.weight.0).min_by_key(|&w| (w.abs_diff(weight), w));
                for face in near.iter().filter(|face| Some(face.weight.0) == nearest) {
                    list(face, style, weight);
                }
            }
        }
    }
    // Fonts of emoji before the others: a font of text may have an emoji too, of its own drawing
    // (in Fedora, where the platform's font of emoji cannot be drawn, they were Adwaita Mono's,
    // half the size of the letters, with Noto Emoji installed).
    let (emoji, others): (Vec<&str>, Vec<&str>) = families.into_iter().partition(|name| name.contains("Emoji"));
    // (Names leaked: the font system that keeps them is made once and lives as long as the program.)
    let mut common = platform.common_fallback().to_vec();
    for name in emoji.into_iter().chain(others) {
        if !common.contains(&name) {
            common.push(Box::leak(name.to_owned().into_boxed_str()));
        }
    }
    Fallbacks { platform, common }
}

/// What cosmic-text falls back to for the interface's text (see [`fallbacks`]): by script as the
/// platform does (CJK in Noto Sans CJK on Linux…), then the platform's common fallbacks (Noto Color
/// Emoji among them), then every other family of the system.
struct Fallbacks {
    platform: cosmic_text::PlatformFallback,
    common: Vec<&'static str>,
}

impl cosmic_text::Fallback for Fallbacks {
    fn common_fallback(&self) -> &[&'static str] {
        &self.common
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        self.platform.forbidden_fallback()
    }

    fn script_fallback(&self, script: unicode_script::Script, locale: &str) -> &[&'static str] {
        self.platform.script_fallback(script, locale)
    }
}

pub fn ui_font() -> Font {
    Font::with_name(UI_FAMILY)
}

pub fn bold() -> Font {
    Font { weight: iced::font::Weight::Semibold, ..ui_font() }
}

/// Titles and headings, and a value that is no longer the one it came with: heavy enough to
/// see at a glance.
pub fn heavy() -> Font {
    Font { weight: iced::font::Weight::Bold, ..ui_font() }
}

/// The name of a field whose value was changed.
pub fn italic() -> Font {
    Font { style: iced::font::Style::Italic, ..ui_font() }
}

pub fn mono() -> Font {
    if cfg!(windows) { Font::with_name("Consolas") } else { Font::MONOSPACE }
}

pub const TEXT_SIZE: f32 = 13.0;
pub const SMALL: f32 = 12.0;
/// Pane titles and section headers (upper case).
pub const CAPTION: f32 = 11.0;

pub fn theme() -> Theme {
    Theme::custom(
        brewed_texture_core::APP_NAME.to_string(),
        Palette { background: PANEL, text: TEXT, primary: ACCENT, success: SUCCESS, warning: WARNING, danger: ERROR },
    )
}

// ---------------------------------------------------------------------------- containers

pub fn panel(_: &Theme) -> container::Style {
    container::Style { background: Some(PANEL.into()), text_color: Some(TEXT), ..Default::default() }
}

pub fn panel_dark(_: &Theme) -> container::Style {
    container::Style { background: Some(PANEL_DARK.into()), text_color: Some(TEXT), ..Default::default() }
}

/// The strip at the top of a pane: its title and tools.
pub fn header_bar(_: &Theme) -> container::Style {
    container::Style {
        background: Some(HEADER.into()),
        border: Border { color: BORDER, width: 0.0, radius: 0.0.into() },
        text_color: Some(TEXT),
        ..Default::default()
    }
}

/// The menu bar and the status bar.
pub fn window_bar(_: &Theme) -> container::Style {
    container::Style { background: Some(WINDOW.into()), text_color: Some(TEXT_DIM), ..Default::default() }
}

/// A 1 px line.
pub fn hairline(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: Some(color.into()), ..Default::default() }
}

pub fn canvas_frame(_: &Theme) -> container::Style {
    container::Style { background: Some(CANVAS.into()), ..Default::default() }
}

pub fn field_box(_: &Theme) -> container::Style {
    container::Style { background: Some(FIELD.into()), border: Border { color: LINE, width: 1.0, radius: 3.0.into() }, ..Default::default() }
}

pub fn popup(_: &Theme) -> container::Style {
    container::Style {
        background: Some(gray(36).into()),
        border: Border { color: BORDER_LIGHT, width: 1.0, radius: 4.0.into() },
        shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.5), offset: Vector::new(0.0, 6.0), blur_radius: 16.0 },
        text_color: Some(TEXT),
        ..Default::default()
    }
}

/// A window of the program's besides the main one (the Color Editor's): of the color of a popup,
/// with a line around it where the program draws its frame (elsewhere the system does).
pub fn tool_window(_: &Theme) -> container::Style {
    let line = if crate::app::OWN_TITLE_BAR { 1.0 } else { 0.0 };
    container::Style {
        background: Some(gray(36).into()),
        border: Border { color: BORDER_LIGHT, width: line, radius: 0.0.into() },
        text_color: Some(TEXT),
        ..Default::default()
    }
}

pub fn scrim(_: &Theme) -> container::Style {
    container::Style { background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()), ..Default::default() }
}

pub fn message_box(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(alpha(color, 0.10).into()),
        border: Border { color: alpha(color, 0.55), width: 1.0, radius: 3.0.into() },
        text_color: Some(mix(color, TEXT, 0.4)),
        ..Default::default()
    }
}

/// Rows of parameter lists: every other one slightly lighter.
pub fn stripe(even: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: (!even).then(|| Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.018))),
        border: border::rounded(2),
        ..Default::default()
    }
}

/// A list of settings: rows one under the other, a line between two, in a frame (the lines
/// are this, between rows a pixel apart).
pub fn setting_list(_: &Theme) -> container::Style {
    container::Style { background: Some(LINE.into()), border: Border { color: LINE, width: 1.0, radius: 2.0.into() }, ..Default::default() }
}

/// A row of a list of settings: every other one a little lighter, so that what is said on
/// the left is read with the value on the right.
pub fn setting_row(even: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: Some(if even { gray(33) } else { gray(36) }.into()), ..Default::default() }
}

/// A row of a list that is a link (the works of the third-party notices): as a row of a list of
/// settings, every other one a little lighter, lit under the mouse.
pub fn link_row(even: bool) -> impl Fn(&Theme, button_w::Status) -> button_w::Style {
    move |_, status| {
        let background = match status {
            button_w::Status::Hovered => HOVER,
            button_w::Status::Pressed => RAISED,
            _ if even => gray(33),
            _ => gray(36),
        };
        button_w::Style { background: Some(background.into()), text_color: TEXT, border: Border::default(), shadow: Shadow::default(), snap: true }
    }
}

/// The red of a button that closes the window, as the system's is.
pub const CLOSE_RED: Color = rgb(196, 43, 28);

/// A button of the window's own title bar: lit while pointed at; the one that closes the window
/// red then.
pub fn window_button(close: bool) -> impl Fn(&Theme, button_w::Status) -> button_w::Style {
    move |_, status| {
        let background = match status {
            button_w::Status::Hovered if close => Some(CLOSE_RED),
            button_w::Status::Pressed if close => Some(alpha(CLOSE_RED, 0.8)),
            button_w::Status::Hovered => Some(HEADER),
            button_w::Status::Pressed => Some(RAISED),
            _ => None,
        };
        let text_color = if close && matches!(status, button_w::Status::Hovered | button_w::Status::Pressed) { TEXT_BRIGHT } else { TEXT };
        button_w::Style { background: background.map(Into::into), text_color, border: Border::default(), shadow: Shadow::default(), snap: true }
    }
}

/// The side of a dialog that lists its pages.
pub fn sidebar(_: &Theme) -> container::Style {
    container::Style { background: Some(PANEL_DARK.into()), text_color: Some(TEXT), ..Default::default() }
}

/// A word that marks something (a setting taken only when the program starts): framed in its
/// color.
pub fn tag(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(alpha(color, 0.12).into()),
        border: Border { color: alpha(color, 0.5), width: 1.0, radius: 3.0.into() },
        text_color: Some(color),
        ..Default::default()
    }
}

/// A field something dragged is dropped on: one that takes it (`takes`), or one that does not.
pub fn drop_zone(takes: bool) -> impl Fn(&Theme) -> container::Style {
    let color = if takes { ACCENT } else { ERROR };
    move |_| container::Style {
        background: Some(Color { a: 0.08, ..color }.into()),
        border: Border { color, width: 1.5, radius: 3.0.into() },
        ..Default::default()
    }
}

pub fn chip(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: Some(color.into()), border: border::rounded(1), ..Default::default() }
}

pub fn tooltip(_: &Theme) -> container::Style {
    container::Style {
        background: Some(gray(22).into()),
        border: Border { color: BORDER_LIGHT, width: 1.0, radius: 3.0.into() },
        text_color: Some(TEXT),
        shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.45), offset: Vector::new(0.0, 3.0), blur_radius: 8.0 },
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------- buttons

fn button_base(bg: Option<Color>, text: Color, border: Color) -> button_w::Style {
    button_w::Style {
        background: bg.map(Background::Color),
        text_color: text,
        border: Border { color: border, width: 1.0, radius: 3.0.into() },
        shadow: Shadow::default(),
        snap: true,
    }
}

/// Regular push button.
pub fn button(_: &Theme, status: button_w::Status) -> button_w::Style {
    match status {
        button_w::Status::Active => button_base(Some(RAISED), TEXT, gray(56)),
        button_w::Status::Hovered => button_base(Some(HOVER), TEXT, BORDER_LIGHT),
        button_w::Status::Pressed => button_base(Some(gray(52)), ACCENT_TEXT, ACCENT_DIM),
        button_w::Status::Disabled => button_base(Some(PANEL), TEXT_FAINT, LINE),
    }
}

/// The default action of a dialog.
pub fn primary_button(_: &Theme, status: button_w::Status) -> button_w::Style {
    match status {
        button_w::Status::Active => button_base(Some(ACCENT), gray(18), ACCENT),
        button_w::Status::Hovered => button_base(Some(gray(250)), gray(18), gray(250)),
        button_w::Status::Pressed => button_base(Some(gray(205)), gray(18), gray(205)),
        button_w::Status::Disabled => button_base(Some(PANEL), TEXT_FAINT, LINE),
    }
}

/// Flat button without chrome until hovered (toolbars, icon buttons).
pub fn flat_button(_: &Theme, status: button_w::Status) -> button_w::Style {
    let none = Color::TRANSPARENT;
    match status {
        button_w::Status::Active => button_base(None, TEXT, none),
        button_w::Status::Hovered => button_base(Some(HOVER), TEXT, none),
        button_w::Status::Pressed => button_base(Some(RAISED), ACCENT_TEXT, none),
        button_w::Status::Disabled => button_base(None, TEXT_FAINT, none),
    }
}

/// A button that stays down while `on` (tabs, channels, modes): a tinted background and
/// accent text rather than a solid block.
pub fn toggle_button(on: bool) -> impl Fn(&Theme, button_w::Status) -> button_w::Style {
    move |theme, status| {
        if on {
            let bg = if status == button_w::Status::Hovered { alpha(ACCENT, 0.24) } else { alpha(ACCENT, 0.16) };
            button_base(Some(bg), ACCENT_TEXT, alpha(ACCENT, 0.45))
        } else {
            flat_button(theme, status)
        }
    }
}

/// A switch that is a button whichever way it is (Float, HSV): off, a push button as any; on,
/// light, its text dark, as a button held down. (Off, it had no chrome at all, as a tab or a
/// channel has: it read as a word beside the sliders, not as something to press.)
pub fn switch_button(on: bool) -> impl Fn(&Theme, button_w::Status) -> button_w::Style {
    move |theme, status| match (on, status) {
        (false, _) => button(theme, status),
        (true, button_w::Status::Hovered) => button_base(Some(gray(232)), gray(18), gray(250)),
        (true, button_w::Status::Disabled) => button_base(Some(gray(96)), gray(40), gray(110)),
        (true, _) => button_base(Some(gray(205)), gray(18), gray(236)),
    }
}

/// The field of a segmented control: its options side by side in it.
pub fn segments(_: &Theme) -> container::Style {
    container::Style { background: Some(FIELD.into()), border: Border { color: LINE, width: 1.0, radius: 4.0.into() }, ..Default::default() }
}

/// An option of a segmented control: raised and bright when it is the one chosen, lit when
/// hovered.
pub fn segment(chosen: bool) -> impl Fn(&Theme, button_w::Status) -> button_w::Style {
    move |_, status| {
        let (bg, text, border) = match (chosen, status) {
            (true, button_w::Status::Disabled) => (Some(gray(44)), TEXT_DIM, gray(52)),
            (true, _) => (Some(gray(78)), ACCENT_TEXT, gray(98)),
            (false, button_w::Status::Hovered) => (Some(gray(38)), TEXT, Color::TRANSPARENT),
            (false, button_w::Status::Pressed) => (Some(gray(46)), TEXT, Color::TRANSPARENT),
            (false, button_w::Status::Disabled) => (None, TEXT_FAINT, Color::TRANSPARENT),
            (false, button_w::Status::Active) => (None, TEXT_DIM, Color::TRANSPARENT),
        };
        let shadow =
            if chosen { Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.35), offset: Vector::new(0.0, 1.0), blur_radius: 2.0 } } else { Shadow::default() };
        button_w::Style {
            background: bg.map(Background::Color),
            text_color: text,
            border: Border { color: border, width: 1.0, radius: 3.0.into() },
            shadow,
            snap: true,
        }
    }
}

/// A row of a list or menu: full width, highlighted when hovered or selected.
/// A page of a side bar (of the settings), as the Home screen's: darker than the bar when it is
/// the one shown, its text white (and heavier, see its label); lighter when pointed at.
pub fn nav_tab(chosen: bool) -> impl Fn(&Theme, button_w::Status) -> button_w::Style {
    move |_, status| {
        let (bg, text) = match (chosen, status) {
            (true, _) => (Some(gray(17)), TEXT_BRIGHT),
            (false, button_w::Status::Hovered) => (Some(gray(46)), TEXT),
            (false, button_w::Status::Pressed) => (Some(gray(52)), TEXT_BRIGHT),
            (false, _) => (None, TEXT_DIM),
        };
        button_w::Style { background: bg.map(Background::Color), text_color: text, border: border::rounded(4), shadow: Shadow::default(), snap: true }
    }
}

/// The font of a label of an option: heavier when it is the one chosen.
pub fn chosen_font(chosen: bool) -> Font {
    if chosen { bold() } else { ui_font() }
}

pub fn row_button(selected: bool) -> impl Fn(&Theme, button_w::Status) -> button_w::Style {
    move |_, status| {
        let bg = match (selected, status) {
            (_, button_w::Status::Pressed) => Some(alpha(ACCENT, 0.28)),
            (true, _) => Some(alpha(ACCENT, 0.18)),
            (false, button_w::Status::Hovered) => Some(HOVER),
            _ => None,
        };
        let text = if status == button_w::Status::Disabled { TEXT_FAINT } else { TEXT };
        button_w::Style { background: bg.map(Background::Color), text_color: text, border: border::rounded(2), shadow: Shadow::default(), snap: true }
    }
}

/// An option in a menu of pictures: tinted when it is the current one, lit when the mouse
/// or the keyboard is on it.
pub fn option_button(current: bool, lit: bool) -> impl Fn(&Theme, button_w::Status) -> button_w::Style {
    move |theme, status| {
        let status = if lit && status == button_w::Status::Active { button_w::Status::Hovered } else { status };
        let mut style = row_button(current)(theme, status);
        if lit {
            style.border = Border { color: BORDER_LIGHT, width: 1.0, radius: 2.0.into() };
        }
        style
    }
}

/// The look of the export window (the `export-window` crate): the program's.
pub fn export_look() -> export_window::Look {
    export_window::Look {
        font: ui_font(),
        bold: bold(),
        mono: mono(),
        text_size: TEXT_SIZE,
        small_size: SMALL,
        caption_size: CAPTION,
        segment: |theme, status, chosen| segment(chosen)(theme, status),
        segments,
        button,
        flat_button,
        primary_button,
        text_input,
        pick_list,
        shorten: crate::view::shortened,
        // The program's surfaces: the window's bar, a pane's header, the panels, what is raised.
        colors: export_window::Colors {
            header: WINDOW,
            strip: HEADER,
            sunken: PANEL_DARK,
            page: PANEL,
            raised: RAISED,
            edge: gray(60),
            menu: gray(52),
            field: FIELD,
            hover: HOVER,
            chosen: gray(17),
            line: LINE,
            text: TEXT,
            bright: TEXT_BRIGHT,
            dim: TEXT_DIM,
            faint: TEXT_FAINT,
            warning: WARNING,
            converted: rgb(222, 150, 60),
            inherited: rgb(214, 196, 158),
        },
    }
}

/// Where the templates of a new graph are: darker than the dialog.
pub fn template_area(_: &Theme) -> container::Style {
    container::Style { background: Some(gray(22).into()), border: Border { color: LINE, width: 1.0, radius: 4.0.into() }, ..Default::default() }
}

/// The frame around a picture of an option.
pub fn picture_frame(_: &Theme) -> container::Style {
    container::Style { background: Some(THUMB_BACKGROUND.into()), border: Border { color: BORDER, width: 1.0, radius: 2.0.into() }, ..Default::default() }
}

// ---------------------------------------------------------------------------- inputs

/// A field that opens a menu, like a pick list: raised while hovered, outlined while open.
pub fn choice_field(open: bool) -> impl Fn(&Theme, button_w::Status) -> button_w::Style {
    move |_, status| {
        let (bg, border) = match (open, status) {
            (true, _) => (RAISED, FOCUS),
            (false, button_w::Status::Hovered | button_w::Status::Pressed) => (RAISED, BORDER_LIGHT),
            _ => (FIELD, LINE),
        };
        button_base(Some(bg), TEXT, border)
    }
}

pub fn text_input(_: &Theme, status: text_input_w::Status) -> text_input_w::Style {
    let border = match status {
        text_input_w::Status::Focused { .. } => FOCUS,
        text_input_w::Status::Hovered => BORDER_LIGHT,
        _ => LINE,
    };
    text_input_w::Style {
        background: FIELD.into(),
        border: Border { color: border, width: 1.0, radius: 3.0.into() },
        icon: TEXT_DIM,
        placeholder: TEXT_FAINT,
        value: if status == text_input_w::Status::Disabled { TEXT_DIM } else { TEXT },
        selection: alpha(ACCENT, 0.3),
    }
}

pub fn text_editor(_: &Theme, status: text_editor_w::Status) -> text_editor_w::Style {
    let border = match status {
        text_editor_w::Status::Focused { .. } => FOCUS,
        text_editor_w::Status::Hovered => BORDER_LIGHT,
        _ => LINE,
    };
    text_editor_w::Style {
        background: FIELD.into(),
        border: Border { color: border, width: 1.0, radius: 3.0.into() },
        placeholder: TEXT_FAINT,
        value: TEXT,
        selection: alpha(ACCENT, 0.3),
    }
}

pub fn pick_list(_: &Theme, status: pick_list_w::Status) -> pick_list_w::Style {
    let (bg, border) = match status {
        pick_list_w::Status::Hovered => (RAISED, BORDER_LIGHT),
        pick_list_w::Status::Opened { .. } => (RAISED, FOCUS),
        _ => (FIELD, LINE),
    };
    pick_list_w::Style {
        text_color: TEXT,
        placeholder_color: TEXT_FAINT,
        handle_color: TEXT_DIM,
        background: bg.into(),
        border: Border { color: border, width: 1.0, radius: 3.0.into() },
    }
}

pub fn toggler(_: &Theme, status: toggler_w::Status) -> toggler_w::Style {
    let (on, hovered, disabled) = match status {
        toggler_w::Status::Active { is_toggled } => (is_toggled, false, false),
        toggler_w::Status::Hovered { is_toggled } => (is_toggled, true, false),
        toggler_w::Status::Disabled { is_toggled } => (is_toggled, false, true),
    };
    let track = match (on, disabled) {
        (_, true) => PANEL,
        (true, _) => {
            if hovered {
                gray(250)
            } else {
                gray(215)
            }
        }
        (false, _) => FIELD,
    };
    toggler_w::Style {
        background: track.into(),
        background_border_width: 1.0,
        background_border_color: if on {
            track
        } else if hovered {
            BORDER_LIGHT
        } else {
            LINE
        },
        foreground: if on {
            gray(30)
        } else if hovered {
            TEXT
        } else {
            TEXT_DIM
        }
        .into(),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        text_color: Some(TEXT),
        border_radius: None,
        padding_ratio: 0.16,
    }
}

/// What a box of what scrolls looks like (see `crate::widget::scroll`): the set's, on this
/// program's panels.
pub fn scrollable(theme: &Theme, status: scrollable_w::Status) -> scrollable_w::Style {
    scroll_box::look(PANEL, TEXT_DIM)(theme, status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::graphics::text::to_shaping;
    use std::sync::Arc;

    /// The interface's font in every weight a font is asked in, upright and in italics.
    fn every_weight() -> Vec<Font> {
        use iced::font::{Style, Weight};
        let weights =
            [Weight::Thin, Weight::ExtraLight, Weight::Light, Weight::Normal, Weight::Medium, Weight::Semibold, Weight::Bold, Weight::ExtraBold, Weight::Black];
        let styles = [Style::Normal, Style::Italic];
        styles.into_iter().flat_map(|style| weights.map(|weight| Font { weight, style, ..ui_font() })).collect()
    }

    /// What `text` in `font` is drawn in, laid out as iced lays out a text: for each glyph, its face,
    /// the weight it is drawn at, and the glyph (0 where the face has not the character).
    fn drawn_in(fonts: &mut cosmic_text::FontSystem, text: &str, font: Font) -> Vec<(fontdb::FaceInfo, u16, u16)> {
        let mut buffer = cosmic_text::Buffer::new(fonts, cosmic_text::Metrics::new(13.0, 17.0));
        let shaping = to_shaping(iced::advanced::text::Shaping::default(), text);
        buffer.set_text(fonts, text, &to_attributes(font), shaping, None);
        buffer.shape_until_scroll(fonts, false);
        let face = |id| fonts.db().face(id).expect("a face of the database").clone();
        let glyphs = buffer.layout_runs().flat_map(|run| run.glyphs);
        glyphs.map(|glyph| (face(glyph.font_id), glyph.font_weight.0, glyph.glyph_id)).collect()
    }

    /// A face as a failure names it: its family, its PostScript name, and where it is.
    fn named(face: &fontdb::FaceInfo) -> String {
        let at = match &face.source {
            fontdb::Source::Binary(_) if embedded(face) => "embedded".to_owned(),
            fontdb::Source::Binary(_) => "another font in memory".to_owned(),
            fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => path.display().to_string(),
        };
        format!("{} ({}, {at})", face.families[0].0, face.post_script_name)
    }

    /// Whether the face `id` of `db` has `c`; and, `colored`, in color: in bitmaps (CBDT, sbix),
    /// layers (COLR) or SVG, as fonts of emoji draw.
    fn has(db: &fontdb::Database, id: fontdb::ID, c: char, colored: bool) -> bool {
        use cosmic_text::skrifa::{FontRef, MetadataProvider as _, Tag};
        let has = |data: &[u8], index| {
            let font = FontRef::from_index(data, index).ok()?;
            let colors = [b"CBDT", b"sbix", b"COLR", b"SVG "];
            let in_color = colors.into_iter().any(|tag| font.table_data(Tag::new(tag)).is_some());
            Some(font.charmap().map(c).is_some() && (in_color || !colored))
        };
        db.with_face_data(id, has).flatten().unwrap_or(false)
    }

    /// Where `texts` are drawn in another font than the interface's Inter, or at another weight
    /// than asked, in any weight.
    fn not_in_inter(fonts: &mut cosmic_text::FontSystem, texts: &[&str]) -> Vec<String> {
        let mut wrong = Vec::new();
        for text in texts {
            for font in every_weight() {
                for (face, weight, _) in drawn_in(fonts, text, font) {
                    let said = format!("{text:?} in {:?} {:?}: {} at {weight}", font.weight, font.style, named(&face));
                    if (!embedded(&face) || weight != to_attributes(font).weight.0) && !wrong.contains(&said) {
                        wrong.push(said);
                    }
                }
            }
        }
        wrong
    }

    /// The interface's text is drawn in its own Inter at every weight, whatever fonts the system
    /// has: here it has none but such as were drawn in Inter's place, a face of each family its
    /// text falls back to at every weight, upright and in italics, and an Inter Variable of its own,
    /// listed as a variable font is, at 400 (as FreeSans Bold, of weight 600, drew the semibold
    /// where FreeFont is installed, DejaVu Sans Bold the bold, and a system's Inter Variable the
    /// regular).
    #[test]
    fn every_weight_is_drawn_in_the_embedded_inter_whatever_fonts_the_system_has() {
        // (Their letters are another Inter's, the engine's 3.19: a face drawn in is told by what it
        // is, not by how it looks.)
        let theirs: Arc<dyn AsRef<[u8]> + Sync + Send> = Arc::new(include_bytes!("../../brewed-texture-engine/assets/fonts/Inter-Regular.ttf").as_slice());
        let mut db = fontdb::Database::new();
        let mut claim = |family: &str, font: Font| {
            let asked = to_attributes(font);
            db.push_face_info(fontdb::FaceInfo {
                id: fontdb::ID::dummy(),
                source: fontdb::Source::Binary(theirs.clone()),
                index: 0,
                families: vec![(family.to_owned(), fontdb::Language::English_UnitedStates)],
                post_script_name: format!("{family} {}", asked.weight.0),
                style: asked.style,
                weight: asked.weight,
                stretch: asked.stretch,
                monospaced: false,
            });
        };
        for family in cosmic_text::PlatformFallback.common_fallback() {
            for font in every_weight() {
                claim(family, font);
            }
        }
        for font in every_weight().into_iter().filter(|font| font.weight == iced::font::Weight::Normal) {
            claim(UI_FAMILY, font);
        }
        // The interface's fonts, given as `load_fonts` gives them.
        for bytes in UI_FONTS {
            db.load_font_source(fontdb::Source::Binary(Arc::new(bytes)));
        }
        let mut fonts = interface_fonts("en-US".to_owned(), db);
        // (A text of ASCII alone is laid out in its first font; any other, in each in turn.)
        let wrong = not_in_inter(&mut fonts, &["Library", "Ωμέγα Жирный"]);
        assert!(wrong.is_empty(), "drawn in another font than the interface's Inter:\n{}", wrong.join("\n"));
    }

    /// What Inter has not is drawn at every weight, upright and in italics, in the family the
    /// regular weight draws it in, whatever weights and styles the system has of it: here it has
    /// of it ('ɕ', which the interface's Inter has not) nothing but upright faces such as an Ubuntu
    /// has of CJK and emoji: of a family its text does not fall back to by name, of 400 alone and
    /// listed first (as IPAGothic is), one of 500 alone (as WenQuanYi Zen Hei), and then, or not,
    /// its font of emoji, of 400 alone and named "Emoji" (as Noto Color Emoji), which it falls back
    /// to by name. Without the emoji font the character is drawn in the family of 400 at every
    /// weight, with it in the emoji font. (cosmic-text drew it from 500 on in the family of 500,
    /// the nearest in weight; in italics, as empty boxes, or in the emoji font, as it takes one
    /// named "Emoji" in any style.) Monospaced text keeps to the monospaced faces there were.
    #[test]
    fn what_inter_has_not_is_drawn_in_one_family_at_every_weight_upright_and_in_italics() {
        // (Their letters are another Inter's, the engine's 3.19, which has some the interface's
        // has not.)
        let theirs: Arc<dyn AsRef<[u8]> + Sync + Send> = Arc::new(include_bytes!("../../brewed-texture-engine/assets/fonts/Inter-Regular.ttf").as_slice());
        let face = |family: &str, weight: u16| fontdb::FaceInfo {
            id: fontdb::ID::dummy(),
            source: fontdb::Source::Binary(theirs.clone()),
            index: 0,
            families: vec![(family.to_owned(), fontdb::Language::English_UnitedStates)],
            post_script_name: family.replace(' ', ""),
            style: fontdb::Style::Normal,
            weight: fontdb::Weight(weight),
            stretch: fontdb::Stretch::Normal,
            // (As Noto Color Emoji and Unifont are.)
            monospaced: true,
        };
        let common = cosmic_text::PlatformFallback.common_fallback();
        let emoji = common.iter().copied().find(|family| family.contains("Emoji"));
        let mut wrong = Vec::new();
        for emoji in [None, emoji] {
            let mut db = fontdb::Database::new();
            db.push_face_info(face("Gothic", 400));
            db.push_face_info(face("Zen Hei", 500));
            if let Some(emoji) = emoji {
                db.push_face_info(face(emoji, 400));
            }
            for bytes in UI_FONTS {
                db.load_font_source(fontdb::Source::Binary(Arc::new(bytes)));
            }
            let monospaced = db.faces().filter(|face| face.monospaced).count();
            let mut fonts = interface_fonts("en-US".to_owned(), db);
            let with = if emoji.is_some() { "with the emoji font" } else { "without the emoji font" };
            for font in every_weight() {
                let asked = format!("{with}, in {:?} {:?}", font.weight, font.style);
                for (face, _, glyph) in drawn_in(&mut fonts, "ɕ", font) {
                    if face.families[0].0 != emoji.unwrap_or("Gothic") || glyph == 0 {
                        wrong.push(format!("{asked}: {}, glyph {glyph}", named(&face)));
                    }
                }
            }
            let now = fonts.db().faces().filter(|face| face.monospaced).count();
            assert_eq!(now, monospaced, "{with}: monospaced faces listed anew");
        }
        assert!(wrong.is_empty(), "not drawn in the family the regular weight draws it in:\n{}", wrong.join("\n"));
    }

    /// In the program's own font system, with the fonts of the system the tests run on: the
    /// interface's text is drawn in its Inter at every weight, and what Inter has not, CJK and
    /// emoji, in a font of the system that has it (where one has), at every weight, upright and in
    /// italics, of the family the regular weight draws it in; emoji, where the system has a font of
    /// them in color, in color at every weight.
    #[test]
    fn what_inter_has_not_is_drawn_in_the_systems_fonts() {
        load_fonts();
        // (Told once the font system is let go: a test that fails holding it would fail every other.)
        let (wrong, missing) = {
            let mut fonts = iced::advanced::graphics::text::font_system().write().expect("the font system");
            let fonts = fonts.raw();
            let wrong = not_in_inter(fonts, &["Library", "Ωμέγα Жирный"]);
            // (Of the faces the interface's text may fall back to: of its width, and not of a
            // family never fallen back to.)
            let width = to_attributes(ui_font()).stretch;
            let never = |face: &fontdb::FaceInfo| {
                let never = cosmic_text::PlatformFallback.forbidden_fallback();
                face.families.iter().any(|(name, _)| never.contains(&name.as_str()))
            };
            let mut missing = Vec::new();
            for c in ['漢', 'か', '🙂'] {
                let theirs = |face: &&fontdb::FaceInfo| face.stretch == width && !embedded(face) && !never(face);
                let of_the_system = |colored| {
                    let db = fonts.db();
                    db.faces().filter(theirs).any(|face| has(db, face.id, c, colored))
                };
                if !of_the_system(false) {
                    eprintln!("{c:?}: no font of this system has it");
                    continue;
                }
                let colored = of_the_system(true);
                let regular = drawn_in(fonts, &c.to_string(), ui_font());
                let regular = regular.first().map_or(String::new(), |(face, _, _)| face.families[0].0.clone());
                for font in every_weight() {
                    let asked = format!("{c:?} in {:?} {:?}", font.weight, font.style);
                    for (face, _, glyph) in drawn_in(fonts, &c.to_string(), font) {
                        let gray = colored && !has(fonts.db(), face.id, c, true);
                        if embedded(&face) || glyph == 0 || face.families[0].0 != regular || gray {
                            let gray = if gray { ", not in color" } else { "" };
                            missing.push(format!("{asked}: {}, glyph {glyph}{gray} (at 400: {regular})", named(&face)));
                        }
                    }
                }
            }
            (wrong, missing)
        };
        assert!(wrong.is_empty(), "drawn in another font than the interface's Inter:\n{}", wrong.join("\n"));
        let said = "not drawn in a font of the system that has it, in its family at 400, in color where it can be";
        assert!(missing.is_empty(), "{said}:\n{}", missing.join("\n"));
    }
}
