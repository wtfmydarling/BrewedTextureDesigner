//! The window's own title bar (where the program draws it: not on macOS, whose own it keeps).

use std::time::{Duration, Instant};

use iced::mouse::Interaction;
use iced::window::Direction;

use super::*;
use crate::app::home::DOUBLE_CLICK;
use crate::app::{OWN_TITLE_BAR, WindowAction, WindowRequest};

const W: f32 = 1400.0;
const H: f32 = 900.0;

/// What of `messages` the title bar asked for.
fn of_the_bar(messages: &[Message]) -> Vec<WindowAction> {
    messages.iter().filter_map(|m| if let Message::Window(a) = m { Some(*a) } else { None }).collect()
}

/// Which way an edge sizes the window.
fn compass(direction: Direction) -> &'static str {
    match direction {
        Direction::North => "N",
        Direction::South => "S",
        Direction::East => "E",
        Direction::West => "W",
        Direction::NorthEast => "NE",
        Direction::NorthWest => "NW",
        Direction::SouthEast => "SE",
        Direction::SouthWest => "SW",
    }
}

/// Which way pressing at `at` sizes the window, if it does (and then it asks nothing else).
fn sizes(w: &mut Harness, at: Point) -> Option<&'static str> {
    let asked = of_the_bar(&pressed(w, at));
    let ways: Vec<&str> = asked.iter().filter_map(|a| if let WindowAction::Resize(d) = a { Some(compass(*d)) } else { None }).collect();
    assert!(ways.is_empty() || asked.len() == 1, "pressed at {at:?}: {asked:?}");
    ways.first().copied()
}

/// The menu bar ends in the window's buttons — make it small, fill the screen, close it — the
/// one that closes it at the right edge, each as wide as the system's; on macOS, whose title bar
/// the window keeps, there are none.
#[test]
fn the_menu_bar_ends_in_the_windows_buttons() {
    let mut w = harness!(view::root, W, H);
    w.input(&[]);
    let button = |i: f32| Point::new(W - view::toolbar::WINDOW_BUTTON_W * (i + 0.5), 15.0);
    let close = of_the_bar(&clicked(&mut w, button(0.0)));
    if !OWN_TITLE_BAR {
        assert!(close.is_empty(), "the system's title bar has them");
        return;
    }
    assert!(matches!(close[..], [WindowAction::Close]), "{close:?}");
    let maximize = of_the_bar(&clicked(&mut w, button(1.0)));
    assert!(matches!(maximize[..], [WindowAction::ToggleMaximize]), "{maximize:?}");
    let minimize = of_the_bar(&clicked(&mut w, button(2.0)));
    assert!(matches!(minimize[..], [WindowAction::Minimize]), "{minimize:?}");
    // Left of them, the bar: pressed, it moves the window.
    let bar = of_the_bar(&pressed(&mut w, button(3.0)));
    assert!(matches!(bar[..], [WindowAction::TitlePressed]), "{bar:?}");
    // Filling the screen, the very corner closes it (Fitts's), and the button gives the window
    // back its size.
    w.send(Message::Window(WindowAction::Maximized(true)));
    w.input(&[]);
    let corner = of_the_bar(&clicked(&mut w, Point::new(W - 1.0, 0.0)));
    assert!(matches!(corner[..], [WindowAction::Close]), "{corner:?}");
    let restore = of_the_bar(&clicked(&mut w, button(1.0)));
    assert!(matches!(restore[..], [WindowAction::ToggleMaximize]), "{restore:?}");
    // The bar is on the Home screen too.
    w.send(Message::Home(crate::app::home::HomeMessage::Show(crate::app::home::HomePage::Home)));
    w.input(&[]);
    assert!(w.app.ui.home.is_some());
    let close = of_the_bar(&clicked(&mut w, button(0.0)));
    assert!(matches!(close[..], [WindowAction::Close]), "{close:?}");
}

