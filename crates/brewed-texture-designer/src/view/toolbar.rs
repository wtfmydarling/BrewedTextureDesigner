//! The bars around the panes: the menu bar at the top of the window, the status bar at
//! the bottom, and the header of the graph pane with the texture size.

use std::time::Duration;

use brewed_texture_core::{Edit, Precision};
use iced::widget::{button, column, container, image, mouse_area, pick_list, row, rule, space, stack, text, tooltip};
use iced::{Alignment, Element, Length, Padding};

use crate::app::{App, Dialog, ExplorerMessage, MenuId, Message, OWN_TITLE_BAR, PreviewMessage, SettingsMessage, SubMenu, WindowAction};
use crate::icons::{Icon, icon};
use crate::shortcuts::Command;
use crate::style;
use crate::view::tip;
use crate::widget::dropdown::dropdown;

/// The texture sizes offered (a graph's, and a new graph's in the settings).
pub(crate) const RESOLUTIONS: [u32; 7] = [128, 256, 512, 1024, 2048, 4096, 8192];
pub(crate) const MENU_BAR_H: f32 = 30.0;
/// How wide a button of the window's own title bar is (as the system's are).
pub(crate) const WINDOW_BUTTON_W: f32 = 46.0;
pub(crate) const STATUS_BAR_H: f32 = 24.0;

/// An item of a menu, and how wide the menu has to be to show it whole.
pub(crate) struct Entry<'a> {
    pub element: Element<'a, Message>,
    pub width: f32,
}

impl<'a> Entry<'a> {
    pub fn new(element: impl Into<Element<'a, Message>>, width: f32) -> Self {
        Entry { element: element.into(), width }
    }

    /// The same item in what else it is (a row that closes a submenu when gone to, say).
    fn map(self, f: impl FnOnce(Element<'a, Message>) -> Element<'a, Message>) -> Self {
        Entry { element: f(self.element), width: self.width }
    }
}

/// How far an item of a menu is padded on each side, and how far what is on its right (its
/// keys, a folder) keeps from its label.
pub(crate) const ITEM_PAD: f32 = 10.0;
pub(crate) const ITEM_GAP: f32 = 16.0;

/// How wide a row of a menu is: its label and, on its right, what goes with it (the space that
/// pushes that to the right between them, a gap on each side of it; a pixel for the rounding).
pub(crate) fn row_width(label: &str, right: &str) -> f32 {
    let right = if right.is_empty() { 0.0 } else { crate::view::text_width(right, style::SMALL) };
    2.0 * ITEM_PAD + crate::view::text_width(label, style::TEXT_SIZE) + 2.0 * ITEM_GAP + right + 1.0
}

/// How wide a menu of `items` is: as its widest item, and at least `width`.
pub(crate) fn menu_width(items: &[Entry<'_>], width: f32) -> f32 {
    items.iter().fold(width, |w, i| w.max(i.width.ceil()))
}

/// The items of a menu in a column as wide as the widest of them, and at least `width`.
pub(crate) fn menu_column<'a>(items: Vec<Entry<'a>>, width: f32) -> iced::widget::Column<'a, Message> {
    let width = menu_width(&items, width);
    column(items.into_iter().map(|i| i.element)).spacing(1).width(Length::Fixed(width))
}

