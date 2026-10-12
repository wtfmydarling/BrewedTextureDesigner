//! How the Home screen looks: its colors, its fonts and the look of its widgets. What it is by
//! default is the look of the set of programs; a program may give its own.

use iced::border::{self, Border};
use iced::font::Weight;
use iced::widget::{button, container, rule};
use iced::{Background, Color, Font, Shadow, Theme, Vector};

/// The look of the Home screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    /// The font of its text (its weights are taken of it): the program loads it.
    pub font: Font,
    /// The size of text, and of small text.
    pub text_size: f32,
    pub small_size: f32,
    /// The background of the side bar, of the page, and of a card or a tile on it.
    pub sidebar: Color,
    pub page: Color,
    pub card: Color,
    /// A card or tile pointed at, and the page chosen in the side bar.
    pub hover: Color,
    pub chosen: Color,
    /// Lines between the parts, and around cards.
    pub line: Color,
    /// Text: headings and what is chosen, text, what is said of it, and what is only hinted.
    pub bright: Color,
    pub text: Color,
    pub dim: Color,
    pub faint: Color,
    /// The button that starts something new, and a warning (a file not where it was).
    pub primary: Color,
    pub warning: Color,
}

const fn gray(v: u8) -> Color {
    Color::from_rgb(v as f32 / 255.0, v as f32 / 255.0, v as f32 / 255.0)
}

impl Default for Style {
    /// The look of the set: dark neutral grays, Inter.
    fn default() -> Style {
        Style {
            font: Font::with_name("Inter Variable"),
            text_size: 13.0,
            small_size: 12.0,
            sidebar: gray(24),
            page: gray(30),
            card: gray(37),
            hover: gray(46),
            chosen: gray(17),
            line: gray(48),
            bright: gray(255),
            text: gray(222),
            dim: gray(150),
            faint: gray(108),
            primary: gray(236),
            warning: Color::from_rgb(228.0 / 255.0, 186.0 / 255.0, 72.0 / 255.0),
        }
    }
}

impl Style {
    /// The font in `weight`.
    pub fn weight(&self, weight: Weight) -> Font {
        Font { weight, ..self.font }
    }

    /// What a button that starts something says: a little heavier than text.
    pub fn medium(&self) -> Font {
        self.weight(Weight::Medium)
    }

    pub fn semibold(&self) -> Font {
        self.weight(Weight::Semibold)
    }

    /// Titles and headings.
    pub fn heavy(&self) -> Font {
        self.weight(Weight::Bold)
    }

    pub(crate) fn sidebar(self) -> impl Fn(&Theme) -> container::Style {
        move |_| container::Style { background: Some(self.sidebar.into()), text_color: Some(self.text), ..Default::default() }
    }

    pub(crate) fn page(self) -> impl Fn(&Theme) -> container::Style {
        move |_| container::Style { background: Some(self.page.into()), text_color: Some(self.text), ..Default::default() }
    }

    pub(crate) fn card(self) -> impl Fn(&Theme) -> container::Style {
        move |_| container::Style {
            background: Some(self.card.into()),
            border: Border { color: self.line, width: 1.0, radius: 4.0.into() },
            ..Default::default()
        }
    }

    /// The frame of a picture.
    pub(crate) fn frame(self) -> impl Fn(&Theme) -> container::Style {
        move |_| container::Style {
            background: Some(self.chosen.into()),
            border: Border { color: gray(12), width: 1.0, radius: 3.0.into() },
            ..Default::default()
        }
    }