/// The bar between the menus and the window's buttons moves the window — over the document's
/// name too — and pressed again at once fills the screen; right-clicked, it shows the system's
/// menu of the window. The menus are not it: a click on one opens it.
#[test]
fn the_bar_moves_the_window_and_twice_fills_the_screen() {
    let mut w = harness!(view::root, W, H);
    w.input(&[]);
    if !OWN_TITLE_BAR {
        assert!(of_the_bar(&pressed(&mut w, Point::new(W / 2.0, 15.0))).is_empty());
        return;
    }
    let name = format!("{} — {}", w.app.doc.title(), w.app.doc.entry().name);
    let title = *texts(&mut w, &name).first().expect("the document's name in the bar");
    for at in [Point::new(W / 2.0, 15.0), title.center(), Point::new(title.x - 20.0, 8.0)] {
        let asked = of_the_bar(&pressed(&mut w, at));
        assert!(matches!(asked[..], [WindowAction::TitlePressed]), "pressed at {at:?}: {asked:?}");
    }
    let menu = of_the_bar(&right_clicked(&mut w, Point::new(W / 2.0, 15.0)));
    assert!(matches!(menu[..], [WindowAction::TitleMenu]), "{menu:?}");
    // A menu's title opens the menu, and does not move the window.
    let file = *texts(&mut w, "File").first().expect("the File menu");
    let asked = clicked(&mut w, file.center());
    assert!(of_the_bar(&asked).is_empty(), "{asked:?}");
    assert!(asked.iter().any(|m| matches!(m, Message::Menu(Some(_)))), "the menu opens: {asked:?}");

    // What a press asks the window: once, to move; again at once, to fill the screen; a third
    // time, to move again.
    w.app.ui.title_pressed = None;
    let request = |w: &mut Harness, action| w.app.window_request(action);
    assert!(matches!(request(&mut w, WindowAction::TitlePressed), Some(WindowRequest::Drag)));
    assert!(matches!(request(&mut w, WindowAction::TitlePressed), Some(WindowRequest::ToggleMaximize)));
    assert!(matches!(request(&mut w, WindowAction::TitlePressed), Some(WindowRequest::Drag)));
    // What the system says of the window between the two presses keeps the first.
    assert!(request(&mut w, WindowAction::Maximized(true)).is_none());
    assert!(matches!(request(&mut w, WindowAction::TitlePressed), Some(WindowRequest::ToggleMaximize)));
    assert!(w.app.ui.maximized, "it said the window fills the screen");
    // Anything else between them, or a press long after, is no double click.
    assert!(matches!(request(&mut w, WindowAction::TitlePressed), Some(WindowRequest::Drag)));
    assert!(matches!(request(&mut w, WindowAction::Minimize), Some(WindowRequest::Minimize)));
    assert!(matches!(request(&mut w, WindowAction::TitlePressed), Some(WindowRequest::Drag)));
    w.app.ui.title_pressed = Some(Instant::now() - DOUBLE_CLICK - Duration::from_millis(20));
    assert!(matches!(request(&mut w, WindowAction::TitlePressed), Some(WindowRequest::Drag)), "long after");
    assert!(matches!(request(&mut w, WindowAction::TitleMenu), Some(WindowRequest::SystemMenu)));
    assert!(w.app.ui.title_pressed.is_none(), "a right click is no first press of a double click");
    // Pressed twice on the bar itself, the second press is the double click.
    w.app.ui.title_pressed = None;
    w.input(&[]);
    w.hover(Point::new(W / 2.0, 15.0));
    w.mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    assert!(w.app.ui.title_pressed.is_some(), "the first press");
    w.mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    assert!(w.app.ui.title_pressed.is_none(), "the second, at once: the double click");
}

