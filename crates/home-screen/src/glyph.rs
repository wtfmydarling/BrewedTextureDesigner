//! Small line icons of the Home screen (SVG, drawn in white and tinted by the widget).

use iced::widget::svg;
use iced::{Color, Element, Length};

/// An icon of the Home screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Glyph {
    Home,
    /// A graduation cap: Learn.
    Learn,
    Plus,
    Open,
    Grid,
    List,
    Info,
    Keyboard,
    Settings,
    Globe,
    Book,
    Chat,
    People,
    Back,
    File,
    /// A ball in a material: what a material starts as.
    Material,
    /// Nodes and a link.
    Graph,
    /// Spots of different sizes: patterns scattered.
    Pattern,
    Filter,
    /// A sun: light (an environment).
    Light,
    /// A camera: what was photographed (a scan).
    Camera,
    /// Three dots in a circle: more to choose from.
    More,
    SortUp,
    SortDown,
    Warning,
}

fn path(glyph: Glyph) -> &'static str {
    match glyph {
        Glyph::Home => r#"<path d="M3.5 11.5L12 4.5l8.5 7M6 10v9.5h4.5v-5.5h3v5.5H18V10"/>"#,
        Glyph::Learn => r#"<path d="M2.5 9.5L12 5l9.5 4.5L12 14z"/><path d="M6.5 11.5V16c1.5 1.4 3.4 2.1 5.5 2.1s4-.7 5.5-2.1v-4.5M21.5 9.5v5"/>"#,
        Glyph::Plus => r#"<path d="M12 5v14M5 12h14"/>"#,
        Glyph::Open => r#"<path d="M3 7a2 2 0 0 1 2-2h4l2 2.5h8a2 2 0 0 1 2 2V18a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#,
        Glyph::Grid => {
            r#"<rect x="4" y="4" width="6.5" height="6.5" rx="1"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1"/>"#
        }
        Glyph::List => r#"<path d="M9 6.5h11M9 12h11M9 17.5h11M4.5 6.5h.5M4.5 12h.5M4.5 17.5h.5"/>"#,
        Glyph::Info => r#"<circle cx="12" cy="12" r="8.5"/><path d="M12 11v5.5M12 7.8v.2"/>"#,
        Glyph::Keyboard => r#"<rect x="2.5" y="6" width="19" height="12" rx="2"/><path d="M6 10h.2M9.5 10h.2M13 10h.2M16.5 10h1.5M6.5 14h11"/>"#,
        Glyph::Settings => r#"<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>"#,
        Glyph::Globe => {
            r#"<circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17M12 3.5c2.4 2.5 3.6 5.3 3.6 8.5s-1.2 6-3.6 8.5c-2.4-2.5-3.6-5.3-3.6-8.5s1.2-6 3.6-8.5z"/>"#
        }
        Glyph::Book => r#"<path d="M3.5 5.5c2.8-1.1 5.6-.9 8.5.6v13c-2.9-1.5-5.7-1.7-8.5-.6zM20.5 5.5c-2.8-1.1-5.6-.9-8.5.6v13c2.9-1.5 5.7-1.7 8.5-.6z"/>"#,
        Glyph::Chat => r#"<path d="M4 5.5h16v10.5H10l-4.5 3.5V16H4z"/>"#,
        Glyph::People => {
            r#"<circle cx="9" cy="8.5" r="3"/><path d="M3.5 19c.8-3.1 2.9-4.7 5.5-4.7s4.7 1.6 5.5 4.7"/><circle cx="17" cy="9" r="2.3"/><path d="M16.3 14.4c2.1.2 3.6 1.7 4.2 4.6"/>"#
        }
        Glyph::Back => r#"<path d="M19 12H5.5M11 6l-6 6 6 6"/>"#,
        Glyph::File => r#"<path d="M6 3.5h8.5l4 4v13H6z"/><path d="M14.5 3.5v4h4"/>"#,
        Glyph::Material => r#"<circle cx="12" cy="12" r="8.5"/><path d="M3.6 13c2.6 1.9 14.2 1.9 16.8 0M8.2 7.2c1.1-.7 2.4-1.1 3.8-1.2"/>"#,
        Glyph::Graph => {
            r#"<rect x="3" y="4" width="7" height="6" rx="1.5"/><rect x="14" y="14" width="7" height="6" rx="1.5"/><path d="M10 7h2.5a2 2 0 0 1 2 2v5"/>"#
        }
        Glyph::Pattern => {
            r#"<circle cx="7" cy="7.5" r="2.6"/><circle cx="16.5" cy="7" r="1.6"/><circle cx="9" cy="16.5" r="1.8"/><circle cx="16.8" cy="16" r="3.2"/>"#
        }
        Glyph::Filter => r#"<path d="M4 5h16l-6.3 7.4V19l-3.4 1.5v-8.1z"/>"#,
        Glyph::Light => {
            r#"<circle cx="12" cy="12" r="3.8"/><path d="M12 3v2.4M12 18.6V21M3 12h2.4M18.6 12H21M5.6 5.6l1.7 1.7M16.7 16.7l1.7 1.7M5.6 18.4l1.7-1.7M16.7 7.3l1.7-1.7"/>"#
        }
        Glyph::Camera => r#"<path d="M4 8.5h3.2L9 6h6l1.8 2.5H20V19H4z"/><circle cx="12" cy="13.5" r="3.3"/>"#,
        Glyph::More => r#"<circle cx="12" cy="12" r="9"/><path d="M7.9 12h.2M11.9 12h.2M15.9 12h.2" stroke-width="2.6"/>"#,
        Glyph::SortUp => r#"<path d="M8 14l4-4 4 4"/>"#,
        Glyph::SortDown => r#"<path d="M8 10l4 4 4-4"/>"#,
        Glyph::Warning => r#"<path d="M12 4l9 16H3zM12 10v4.5M12 17.4v.2"/>"#,
    }
}

fn handle(glyph: Glyph) -> svg::Handle {
    // (Handles are known by their bytes: making them for every view is cheap.)
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
        path(glyph)
    );
    svg::Handle::from_memory(svg.into_bytes())
}

/// `glyph`, `size` pixels, in `color`.
pub fn glyph<'a, Message: 'a>(glyph: Glyph, size: f32, color: Color) -> Element<'a, Message> {
    svg(handle(glyph)).width(Length::Fixed(size)).height(Length::Fixed(size)).style(move |_, _| svg::Style { color: Some(color) }).into()
}