fn item<'a>(label: impl Into<String>, shortcut: impl Into<String>, message: Option<Message>) -> Entry<'a> {
    let (label, shortcut) = (label.into(), shortcut.into());
    let width = row_width(&label, &shortcut);
    let enabled = message.is_some();
    let color = if enabled { style::TEXT } else { style::TEXT_FAINT };
    let element = button(
        row![
            text(label).size(style::TEXT_SIZE).color(color).wrapping(text::Wrapping::None),
            space().width(Length::Fill),
            text(shortcut).size(style::SMALL).color(if enabled { style::TEXT_DIM } else { style::TEXT_FAINT }).wrapping(text::Wrapping::None),
        ]
        .spacing(ITEM_GAP)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([4.0, ITEM_PAD])
    .style(style::row_button(false))
    .on_press_maybe(message.map(|m| Message::FromMenu(Box::new(m))));
    Entry::new(element, width)
}

pub(crate) fn separator<'a>() -> Entry<'a> {
    let line = container(rule::horizontal(1).style(|_| rule::Style { color: style::LINE, radius: 0.0.into(), fill_mode: rule::FillMode::Full, snap: true }))
        .padding([3, 6]);
    Entry::new(line, 0.0)
}

fn menu_body<'a>(items: Vec<Entry<'a>>) -> Element<'a, Message> {
    menu_of_width(items, 250.0)
}

/// A menu at least `width` wide, and as wide as its widest item.
fn menu_of_width<'a>(items: Vec<Entry<'a>>, width: f32) -> Element<'a, Message> {
    container(menu_column(items, width)).padding(4).style(style::popup).into()
}

/// An item of a menu that has a submenu: gone to, it closes the submenu open.
fn beside_submenu(item: Entry<'_>) -> Entry<'_> {
    item.map(|e| iced::widget::mouse_area(e).on_enter(Message::SubMenu(None)).into())
}

/// Open Recent Package ▸, whose submenu (open while the item is pointed at) lists the packages
/// opened or saved last, as many as the settings say, each by its name and folder, then Clear
/// Recent Packages; or says there are none.
fn recent_packages(app: &App) -> Entry<'_> {
    let open = app.ui.submenu == Some(SubMenu::Recent);
    let offered = app.settings.recent_packages as usize;
    let mut items: Vec<Entry<'_>> = app
        .ui
        .recent
        .iter()
        .take(offered)
        .map(|path| {
            let p = std::path::Path::new(path);
            let name = p.file_name().map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
            let folder = p.parent().and_then(|f| f.file_name()).map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            recent_item(name, folder, Message::OpenRecent(path.clone()))
        })
        .collect();
    if items.is_empty() {
        items.push(item("No recent packages", "", None));
    } else {
        items.extend([separator(), item("Clear Recent Packages", "", Some(Message::ClearRecent))]);
    }
    let (title, arrow) = ("Open Recent Package", "▸");
    let label = button(
        row![text(title).size(style::TEXT_SIZE).color(style::TEXT), space().width(Length::Fill), text(arrow).size(style::SMALL).color(style::TEXT_DIM),]
            .spacing(ITEM_GAP)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([4.0, ITEM_PAD])
    .style(style::row_button(open))
    .on_press(Message::SubMenu(Some(SubMenu::Recent)));
    let anchor = iced::widget::mouse_area(label).on_enter(Message::SubMenu(Some(SubMenu::Recent)));
    Entry::new(dropdown(anchor, menu_of_width(items, 320.0), open).beside().on_dismiss(Message::Menu(None)), row_width(title, arrow))
}

fn menu<'a>(app: &'a App, id: MenuId, title: &'a str, items: Vec<Entry<'a>>) -> Element<'a, Message> {
    let open = app.ui.menu == Some(id);
    let title_button = iced::widget::mouse_area(
        button(text(title).size(style::TEXT_SIZE)).padding([4, 10]).style(style::toggle_button(open)).on_press(Message::Menu(Some(id))),
    )
    .on_enter(Message::MenuHover(id));
    dropdown(title_button, menu_body(items), open).on_dismiss(Message::Menu(None)).into()
}