/// The edges of the window size it, its corners both ways — each corner as far along the top or
/// bottom as along the side — the cursor saying which way; a few pixels in, the window is as it
/// was (the bar moves it). Filling the screen, nothing at the edges sizes it.
#[test]
fn the_edges_of_the_window_size_it() {
    let mut w = harness!(view::root, W, H);
    w.input(&[]);
    if !OWN_TITLE_BAR {
        assert_eq!(sizes(&mut w, Point::new(W / 2.0, H - 1.0)), None, "the system's frame does");
        return;
    }
    let corner = view::toolbar::WINDOW_CORNER - 2.0;
    let edges = [
        (Point::new(W / 2.0, 1.0), "N", Interaction::ResizingVertically),
        (Point::new(W / 2.0, H - 1.0), "S", Interaction::ResizingVertically),
        (Point::new(1.0, H / 2.0), "W", Interaction::ResizingHorizontally),
        (Point::new(W - 1.0, H / 2.0), "E", Interaction::ResizingHorizontally),
        (Point::new(1.0, 1.0), "NW", Interaction::ResizingDiagonallyDown),
        (Point::new(corner, 1.0), "NW", Interaction::ResizingDiagonallyDown),
        (Point::new(1.0, corner), "NW", Interaction::ResizingDiagonallyDown),
        (Point::new(W - 1.0, 1.0), "NE", Interaction::ResizingDiagonallyUp),
        (Point::new(W - corner, 1.0), "NE", Interaction::ResizingDiagonallyUp),
        (Point::new(W - 1.0, corner), "NE", Interaction::ResizingDiagonallyUp),
        (Point::new(1.0, H - 1.0), "SW", Interaction::ResizingDiagonallyUp),
        (Point::new(1.0, H - corner), "SW", Interaction::ResizingDiagonallyUp),
        (Point::new(W - 1.0, H - 1.0), "SE", Interaction::ResizingDiagonallyDown),
        (Point::new(W - corner, H - 1.0), "SE", Interaction::ResizingDiagonallyDown),
        (Point::new(W - 1.0, H - corner), "SE", Interaction::ResizingDiagonallyDown),
    ];
    for (at, way, cursor) in edges {
        assert_eq!(sizes(&mut w, at), Some(way), "pressed at {at:?}");
        assert_eq!(w.interaction(at), cursor, "the cursor at {at:?}");
    }
    // A few pixels in: the window as it was.
    let inside = view::toolbar::WINDOW_EDGE + 2.0;
    for at in [Point::new(W / 2.0, inside), Point::new(inside, H / 2.0), Point::new(W - inside, H / 2.0), Point::new(W / 2.0, H - inside)] {
        assert_eq!(sizes(&mut w, at), None, "pressed at {at:?}");
    }
    // Over a dialog too.
    w.send(Message::Dialog(Some(Dialog::Shortcuts)));
    w.input(&[]);
    assert_eq!(sizes(&mut w, Point::new(W / 2.0, H - 1.0)), Some("S"));
    w.send(Message::Dialog(None));
    // Filling the screen: no edges.
    w.send(Message::Window(WindowAction::Maximized(true)));
    w.input(&[]);
    for (at, _, _) in edges {
        assert_eq!(sizes(&mut w, at), None, "pressed at {at:?}, the window filling the screen");
    }
    assert_ne!(w.interaction(Point::new(W / 2.0, H - 1.0)), Interaction::ResizingVertically);
    // Its size given back: the edges again.
    w.send(Message::Window(WindowAction::Maximized(false)));
    w.input(&[]);
    assert_eq!(sizes(&mut w, Point::new(W - 1.0, H - 1.0)), Some("SE"));
}

