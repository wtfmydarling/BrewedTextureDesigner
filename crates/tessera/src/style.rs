//! Colors, fonts and widget styles of the dark theme.
//!
//! Neutral, slightly cool grays in a few steps (window, panel, raised, hover), hairline
//! borders, small radii. The amber accent marks what is active or selected and is used
//! sparingly: outlines, text and thin fills rather than large blocks.

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

use tessera_core::{Category, DataType};

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

// Surfaces, darkest to lightest.
/// Behind the panels: the gaps between them, the menu and status bars.
pub const WINDOW: Color = rgb(17, 18, 20);
/// The graph and the 2D view.
pub const CANVAS: Color = rgb(22, 23, 26);
pub const GRID_MINOR: Color = rgb(28, 29, 33);
pub const GRID_MAJOR: Color = rgb(36, 38, 42);
pub const FIELD: Color = rgb(20, 21, 24);
pub const PANEL_DARK: Color = rgb(24, 25, 28);
pub const PANEL: Color = rgb(29, 30, 34);
/// Pane headers and cards.
pub const HEADER: Color = rgb(34, 35, 40);
pub const RAISED: Color = rgb(42, 44, 50);
pub const HOVER: Color = rgb(52, 55, 62);
pub const BORDER: Color = rgb(12, 12, 14);
pub const BORDER_LIGHT: Color = rgb(56, 59, 66);
/// Hairlines between rows and sections.
pub const LINE: Color = rgb(40, 42, 47);

pub const TEXT: Color = rgb(216, 218, 222);
pub const TEXT_DIM: Color = rgb(145, 149, 157);
pub const TEXT_FAINT: Color = rgb(98, 102, 110);

pub const ACCENT: Color = rgb(230, 160, 62);
pub const ACCENT_DIM: Color = rgb(128, 90, 40);
/// Accent for text on dark surfaces.
pub const ACCENT_TEXT: Color = rgb(240, 186, 110);
pub const ERROR: Color = rgb(232, 88, 76);
pub const WARNING: Color = rgb(228, 186, 72);
pub const SUCCESS: Color = rgb(120, 196, 134);

/// Filled part of number fields (how far the value is in its range).
pub const VALUE_FILL: Color = rgb(52, 56, 64);
pub const VALUE_FILL_HOT: Color = rgb(64, 69, 79);

pub const NODE_BODY: Color = rgb(40, 42, 47);
pub const NODE_BYPASSED: Color = rgb(34, 35, 39);
pub const NODE_OUTLINE: Color = rgb(14, 14, 16);
pub const THUMB_BACKGROUND: Color = rgb(18, 18, 20);
pub const SOCKET_EMPTY: Color = rgb(28, 29, 33);
/// Text on nodes (socket names, values).
pub const NODE_TEXT: Color = rgb(206, 208, 212);
pub const EXPRESSION: Color = rgb(226, 206, 156);

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

pub fn category_color(c: Category) -> Color {
    match c {
        Category::Values => rgb(132, 102, 46),
        Category::Generators => rgb(52, 110, 80),
        Category::Filters => rgb(50, 88, 138),
        Category::Effects => rgb(36, 106, 114),
        Category::Adjustments => rgb(100, 74, 138),
        Category::Output => rgb(136, 60, 60),
    }
}

