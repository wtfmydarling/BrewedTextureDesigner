//! Small line icons (SVG, drawn in white and tinted by the widget style).

use iced::widget::svg;
use iced::{Color, Element, Length};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    ChevronDown,
    ChevronRight,
    Plus,
    Close,
    Search,
    Eye,
    Lock,
    Pipette,
    Reset,
    Trash,
    Folder,
    Refresh,
    Grip,
    Fit,
    Grid,
    Cube,
    Image,
    Reverse,
    Distribute,
    Bypass,
    Link,
}

fn path(icon: Icon) -> &'static str {
    match icon {
        Icon::ChevronDown => r#"<path d="M6 9l6 6 6-6"/>"#,
        Icon::ChevronRight => r#"<path d="M9 6l6 6-6 6"/>"#,
        Icon::Plus => r#"<path d="M12 5v14M5 12h14"/>"#,
        Icon::Close => r#"<path d="M6 6l12 12M18 6L6 18"/>"#,
        Icon::Search => r#"<circle cx="11" cy="11" r="6.5"/><path d="M16 16l4.5 4.5"/>"#,
        Icon::Eye => r#"<path d="M2.5 12s3.5-6.5 9.5-6.5S21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z"/><circle cx="12" cy="12" r="3"/>"#,
        Icon::Lock => r#"<rect x="5" y="11" width="14" height="9.5" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>"#,
        Icon::Pipette => r#"<path d="M14.5 5.5l4 4M17 3.5a2.1 2.1 0 0 1 3 3l-2.5 2.5-3-3zM15 7l-9.5 9.5-1 3.5 3.5-1L17.5 9.5"/>"#,
        Icon::Reset => r#"<path d="M4 12a8 8 0 1 0 2.4-5.7M4 4v4.5h4.5"/>"#,
        Icon::Trash => r#"<path d="M4.5 7h15M9.5 7V4.5h5V7M6.5 7l1 13h9l1-13"/>"#,
        Icon::Folder => r#"<path d="M3 7a2 2 0 0 1 2-2h4l2 2.5h8a2 2 0 0 1 2 2V18a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#,
        Icon::Refresh => r#"<path d="M20 12a8 8 0 0 1-14.3 4.9M4 12a8 8 0 0 1 14.3-4.9M18.5 3v4.5H14M5.5 21v-4.5H10"/>"#,
        Icon::Grip => {
            r#"<circle cx="9" cy="6" r="1.2"/><circle cx="15" cy="6" r="1.2"/><circle cx="9" cy="12" r="1.2"/><circle cx="15" cy="12" r="1.2"/><circle cx="9" cy="18" r="1.2"/><circle cx="15" cy="18" r="1.2"/>"#
        }
        Icon::Fit => r#"<path d="M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5"/>"#,
        Icon::Grid => r#"<rect x="3.5" y="3.5" width="17" height="17" rx="1"/><path d="M9.2 3.5v17M14.8 3.5v17M3.5 9.2h17M3.5 14.8h17"/>"#,
        Icon::Cube => r#"<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9zM4 7.5l8 4.5 8-4.5M12 12v9"/>"#,
        Icon::Image => r#"<rect x="3.5" y="4.5" width="17" height="15" rx="2"/><circle cx="9" cy="10" r="1.8"/><path d="M4 17l5-5 4 4 3-3 4 4"/>"#,
        Icon::Reverse => r#"<path d="M4 8h14l-3.5-3.5M20 16H6l3.5 3.5"/>"#,
        Icon::Distribute => r#"<path d="M4 4v16M20 4v16M9 8h6v8H9z"/>"#,
        Icon::Bypass => r#"<path d="M3 12h6M15 12h6M9 12c0-5 6-5 6 0"/><path d="M9 12c0 5 6 5 6 0" stroke-dasharray="2 2"/>"#,
        Icon::Link => r#"<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>"#,
    }
}

fn handle(icon: Icon) -> svg::Handle {
    // Handles are cached by content, so building them on every view is cheap.
    let fill = if icon == Icon::Grip { "white" } else { "none" };
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="{fill}" stroke="white" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
        path(icon)
    );
    svg::Handle::from_memory(svg.into_bytes())
}

/// An icon of `size` pixels tinted with `color`.
pub fn icon<'a, Message: 'a>(icon: Icon, size: f32, color: Color) -> Element<'a, Message> {
    svg(handle(icon))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}
