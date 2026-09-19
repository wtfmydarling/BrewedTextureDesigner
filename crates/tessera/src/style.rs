//! Colors, fonts and widget styles of the dark theme.

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

pub const CANVAS: Color = rgb(24, 25, 28);
pub const GRID_MINOR: Color = rgb(31, 32, 36);
pub const GRID_MAJOR: Color = rgb(40, 41, 46);
pub const PANEL: Color = rgb(31, 32, 36);
pub const PANEL_DARK: Color = rgb(26, 27, 30);
pub const FIELD: Color = rgb(22, 23, 26);
pub const RAISED: Color = rgb(42, 44, 49);
pub const HOVER: Color = rgb(52, 54, 60);
pub const BORDER: Color = rgb(16, 16, 18);
pub const BORDER_LIGHT: Color = rgb(60, 62, 68);
pub const TEXT: Color = rgb(222, 223, 226);
pub const TEXT_DIM: Color = rgb(140, 143, 150);
pub const TEXT_FAINT: Color = rgb(96, 99, 106);
pub const ACCENT: Color = rgb(240, 160, 48);
pub const ACCENT_DIM: Color = rgb(150, 100, 34);
pub const ERROR: Color = rgb(235, 86, 74);
pub const WARNING: Color = rgb(232, 190, 72);
pub const SUCCESS: Color = rgb(128, 200, 138);
pub const NODE_BODY: Color = rgb(44, 46, 51);
pub const EXPRESSION: Color = rgb(230, 210, 160);

pub fn type_color(ty: DataType) -> Color {
    match ty {
        DataType::Image => rgb(205, 205, 205),
        DataType::Float => rgb(90, 180, 255),
        DataType::Int => rgb(80, 210, 140),
        DataType::Bool => rgb(255, 110, 110),
        DataType::Vec2 => rgb(255, 205, 100),
        DataType::Color => rgb(200, 146, 234),
    }
}

pub fn category_color(c: Category) -> Color {
    match c {
        Category::Values => rgb(150, 112, 34),
        Category::Generators => rgb(46, 118, 78),
        Category::Filters => rgb(44, 92, 146),
        Category::Effects => rgb(30, 118, 128),
        Category::Adjustments => rgb(110, 74, 150),
        Category::Output => rgb(150, 58, 58),
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

pub fn header_bar(_: &Theme) -> container::Style {
    container::Style {
        background: Some(PANEL_DARK.into()),
        border: Border { color: BORDER, width: 1.0, radius: 0.0.into() },
        text_color: Some(TEXT),
        ..Default::default()
    }
}

pub fn canvas_frame(_: &Theme) -> container::Style {
    container::Style { background: Some(CANVAS.into()), ..Default::default() }
}

pub fn card(_: &Theme) -> container::Style {
    container::Style {
        background: Some(RAISED.into()),
        border: Border { color: BORDER, width: 1.0, radius: 4.0.into() },
        ..Default::default()
    }
}

pub fn field_box(_: &Theme) -> container::Style {
    container::Style {
        background: Some(FIELD.into()),
        border: Border { color: BORDER, width: 1.0, radius: 3.0.into() },
        ..Default::default()
    }
}

pub fn popup(_: &Theme) -> container::Style {
    container::Style {
        background: Some(rgb(37, 38, 43).into()),
        border: Border { color: BORDER_LIGHT, width: 1.0, radius: 6.0.into() },
        shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.55), offset: Vector::new(0.0, 6.0), blur_radius: 18.0 },
        text_color: Some(TEXT),
        ..Default::default()
    }
}

pub fn scrim(_: &Theme) -> container::Style {
    container::Style { background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()), ..Default::default() }
}

pub fn message_box(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(alpha(color, 0.14).into()),
        border: Border { color: alpha(color, 0.8), width: 1.0, radius: 4.0.into() },
        text_color: Some(mix(color, TEXT, 0.35)),
        ..Default::default()
    }
}

pub fn stripe(even: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: (!even).then(|| Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.025))),
        border: border::rounded(3),
        ..Default::default()
    }
}

pub fn chip(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: Some(color.into()), border: border::rounded(2), ..Default::default() }
}

pub fn tooltip(_: &Theme) -> container::Style {
    container::Style {
        background: Some(rgb(20, 21, 24).into()),
        border: Border { color: BORDER_LIGHT, width: 1.0, radius: 4.0.into() },
        text_color: Some(TEXT),
        shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.5), offset: Vector::new(0.0, 3.0), blur_radius: 8.0 },
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------- buttons

fn button_base(bg: Option<Color>, text: Color, border: Color) -> button_w::Style {
    button_w::Style {
        background: bg.map(Background::Color),
        text_color: text,
        border: Border { color: border, width: 1.0, radius: 4.0.into() },
        shadow: Shadow::default(),
        snap: true,
    }
}

/// Regular push button.
pub fn button(_: &Theme, status: button_w::Status) -> button_w::Style {
    match status {
        button_w::Status::Active => button_base(Some(RAISED), TEXT, BORDER),
        button_w::Status::Hovered => button_base(Some(HOVER), TEXT, BORDER_LIGHT),
        button_w::Status::Pressed => button_base(Some(ACCENT_DIM), TEXT, ACCENT),
        button_w::Status::Disabled => button_base(Some(PANEL), TEXT_FAINT, BORDER),
    }
}