fn file_menu(app: &App) -> Vec<Entry<'_>> {
    let key = app.doc_key;
    let h = beside_submenu;
    let mut items = vec![
        h(item("Home Screen", "", Some(Message::Home(crate::app::home::HomeMessage::Show(crate::app::home::HomePage::Home))))),
        separator(),
        h(item("New Graph…", app.key_of(Command::New), Some(Message::NewGraph(crate::app::home::NewGraphMessage::Open(None))))),
        h(item("New Package", "", Some(Message::Explorer(ExplorerMessage::NewPackage)))),
        h(item("Open Package(s)…", app.key_of(Command::Open), Some(Message::Open))),
        recent_packages(app),
    ];
    let items_after: Vec<Entry<'_>> = vec![
        separator(),
        item("Import and Link Files…", "", Some(Message::Explorer(ExplorerMessage::LinkFiles(key)))),
        item("Import and Embed Files…", "", Some(Message::Explorer(ExplorerMessage::ImportFiles(key)))),
        item("Collect Files", "", Some(Message::Explorer(ExplorerMessage::CollectFiles(key)))),
        separator(),
        item("Save", app.key_of(Command::Save), Some(Message::Save)),
        item("Save As…", app.key_of(Command::SaveAs), Some(Message::SaveAs)),
        item("Package History…", "", app.doc.path().is_some().then_some(Message::Versions(crate::app::versions::VersionsMessage::Open))),
        item("Close Package", "", Some(Message::Explorer(ExplorerMessage::Close(key)))),
        separator(),
        item("Export…", app.key_of(Command::Export), Some(Message::Export(crate::app::export_dialog::ExportMessage::Open))),
        separator(),
        item("Quit", "", Some(Message::Quit)),
    ];
    items.extend(items_after.into_iter().map(h));
    items
}

/// A package of the recent ones: its name, and on the right, the folder it is in (cut short
/// to fit the menu).
fn recent_item<'a>(name: String, folder: String, message: Message) -> Entry<'a> {
    let folder: String = if folder.chars().count() > 18 { folder.chars().take(17).chain(['…']).collect() } else { folder };
    let width = row_width(&name, &folder);
    let element = button(
        row![
            text(name).size(style::TEXT_SIZE).wrapping(text::Wrapping::None),
            space().width(Length::Fill),
            text(folder).size(style::SMALL).color(style::TEXT_DIM).wrapping(text::Wrapping::None),
        ]
        .spacing(ITEM_GAP)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([4.0, ITEM_PAD])
    .style(style::row_button(false))
    .on_press(Message::FromMenu(Box::new(message)));
    Entry::new(element, width)
}

fn edit_menu(app: &App) -> Vec<Entry<'_>> {
    let has_selection = !app.ui.selection.is_empty();
    let sel = |m: Message| has_selection.then_some(m);
    let undo = app.doc.undo_label().map_or_else(|| "Undo".to_string(), |l| format!("Undo {l}"));
    let redo = app.doc.redo_label().map_or_else(|| "Redo".to_string(), |l| format!("Redo {l}"));
    let bypass = app.ui.selection.iter().next().copied().filter(|id| {
        app.graph().node(*id).is_some_and(|n| n.def().has_image() && !n.def().inputs.is_empty() && n.kind != brewed_texture_core::NodeKind::OUTPUT)
    });
    vec![
        item(undo, app.key_of(Command::Undo), app.doc.can_undo().then_some(Message::Undo)),
        item(redo, app.key_of(Command::Redo), app.doc.can_redo().then_some(Message::Redo)),
        separator(),
        item("Copy", app.key_of(Command::Copy), sel(Message::Copy)),
        item("Cut", app.key_of(Command::Cut), sel(Message::Cut)),
        item("Paste", app.key_of(Command::Paste), Some(Message::Paste)),
        item("Duplicate", app.key_of(Command::Duplicate), sel(Message::Duplicate)),
        item(
            "Delete",
            app.key_of(Command::Delete),
            (has_selection || app.ui.selected_link.is_some() || app.ui.selected_comment.is_some()).then_some(Message::Delete),
        ),
        separator(),
        item("Select All", app.key_of(Command::SelectAll), Some(Message::SelectAll)),
        item("Bypass Node", app.key_of(Command::Bypass), bypass.map(Message::ToggleBypass)),
        item(
            match app.ui.selection.len() {
                0 => "Create Graph from Selection".to_string(),
                n => format!("Create Graph from Selection ({n})"),
            },
            app.key_of(Command::GraphFromSelection),
            sel(Message::GraphFromSelection),
        ),
        item(if has_selection { "Frame Selection in a Comment" } else { "Add Comment" }, app.key_of(Command::AddComment), Some(Message::AddComment)),
        separator(),
        item("Settings…", app.key_of(Command::Settings), Some(Message::Settings(SettingsMessage::Open))),
    ]
}