    pub(crate) fn tooltip(self) -> impl Fn(&Theme) -> container::Style {
        move |_| container::Style {
            background: Some(gray(22).into()),
            border: Border { color: gray(68), width: 1.0, radius: 3.0.into() },
            text_color: Some(self.text),
            shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.45), offset: Vector::new(0.0, 3.0), blur_radius: 8.0 },
            ..Default::default()
        }
    }

    /// A line between the parts.
    pub(crate) fn rule(self) -> impl Fn(&Theme) -> rule::Style {
        move |_| rule::Style { color: self.line, radius: 0.0.into(), fill_mode: rule::FillMode::Full, snap: true }
    }

    /// New…: light, with dark text.
    pub(crate) fn primary(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| {
            let bg = match status {
                button::Status::Hovered => gray(250),
                button::Status::Pressed => gray(205),
                button::Status::Disabled => self.card,
                button::Status::Active => self.primary,
            };
            base(Some(bg), gray(18), bg)
        }
    }

    /// Open… and the buttons of cards.
    pub(crate) fn secondary(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| match status {
            button::Status::Hovered => base(Some(gray(58)), self.text, gray(68)),
            button::Status::Pressed => base(Some(gray(52)), self.bright, gray(120)),
            button::Status::Disabled => base(Some(self.card), self.faint, self.line),
            button::Status::Active => base(Some(gray(47)), self.text, gray(56)),
        }
    }

    /// A page of the side bar: when it is the one shown, darker than the bar, its text white;
    /// else nothing until pointed at.
    pub(crate) fn tab(self, chosen: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| {
            let (bg, text) = match (chosen, status) {
                (true, _) => (Some(self.chosen), self.bright),
                (false, button::Status::Hovered) => (Some(self.hover), self.text),
                (false, button::Status::Pressed) => (Some(gray(52)), self.bright),
                (false, _) => (None, self.dim),
            };
            base(bg, text, Color::TRANSPARENT)
        }
    }

    /// A link of the side bar, a row of a list, a heading of a column: nothing until pointed at.
    pub(crate) fn flat(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| match status {
            button::Status::Hovered => base(Some(self.hover), self.text, Color::TRANSPARENT),
            button::Status::Pressed => base(Some(gray(52)), self.bright, Color::TRANSPARENT),
            button::Status::Disabled => base(None, self.faint, Color::TRANSPARENT),
            button::Status::Active => base(None, self.text, Color::TRANSPARENT),
        }
    }

    /// A card of what a new file starts from: framed, lit when pointed at, outlined white when
    /// it is the one chosen.
    pub(crate) fn template_card(self, chosen: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| {
            let bg = match status {
                button::Status::Hovered => self.hover,
                button::Status::Pressed => gray(52),
                _ => self.card,
            };
            let border = if chosen { Border { color: gray(214), width: 1.5, radius: 6.0.into() } } else { Border { color: self.line, width: 1.0, radius: 6.0.into() } };
            button::Style { background: Some(Background::Color(bg)), text_color: self.text, border, shadow: Shadow::default(), snap: true }
        }
    }

    /// The dark round behind an (i) on a picture.
    pub(crate) fn badge(self) -> impl Fn(&Theme) -> container::Style {
        move |_| container::Style { background: Some(Color { a: 0.7, ..gray(12) }.into()), border: border::rounded(20), ..Default::default() }
    }

    /// A tile (what to start from, a file opened last): a card, lit when pointed at.
    pub(crate) fn tile(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| {
            let (bg, line) = match status {
                button::Status::Hovered => (self.hover, gray(78)),
                button::Status::Pressed => (gray(52), gray(92)),
                _ => (self.card, self.line),
            };
            let text = if status == button::Status::Disabled { self.faint } else { self.text };
            button::Style { background: Some(Background::Color(bg)), text_color: text, border: Border { color: line, width: 1.0, radius: 6.0.into() }, shadow: Shadow::default(), snap: true }
        }
    }

    /// An option of two or three side by side (the grid or the list): raised when chosen.
    pub(crate) fn segment(self, chosen: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| {
            let (bg, text) = match (chosen, status) {
                (true, _) => (Some(gray(78)), self.bright),
                (false, button::Status::Hovered) => (Some(gray(44)), self.text),
                (false, _) => (None, self.dim),
            };
            button::Style { background: bg.map(Background::Color), text_color: text, border: border::rounded(3), shadow: Shadow::default(), snap: true }
        }
    }

    pub(crate) fn segments(self) -> impl Fn(&Theme) -> container::Style {
        move |_| container::Style {
            background: Some(gray(21).into()),
            border: Border { color: self.line, width: 1.0, radius: 4.0.into() },
            ..Default::default()
        }
    }
}

fn base(bg: Option<Color>, text: Color, line: Color) -> button::Style {
    button::Style {
        background: bg.map(Background::Color),
        text_color: text,
        border: Border { color: line, width: 1.0, radius: 4.0.into() },
        shadow: Shadow::default(),
        snap: true,
    }
}