/// Highlighted action (default button of a dialog).
pub fn primary_button(_: &Theme, status: button_w::Status) -> button_w::Style {
    match status {
        button_w::Status::Active => button_base(Some(ACCENT_DIM), TEXT, ACCENT),
        button_w::Status::Hovered => button_base(Some(rgb(180, 120, 40)), Color::WHITE, ACCENT),
        button_w::Status::Pressed => button_base(Some(ACCENT), Color::BLACK, ACCENT),
        button_w::Status::Disabled => button_base(Some(PANEL), TEXT_FAINT, BORDER),
    }
}

/// Flat button without chrome until hovered (toolbars, icon buttons).
pub fn flat_button(_: &Theme, status: button_w::Status) -> button_w::Style {
    let transparent = Color::TRANSPARENT;
    match status {
        button_w::Status::Active => button_base(None, TEXT, transparent),
        button_w::Status::Hovered => button_base(Some(HOVER), TEXT, transparent),
        button_w::Status::Pressed => button_base(Some(ACCENT_DIM), TEXT, transparent),
        button_w::Status::Disabled => button_base(None, TEXT_FAINT, transparent),
    }
}

/// Toggle-like button: `on` shows the accent.
pub fn toggle_button(on: bool) -> impl Fn(&Theme, button_w::Status) -> button_w::Style {
    move |theme, status| {
        if on {
            match status {
                button_w::Status::Hovered => button_base(Some(rgb(120, 82, 30)), Color::WHITE, ACCENT),
                _ => button_base(Some(ACCENT_DIM), Color::WHITE, ACCENT),
            }
        } else {
            flat_button(theme, status)
        }
    }
}

/// A row of a list or menu: full width, highlighted when hovered or selected.
pub fn row_button(selected: bool) -> impl Fn(&Theme, button_w::Status) -> button_w::Style {
    move |_, status| {
        let bg = match (selected, status) {
            (_, button_w::Status::Pressed) => Some(ACCENT_DIM),
            (true, _) => Some(rgb(70, 58, 38)),
            (false, button_w::Status::Hovered) => Some(HOVER),
            _ => None,
        };
        let text = if status == button_w::Status::Disabled { TEXT_FAINT } else { TEXT };
        button_w::Style {
            background: bg.map(Background::Color),
            text_color: text,
            border: border::rounded(3),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

// ---------------------------------------------------------------------------- inputs

pub fn text_input(_: &Theme, status: text_input_w::Status) -> text_input_w::Style {
    let border = match status {
        text_input_w::Status::Focused { .. } => ACCENT,
        text_input_w::Status::Hovered => BORDER_LIGHT,
        _ => BORDER,
    };
    text_input_w::Style {
        background: FIELD.into(),
        border: Border { color: border, width: 1.0, radius: 3.0.into() },
        icon: TEXT_DIM,
        placeholder: TEXT_FAINT,
        value: if status == text_input_w::Status::Disabled { TEXT_DIM } else { TEXT },
        selection: alpha(ACCENT, 0.45),
    }
}

pub fn text_editor(_: &Theme, status: text_editor_w::Status) -> text_editor_w::Style {
    let border = match status {
        text_editor_w::Status::Focused { .. } => ACCENT,
        text_editor_w::Status::Hovered => BORDER_LIGHT,
        _ => BORDER,
    };
    text_editor_w::Style {
        background: FIELD.into(),
        border: Border { color: border, width: 1.0, radius: 3.0.into() },
        placeholder: TEXT_FAINT,
        value: TEXT,
        selection: alpha(ACCENT, 0.45),
    }
}

pub fn pick_list(_: &Theme, status: pick_list_w::Status) -> pick_list_w::Style {
    let (bg, border) = match status {
        pick_list_w::Status::Hovered => (RAISED, BORDER_LIGHT),
        pick_list_w::Status::Opened { .. } => (RAISED, ACCENT),
        _ => (FIELD, BORDER),
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
    let bg = match (on, disabled) {
        (_, true) => PANEL,
        (true, _) => ACCENT_DIM,
        (false, _) => FIELD,
    };
    toggler_w::Style {
        background: bg.into(),
        background_border_width: 1.0,
        background_border_color: if hovered { BORDER_LIGHT } else { BORDER },
        foreground: if on { ACCENT } else { TEXT_DIM }.into(),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        text_color: Some(TEXT),
        border_radius: None,
        padding_ratio: 0.12,
    }
}

pub fn scrollable(_: &Theme, status: scrollable_w::Status) -> scrollable_w::Style {
    let hovered = matches!(status, scrollable_w::Status::Hovered { .. } | scrollable_w::Status::Dragged { .. });
    let rail = scrollable_w::Rail {
        background: None,
        border: border::rounded(3),
        scroller: scrollable_w::Scroller {
            background: if hovered { rgb(90, 92, 100) } else { rgb(64, 66, 72) }.into(),
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
