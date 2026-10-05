//! Dialogs, answered from the keyboard as from the mouse.

use super::*;

/// Enter answers a dialog as its first button does: About and the shortcuts close, a new graph is
/// made, the question about unsaved changes saves them (and then does what was asked); a field
/// that takes the key keeps it, and a dialog with a search of its own waits for a click.
#[test]
fn enter_answers_a_dialog() {
    use crate::app::home::NewGraphMessage;
    let mut w = harness!(view::root, 1400.0, 900.0);
    let enter = |w: &mut Harness, typing: bool| {
        let key = Key::Named(keyboard::key::Named::Enter);
        let event = iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
            text: None,
            repeat: false,
        });
        let status = if typing { iced::event::Status::Captured } else { iced::event::Status::Ignored };
        let main = w.main_window();
        w.send(Message::Runtime(event, status, main));
    };
    for dialog in [Dialog::About, Dialog::Shortcuts] {
        w.send(Message::Dialog(Some(dialog)));
        enter(&mut w, true);
        assert!(w.app.ui.dialog.is_some(), "a field that took Enter keeps it");
        enter(&mut w, false);
        assert!(w.app.ui.dialog.is_none(), "Enter closes it");
    }
    w.send(Message::Dialog(Some(Dialog::Notices(Default::default()))));
    enter(&mut w, false);
    assert!(matches!(w.app.ui.dialog, Some(Dialog::Notices(_))), "the notices, with their search, wait for a click");
    w.send(Message::Dialog(None));
    // A new graph: made, and edited.
    let before = w.app.doc.entry().name.clone();
    w.send(Message::NewGraph(NewGraphMessage::Open(None)));
    enter(&mut w, false);
    assert!(w.app.ui.dialog.is_none() && w.app.doc.entry().name != before, "Enter makes the new graph (in place of the demo, untouched)");
    // Unsaved changes: saved, then what was asked.
    let dir = super::scratch("enter-saves");
    let file = dir.join("walls.brewtex");
    w.send(Message::SavePicked(Some(file.clone())));
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [0.0, 0.0] }));
    w.send(Message::Seal);
    let blurs = w.app.graph().nodes_of(NodeKind::BLUR).count();
    assert!(w.app.doc.is_dirty());
    w.send(Message::Dialog(Some(Dialog::Unsaved(PendingAction::New))));
    enter(&mut w, false);
    let saved = brewed_texture_core::project::load(&file).expect("the package, saved");
    assert_eq!(saved.package.graphs.iter().map(|g| g.graph.nodes_of(NodeKind::BLUR).count()).sum::<usize>(), blurs, "saved with the change");
    assert!(w.app.ui.dialog.is_none() && w.app.doc.path().is_none(), "then a new package, as was asked");
    let _ = std::fs::remove_dir_all(&dir);
}