fn view_menu(app: &App) -> Vec<Entry<'_>> {
    let p = &app.ui.preview;
    let check = |on: bool, label: &str| format!("{}  {label}", if on { "●" } else { "○" });
    vec![
        item(check(p.show_tiling, "Show Tiling in 2D View"), app.key_of(Command::ShowTiling), Some(Message::Preview(PreviewMessage::ToggleTiling))),
        item(check(app.ui.realtime, "Real-time Rendering"), "", Some(Message::ToggleRealtime)),
        separator(),
        item("Frame Selection / All", app.key_of(Command::Frame), Some(Message::Frame)),
        item("Reset Panel Layout", "", Some(Message::ResetLayout)),
    ]
}

fn help_menu(app: &App) -> Vec<Entry<'_>> {
    vec![
        item("Keyboard Shortcuts", app.key_of(Command::Help), Some(Message::Dialog(Some(Dialog::Shortcuts)))),
        item("Third-Party Notices…", "", Some(Message::Dialog(Some(Dialog::Notices(Default::default()))))),
        // (Where a log file could be made.)
        item("Reveal Log File", "", app.log.is_some().then_some(Message::RevealLog)),
        item(format!("About {}", brewed_texture_core::APP_NAME), "", Some(Message::Dialog(Some(Dialog::About)))),
    ]
}

/// The menu bar across the top of the window: the menus and the document's name. Where the
/// program draws the window's title bar (see [`OWN_TITLE_BAR`]) the bar is that too: the rest of
/// it moves the window (twice at once, it fills the screen), its buttons at the right make the
/// window small, fill the screen or give it back its size, close it.
pub fn menu_bar(app: &App) -> Element<'_, Message> {
    let logo = image(crate::brand::logo()).width(Length::Fixed(16.0)).height(Length::Fixed(16.0));
    let dirty = if app.doc.is_dirty() { "  •" } else { "" };
    // (Not while the Home screen is shown: nothing of it is edited there.)
    // (On the right, shortened in its middle where the bar is too short for it.)
    let document: Element<'_, Message> = match app.ui.home {
        Some(_) => space().width(Length::Fill).into(),
        None => {
            // (A package with no graph: the package alone.)
            let line = match app.doc.has_graph() {
                true => format!("{} — {}{dirty}", app.doc.title(), app.doc.entry().name),
                false => format!("{}{dirty}", app.doc.title()),
            };
            iced::widget::responsive(move |room| {
                let line = text(crate::view::shortened(&line, style::SMALL, style::ui_font(), room.width))
                    .size(style::SMALL)
                    .color(style::TEXT_DIM)
                    .wrapping(text::Wrapping::None);
                row![space().width(Length::Fill), line].into()
            })
            .height(Length::Shrink)
            .into()
        }
    };
    // The logo goes to the Home screen, as Adobe's does.
    let home = crate::app::home::HomeMessage::Show(crate::app::home::HomePage::Home);
    let logo = tip(button(logo).padding([4, 6]).style(style::flat_button).on_press(Message::Home(home)), "Home Screen", tooltip::Position::Bottom);
    let menus = row![
        logo,
        menu(app, MenuId::File, "File", file_menu(app)),
        menu(app, MenuId::Edit, "Edit", edit_menu(app)),
        menu(app, MenuId::View, "View", view_menu(app)),
        menu(app, MenuId::Help, "Help", help_menu(app)),
    ]
    .spacing(2)
    .align_y(Alignment::Center);
    let title = row![document].align_y(Alignment::Center).height(Length::Fill);
    if !OWN_TITLE_BAR {
        return container(row![menus, title].spacing(2).align_y(Alignment::Center).height(Length::Fixed(MENU_BAR_H)))
            .padding([0, 8])
            .width(Length::Fill)
            .style(style::window_bar)
            .into();
    }
    // A dialog open covers what is under the bar, not the bar: its menus are dimmed and take
    // nothing then (the dialog is answered first); the window still moves, and its buttons work.
    let dialog = app.ui.dialog.is_some();
    let menus: Element<'_, Message> = if dialog {
        let dim = container(space().width(Length::Fill).height(Length::Fill)).style(style::scrim);
        crate::widget::blind::blind(stack![menus, dim], true).into()
    } else {
        menus.into()
    };
    let drag = mouse_area(container(title).width(Length::Fill).height(Length::Fill).padding([0, 12]))
        .on_press(Message::Window(WindowAction::TitlePressed))
        .on_right_press(Message::Window(WindowAction::TitleMenu));
    container(row![menus, drag, window_buttons(app)].align_y(Alignment::Center).height(Length::Fixed(MENU_BAR_H)))
        .padding(Padding { left: 8.0, ..Padding::ZERO })
        .width(Length::Fill)
        .style(style::window_bar)
        .into()
}