pub fn alpha(c: Color, a: f32) -> Color {
    Color { a: c.a * a, ..c }
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

/// Display color of an RGBA value (values are shown as stored, like the 2D view).
pub fn color_of(c: [f32; 4]) -> Color {
    let q = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    Color { r: q(c[0]), g: q(c[1]), b: q(c[2]), a: q(c[3]) }
}

pub fn ui_font() -> Font {
    if cfg!(windows) { Font::with_name("Segoe UI") } else { Font::DEFAULT }
}

pub fn bold() -> Font {
    Font { weight: iced::font::Weight::Semibold, ..ui_font() }
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
        "Tessera".to_string(),
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

pub fn card(_: &Theme) -> container::Style {
    container::Style {
        background: Some(HEADER.into()),
        border: Border { color: LINE, width: 1.0, radius: 3.0.into() },
        ..Default::default()
    }
}

pub fn field_box(_: &Theme) -> container::Style {
    container::Style {
        background: Some(FIELD.into()),
        border: Border { color: LINE, width: 1.0, radius: 3.0.into() },
        ..Default::default()
    }
}

pub fn popup(_: &Theme) -> container::Style {
    container::Style {
        background: Some(rgb(33, 34, 39).into()),
        border: Border { color: BORDER_LIGHT, width: 1.0, radius: 4.0.into() },
        shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.5), offset: Vector::new(0.0, 6.0), blur_radius: 16.0 },
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

pub fn chip(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: Some(color.into()), border: border::rounded(1), ..Default::default() }
}

pub fn tooltip(_: &Theme) -> container::Style {
    container::Style {
        background: Some(rgb(20, 21, 24).into()),
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
        button_w::Status::Active => button_base(Some(RAISED), TEXT, rgb(50, 53, 60)),
        button_w::Status::Hovered => button_base(Some(HOVER), TEXT, BORDER_LIGHT),
        button_w::Status::Pressed => button_base(Some(rgb(46, 48, 54)), ACCENT_TEXT, ACCENT_DIM),
        button_w::Status::Disabled => button_base(Some(PANEL), TEXT_FAINT, LINE),
    }
}

/// The default action of a dialog.
pub fn primary_button(_: &Theme, status: button_w::Status) -> button_w::Style {
    match status {
        button_w::Status::Active => button_base(Some(ACCENT), rgb(26, 20, 12), ACCENT),
        button_w::Status::Hovered => button_base(Some(rgb(242, 176, 84)), rgb(26, 20, 12), rgb(242, 176, 84)),
        button_w::Status::Pressed => button_base(Some(rgb(206, 140, 50)), rgb(26, 20, 12), rgb(206, 140, 50)),
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

/// A row of a list or menu: full width, highlighted when hovered or selected.
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

// ---------------------------------------------------------------------------- inputs

pub fn text_input(_: &Theme, status: text_input_w::Status) -> text_input_w::Style {
    let border = match status {
        text_input_w::Status::Focused { .. } => ACCENT,
        text_input_w::Status::Hovered => BORDER_LIGHT,
        _ => LINE,
    };
    text_input_w::Style {
        background: FIELD.into(),
        border: Border { color: border, width: 1.0, radius: 3.0.into() },
        icon: TEXT_DIM,
        placeholder: TEXT_FAINT,
        value: if status == text_input_w::Status::Disabled { TEXT_DIM } else { TEXT },
        selection: alpha(ACCENT, 0.4),
    }
}

pub fn text_editor(_: &Theme, status: text_editor_w::Status) -> text_editor_w::Style {
    let border = match status {
        text_editor_w::Status::Focused { .. } => ACCENT,
        text_editor_w::Status::Hovered => BORDER_LIGHT,
        _ => LINE,
    };
    text_editor_w::Style {
        background: FIELD.into(),
        border: Border { color: border, width: 1.0, radius: 3.0.into() },
        placeholder: TEXT_FAINT,
        value: TEXT,
        selection: alpha(ACCENT, 0.4),
    }
}

pub fn pick_list(_: &Theme, status: pick_list_w::Status) -> pick_list_w::Style {
    let (bg, border) = match status {
        pick_list_w::Status::Hovered => (RAISED, BORDER_LIGHT),
        pick_list_w::Status::Opened { .. } => (RAISED, ACCENT),
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
        (true, _) => if hovered { rgb(242, 176, 84) } else { ACCENT },
        (false, _) => FIELD,
    };
    toggler_w::Style {
        background: track.into(),
        background_border_width: 1.0,
        background_border_color: if on { track } else if hovered { BORDER_LIGHT } else { LINE },
        foreground: if on { rgb(30, 24, 16) } else if hovered { TEXT } else { TEXT_DIM }.into(),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        text_color: Some(TEXT),
        border_radius: None,
        padding_ratio: 0.16,
    }
}

pub fn scrollable(_: &Theme, status: scrollable_w::Status) -> scrollable_w::Style {
    let hovered = matches!(status, scrollable_w::Status::Hovered { .. } | scrollable_w::Status::Dragged { .. });
    let rail = scrollable_w::Rail {
        background: None,
        border: border::rounded(3),
        scroller: scrollable_w::Scroller {
            background: if hovered { rgb(84, 88, 96) } else { rgb(58, 61, 68) }.into(),
            border: border::rounded(3),
        },
    };
    scrollable_w::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable_w::AutoScroll {
            background: alpha(PANEL, 0.9).into(),
            border: border::rounded(u32::MAX).width(1).color(TEXT_DIM),
            shadow: Shadow::default(),
            icon: TEXT_DIM,
        },
    }
}