/// A dialog covers what is under the bar, not the bar: the window still moves and its buttons
/// work, a dialog that a click beside it closes stays open when the bar is pressed, and the
/// menus take nothing until the dialog is answered. Closing the window asks about unsaved
/// changes, as quitting does; asked, the bar still works.
#[test]
fn the_bar_works_over_a_dialog_and_closing_asks_first() {
    let mut w = harness!(view::root, W, H);
    w.input(&[]);
    if !OWN_TITLE_BAR {
        return;
    }
    let button = |i: f32| Point::new(W - view::toolbar::WINDOW_BUTTON_W * (i + 0.5), 15.0);
    w.send(Message::Dialog(Some(Dialog::Shortcuts)));
    w.input(&[]);
    let bar = of_the_bar(&pressed(&mut w, Point::new(W / 2.0, 15.0)));
    assert!(matches!(bar[..], [WindowAction::TitlePressed]), "{bar:?}");
    // (Applied: the press moves the window, and the dialog stays.)
    click(&mut w, Point::new(W / 2.0, 15.0));
    assert!(matches!(w.app.ui.dialog, Some(Dialog::Shortcuts)), "pressing the bar is no click beside the dialog");
    let minimize = of_the_bar(&clicked(&mut w, button(2.0)));
    assert!(matches!(minimize[..], [WindowAction::Minimize]), "{minimize:?}");
    // The menus take nothing.
    let file = *texts(&mut w, "File").first().expect("the File menu");
    click(&mut w, file.center());
    assert_eq!(w.app.ui.menu, None, "no menu opens over a dialog");
    assert!(matches!(w.app.ui.dialog, Some(Dialog::Shortcuts)));
    // A click beside the dialog, under the bar, still closes it.
    click(&mut w, Point::new(30.0, H - 60.0));
    assert!(w.app.ui.dialog.is_none(), "a click beside it closes it");

    // A change not saved: closing the window asks first.
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [1500.0, -400.0] }));
    w.send(Message::Seal);
    w.input(&[]);
    assert!(w.app.doc.is_dirty());
    click(&mut w, button(0.0));
    assert!(matches!(w.app.ui.dialog, Some(Dialog::Unsaved(PendingAction::Quit))), "{:?}", w.app.ui.dialog.as_ref().map(std::mem::discriminant));
    // Asked, the window still moves and is made small.
    let bar = of_the_bar(&pressed(&mut w, Point::new(W / 2.0, 15.0)));
    assert!(matches!(bar[..], [WindowAction::TitlePressed]), "{bar:?}");
    let minimize = of_the_bar(&clicked(&mut w, button(2.0)));
    assert!(matches!(minimize[..], [WindowAction::Minimize]), "{minimize:?}");
    // Cancelled: nothing closes, the change is still there; asked again, asked again.
    let cancel = *texts(&mut w, "Cancel").first().expect("Cancel");
    click(&mut w, cancel.center());
    assert!(w.app.ui.dialog.is_none() && w.app.doc.is_dirty());
    click(&mut w, button(0.0));
    assert!(matches!(w.app.ui.dialog, Some(Dialog::Unsaved(PendingAction::Quit))), "asked again");
    w.send(Message::Dialog(None));
}

/// An open menu closes when the bar is pressed, and the window does not move with that press
/// (as a click beside any menu only closes it).
#[test]
fn pressing_the_bar_closes_an_open_menu_first() {
    let mut w = harness!(view::root, W, H);
    w.input(&[]);
    let file = *texts(&mut w, "File").first().expect("the File menu");
    click(&mut w, file.center());
    assert!(w.app.ui.menu.is_some(), "the menu is open");
    let asked = pressed(&mut w, Point::new(W / 2.0, 15.0));
    assert!(of_the_bar(&asked).is_empty(), "{asked:?}");
    assert!(asked.iter().any(|m| matches!(m, Message::Menu(None))), "{asked:?}");
}

/// Made small to the taskbar, the window says it is of no size (as Windows says it): the size it
/// had stays, the one kept for when the program starts again, and so do the columns' widths.
#[test]
fn a_window_made_small_to_the_taskbar_keeps_its_size() {
    let mut w = harness!(view::root, W, H);
    let id = w.main_window();
    let size = Size::new(1500.0, 900.0);
    w.window_event(id, iced::Event::Window(window::Event::Resized(size)));
    assert_eq!(w.app.ui.window_size, size);
    let splits = crate::app::splits_of(&w.app.ui.panes);
    w.window_event(id, iced::Event::Window(window::Event::Resized(Size::ZERO)));
    assert_eq!(w.app.ui.window_size, size, "the size it had");
    assert_eq!(crate::app::splits_of(&w.app.ui.panes), splits, "the columns as they were");
    // Back: as it was.
    w.window_event(id, iced::Event::Window(window::Event::Resized(size)));
    assert_eq!(crate::app::splits_of(&w.app.ui.panes), splits);
}