/// The window's buttons, at the right of the menu bar: make it small, fill the screen or give it
/// back its size, close it (see [`OWN_TITLE_BAR`]).
fn window_buttons(app: &App) -> Element<'_, Message> {
    let button_of = |glyph: Icon, action: WindowAction, close: bool, says: &'static str| {
        tip(
            button(container(icon(glyph, 16.0, style::TEXT)).center(Length::Fill))
                .width(Length::Fixed(WINDOW_BUTTON_W))
                .height(Length::Fill)
                .padding(0)
                .style(style::window_button(close))
                .on_press(Message::Window(action)),
            says,
            tooltip::Position::Bottom,
        )
    };
    let (glyph, says) = if app.ui.maximized { (Icon::Restore, "Restore Down") } else { (Icon::Maximize, "Maximize") };
    row![
        button_of(Icon::Minimize, WindowAction::Minimize, false, "Minimize"),
        button_of(glyph, WindowAction::ToggleMaximize, false, says),
        button_of(Icon::Close, WindowAction::Close, true, "Close"),
    ]
    .height(Length::Fill)
    .into()
}

/// How thick the edges of the window are that size it, where the program draws its frame (see
/// [`OWN_TITLE_BAR`]), and how far along them its corners reach.
pub(crate) const WINDOW_EDGE: f32 = 4.0;
pub(crate) const WINDOW_CORNER: f32 = 14.0;

/// The edges and corners of the window, which size it where the program draws its frame (not
/// while it fills the screen): over everything, a few pixels deep, the cursor saying which way.
pub fn window_edges<'a>() -> Element<'a, Message> {
    window_edges_with(|direction| Message::Window(WindowAction::Resize(direction)))
}

/// The edges and corners of a window of the program's that size it, each pressed saying
/// `resize` which way.
pub fn window_edges_with<'a>(resize: fn(iced::window::Direction) -> Message) -> Element<'a, Message> {
    use iced::mouse::Interaction as I;
    use iced::window::Direction as D;
    let grip = |direction: D, interaction: I, width: Length, height: Length| -> Element<'a, Message> {
        mouse_area(space().width(width).height(height)).interaction(interaction).on_press(resize(direction)).into()
    };
    let edge = Length::Fixed(WINDOW_EDGE);
    let corner = Length::Fixed(WINDOW_CORNER);
    let along = Length::Fixed(WINDOW_CORNER - WINDOW_EDGE);
    // Each corner is an L: as far along the top or bottom as along the side.
    let side = |top: D, middle: D, bottom: D, top_i: I, bottom_i: I| {
        column![grip(top, top_i, edge, along), grip(middle, I::ResizingHorizontally, edge, Length::Fill), grip(bottom, bottom_i, edge, along),].width(edge)
    };
    column![
        row![
            grip(D::NorthWest, I::ResizingDiagonallyDown, corner, edge),
            grip(D::North, I::ResizingVertically, Length::Fill, edge),
            grip(D::NorthEast, I::ResizingDiagonallyUp, corner, edge),
        ],
        row![
            side(D::NorthWest, D::West, D::SouthWest, I::ResizingDiagonallyDown, I::ResizingDiagonallyUp),
            space().width(Length::Fill).height(Length::Fill),
            side(D::NorthEast, D::East, D::SouthEast, I::ResizingDiagonallyUp, I::ResizingDiagonallyDown),
        ]
        .height(Length::Fill),
        row![
            grip(D::SouthWest, I::ResizingDiagonallyUp, corner, edge),
            grip(D::South, I::ResizingVertically, Length::Fill, edge),
            grip(D::SouthEast, I::ResizingDiagonallyDown, corner, edge),
        ],
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Header of the graph pane, with the size of the textures.
pub fn graph_header(app: &App) -> Element<'_, Message> {
    let [w, h] = app.graph().resolution;
    let lock = app.ui.lock_square;
    let size_list = |value: u32, on_select: Box<dyn Fn(u32) -> Message>| {
        pick_list(RESOLUTIONS, Some(value), on_select).text_size(style::SMALL).padding([2, 6]).style(style::pick_list)
    };
    let tools = row![
        text("Texture size").size(style::SMALL).color(style::TEXT_DIM),
        size_list(w, Box::new(move |nw| Message::Edit(Edit::SetResolution([nw, if lock { nw } else { h }])))),
        text("×").size(style::SMALL).color(style::TEXT_DIM),
        size_list(h, Box::new(move |nh| Message::Edit(Edit::SetResolution([if lock { nh } else { w }, nh])))),
        tip(
            button(icon(Icon::Link, 14.0, if lock { style::ACCENT_TEXT } else { style::TEXT_DIM }))
                .padding(4)
                .style(style::toggle_button(lock))
                .on_press(Message::LockSquare),
            if lock { "Width and height are kept equal (click to size them separately)" } else { "Keep width and height equal" },
            tooltip::Position::Bottom,
        ),
        tip(
            pick_list(Precision::ALL, Some(app.graph().precision), |p| Message::Edit(Edit::SetPrecision(p)))
                .text_size(style::SMALL)
                .padding([2, 6])
                .style(style::pick_list),
            "What the graph keeps its images in; a node can keep its own in another (its Precision). \
             8 bits: 256 steps from 0 to 1, a quarter of the memory of floats. 16 bits float: steps of 1/2048 \
             near 1 and finer below, values beyond 0 and 1 kept too; what a game needs. 32 bits float: smooth \
             everywhere (a gentle slope of a height stays smooth in a normal map), twice the memory.",
            tooltip::Position::Bottom,
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    crate::view::pane_header("Graph", tools)
}

/// How long a change takes before the status bar says it is being computed.
pub const COMPUTING_AFTER: Duration = Duration::from_millis(250);
/// How far back the status bar's readings go.
pub const READING_SPAN: Duration = Duration::from_secs(1);

/// A duration in milliseconds, as precise as is worth reading.
fn duration(millis: f32) -> String {
    match millis {
        m if m < 10.0 => format!("{m:.1} ms"),
        m if m < 1000.0 => format!("{m:.0} ms"),
        m => format!("{:.1} s", m / 1000.0),
    }
}

fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb < 1000.0 { format!("{mb:.0} MB") } else { format!("{:.1} GB", mb / 1024.0) }
}

/// `n` of `thing`, as English counts them.
fn count(n: usize, thing: &str) -> String {
    if n == 1 { format!("1 {thing}") } else { format!("{n} {thing}s") }
}

/// A reading of the status bar, as long as it has to be and no longer.
fn reading<'a>(content: String, hint: String) -> Element<'a, Message> {
    tip(text(content).size(style::SMALL).color(style::TEXT_DIM).wrapping(text::Wrapping::None), hint, tooltip::Position::Top)
}

fn divider<'a>() -> Element<'a, Message> {
    container(space().width(1).height(12)).style(style::hairline(style::LINE)).into()
}

/// The status bar across the bottom of the window: the latest message (nothing when there is
/// none), then what is happening now, over the last second: the graph's nodes, how many of
/// them its last change computed again and how long its changes take; how much GPU memory
/// the images take; and the GPU. (How fast the 3D view draws, it says itself.)
pub fn status_bar(app: &App) -> Element<'_, Message> {
    // (Shortened in its middle beside the readings: the file a message is about is at its end.)
    let status: Element<'_, Message> = match &app.ui.status {
        Some(s) => crate::view::fitted(s.text.clone(), style::SMALL, style::ui_font(), Some(if s.error { style::ERROR } else { style::TEXT })),
        None => space().into(),
    };

    let nodes = count(app.graph().nodes.len(), "node");
    let busy = app.res.busy_since.map(|since| since.elapsed());
    let recent: Vec<(f32, usize)> = app.res.updates.iter().filter(|(at, ..)| at.elapsed() <= READING_SPAN).map(|(_, millis, n)| (*millis, *n)).collect();
    let graph = match busy {
        Some(for_) if app.res.slow || for_ >= COMPUTING_AFTER => {
            format!("{nodes} · computing… {}", duration(for_.as_secs_f32() * 1000.0))
        }
        _ => {
            let (recomputed, average) = match recent.last() {
                Some((_, recomputed)) => (*recomputed, recent.iter().map(|(millis, _)| millis).sum::<f32>() / recent.len() as f32),
                None => (0, 0.0),
            };
            format!("{nodes} · {recomputed} recomputed · {}", duration(average))
        }
    };
    let graph = reading(
        graph,
        "The nodes of the graph; how many of them the last change of the last second computed again \
         (only those it affects); and how long the changes of the last second took on average, from \
         the change until the GPU was done with the images, their thumbnails and the views."
            .into(),
    );

    // The engine is there once it started, or once it said anything.
    let engine = app.engine.is_some() || app.res.finished_revision > 0;
    let m = app.res.memory;
    let memory = reading(
        format!("VRAM {}", megabytes(m.total())),
        format!(
            "GPU memory the images take, at the texture size:\n\
             {} the 2D and 3D views show: {} with their mips (an output's in its channels and \
             at its bit depth)\n\
             {} kept for the next change (what the last one read)\n\
             {} scratch images, kept while changes keep coming (a slider dragged)\n\
             {} files and gradients\n\
             Half a second after the last change, only what the views show stays.",
            megabytes(m.shown),
            count(m.shown_images, "image"),
            megabytes(m.kept),
            megabytes(m.scratch),
            megabytes(m.files),
        ),
    );

    let gpu = if app.engine.is_none() {
        "Starting the GPU…".to_string()
    } else if app.adapter.is_empty() {
        "GPU".to_string()
    } else {
        app.adapter.clone()
    };
    // (It never should.)
    let gpu: Element<'_, Message> = match &app.res.stopped {
        Some(why) => tip(
            text("The engine stopped").size(style::SMALL).color(style::ERROR),
            format!("{why}\n\nNothing is computed any more: save the packages and start the program again."),
            tooltip::Position::Top,
        ),
        None => text(gpu).size(style::SMALL).color(style::TEXT_FAINT).into(),
    };
    let mut bar = row![container(status).clip(true).width(Length::Fill), graph].spacing(8).align_y(Alignment::Center).height(Length::Fixed(STATUS_BAR_H));
    if engine {
        bar = bar.push(divider()).push(memory);
    }
    bar = bar.push(divider()).push(gpu);
    container(bar).padding([0, 10]).width(Length::Fill).style(style::window_bar).into()
}