/// The smallest the window is let be is what its screen has room for: 1000 by 640 points on a
/// screen with room for that, less on one without — 1920 by 1080 at 200 %, 1366 by 768 at
/// 125 %, a screen whose scale was raised under the window (held to more, the window was larger
/// than its screen for good: found on Wayland, a display taken from 100 % to 200 %) — and the
/// least again on a larger screen. A screen that says nonsense of itself changes nothing much;
/// and the window is drawn, and nothing in it breaks, as small as it is let be.
#[test]
fn the_window_is_never_held_larger_than_its_screen() {
    use crate::app::{MIN_WINDOW, smallest_window};
    assert_eq!(smallest_window(None), MIN_WINDOW);
    for roomy in [Size::new(1280.0, 720.0), Size::new(1707.0, 960.0), Size::new(1920.0, 1080.0), Size::new(3840.0, 2160.0)] {
        assert_eq!(smallest_window(Some(roomy)), MIN_WINDOW, "{roomy:?}");
    }
    let tight = [Size::new(960.0, 540.0), Size::new(1092.8, 614.4), Size::new(1024.0, 600.0), Size::new(640.0, 1136.0)];
    for screen in tight {
        let least = smallest_window(Some(screen));
        // (Less its taskbar, its top bar or its dock: 48 points of it at the most, here.)
        assert!(least.width <= screen.width && least.height <= screen.height - 48.0, "{least:?} on {screen:?}");
        assert!(least.width <= MIN_WINDOW.width && least.height <= MIN_WINDOW.height && least != MIN_WINDOW, "{least:?} on {screen:?}");
    }
    assert_eq!(smallest_window(Some(tight[0])), Size::new(888.0, 468.0));
    for odd in [Size::new(0.0, 0.0), Size::new(-5.0, f32::NAN), Size::new(f32::INFINITY, 10.0), Size::new(1.0, 1.0)] {
        let least = smallest_window(Some(odd));
        assert!((400.0..=1000.0).contains(&least.width) && (256.0..=640.0).contains(&least.height), "{least:?} of {odd:?}");
    }

    // Told of its screen, the program holds the window to what it has room for; of a larger
    // one, to the least it is let be anywhere.
    let mut w = harness!(view::root, 1680.0, 1000.0);
    assert_eq!(w.app.window_min, MIN_WINDOW);
    w.send(Message::Screen(Some(tight[0]), false));
    assert_eq!(w.app.window_min, Size::new(888.0, 468.0));
    w.send(Message::Screen(Some(Size::new(2560.0, 1440.0)), false));
    assert_eq!(w.app.window_min, MIN_WINDOW);
    w.send(Message::Screen(Some(tight[1]), false));
    w.send(Message::Screen(None, false));
    assert_eq!(w.app.window_min, MIN_WINDOW, "a screen not known holds it to no less");
    // A window that filled its screen as the screen's scale was raised under it, and is left
    // larger than the screen (1680 by 1000 points on one of 960 by 540), is made to fill it
    // again: two frames later, once the system knows how small it may be. One that fits its
    // screen is left alone, and so is one that did not fill its screen.
    assert_eq!(w.app.fill_after, None);
    w.send(Message::Screen(Some(Size::new(1920.0, 1080.0)), true));
    assert_eq!(w.app.fill_after, None, "it fits its screen");
    w.send(Message::Screen(Some(tight[0]), false));
    assert_eq!(w.app.fill_after, None, "it did not fill its screen");
    w.send(Message::Screen(Some(Size::new(2560.0, 1440.0)), false));
    w.send(Message::Screen(Some(tight[0]), true));
    assert_eq!((w.app.window_min, w.app.fill_after), (Size::new(888.0, 468.0), Some(2)));
    w.send(Message::FillFrame);
    assert_eq!(w.app.fill_after, Some(1));
    w.send(Message::FillFrame);
    assert_eq!(w.app.fill_after, None);
    w.send(Message::FillFrame);
    assert_eq!(w.app.fill_after, None, "asked once");
    w.send(Message::Screen(Some(Size::new(2560.0, 1440.0)), false));
    // As small as that, it is drawn and nothing breaks.
    for screen in tight {
        let least = smallest_window(Some(screen));
        let id = w.main_window();
        w.look_at(Box::new(view::root), least);
        w.window_event(id, iced::Event::Window(window::Event::Resized(least)));
        draw_frame(&mut w, Instant::now());
        assert_eq!(w.app.ui.window_size, least);
        assert!(crate::ui_tests::broken(&w.app).is_none(), "{least:?}: {:?}", crate::ui_tests::broken(&w.app));
    }
}
