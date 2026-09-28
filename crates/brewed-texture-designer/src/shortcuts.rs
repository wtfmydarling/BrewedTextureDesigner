//! Keyboard shortcuts: the commands the keyboard gives, the keys each is given by default, and
//! the keys people give them instead (Settings → Shortcuts, kept in `settings.toml`). A key is
//! the same on every keyboard layout: a letter is the Latin letter it types (the key a Cyrillic
//! or Greek layout types "я" on is Z's), any other key is the key where it is (the comma's
//! key, whatever it types). Ctrl is ⌘ on macOS. Escape always closes what is open, and is no
//! one's to take.

use std::collections::BTreeMap;
use std::fmt;

use iced::keyboard::{self, Key, key::Code, key::Named, key::Physical};

/// Where a command is given from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Anywhere, even while a name or a value is being typed.
    Anywhere,
    /// The window, when no text is being typed.
    Window,
    /// With the pointer over the graph.
    Graph,
    /// With the pointer over the 2D view.
    View2d,
}

impl Place {
    /// Could a key given to both be meant for either? (Only the graph and the 2D view are apart.)
    fn meets(self, other: Place) -> bool {
        !matches!((self, other), (Place::Graph, Place::View2d) | (Place::View2d, Place::Graph))
    }
}

macro_rules! commands {
    ($($command:ident: $id:literal, $label:literal, $place:ident, [$($default:literal),*];)*) => {
        /// What the keyboard can give.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Command {
            $($command,)*
        }

        impl Command {
            pub const ALL: &'static [Command] = &[$(Command::$command,)*];

            /// What `settings.toml` calls it.
            pub fn id(self) -> &'static str {
                match self {
                    $(Command::$command => $id,)*
                }
            }

            pub fn label(self) -> &'static str {
                match self {
                    $(Command::$command => $label,)*
                }
            }

            pub fn place(self) -> Place {
                match self {
                    $(Command::$command => Place::$place,)*
                }
            }

            /// Its keys by default.
            pub fn defaults(self) -> &'static [&'static str] {
                match self {
                    $(Command::$command => &[$($default),*],)*
                }
            }
        }
    };
}

commands! {
    New: "new", "New Package", Window, ["Ctrl+N"];
    Open: "open", "Open Package…", Anywhere, ["Ctrl+O"];
    Save: "save", "Save", Anywhere, ["Ctrl+S"];
    SaveAs: "save_as", "Save As…", Anywhere, ["Ctrl+Shift+S"];
    Export: "export", "Export Outputs…", Anywhere, ["Ctrl+E"];
    Settings: "settings", "Settings…", Anywhere, ["Ctrl+,"];
    Undo: "undo", "Undo", Window, ["Ctrl+Z"];
    Redo: "redo", "Redo", Window, ["Ctrl+Y", "Ctrl+Shift+Z"];
    Copy: "copy", "Copy", Window, ["Ctrl+C"];
    Cut: "cut", "Cut", Window, ["Ctrl+X"];
    Paste: "paste", "Paste", Window, ["Ctrl+V"];
    Duplicate: "duplicate", "Duplicate", Window, ["Ctrl+D"];
    Delete: "delete", "Delete", Window, ["Delete", "Backspace"];
    SelectAll: "select_all", "Select All", Window, ["Ctrl+A"];
    Bypass: "bypass", "Bypass Node", Window, ["Ctrl+B"];
    GraphFromSelection: "graph_from_selection", "Create Graph from Selection", Window, ["Ctrl+G"];
    Frame: "frame", "Frame Selection / All", Window, ["F"];
    AddComment: "add_comment", "Add Comment", Graph, ["C"];
    AddNode: "add_node", "Search and Add a Node", Graph, ["Space"];
    ShowTiling: "show_tiling", "Show Tiling in the 2D View", View2d, ["Space"];
    Help: "help", "Keyboard Shortcuts", Window, ["F1"];
}

/// A key and the modifiers held with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: KeyName,
}

/// A key, as a shortcut names it (see the module).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyName {
    /// A Latin letter or a digit (small), or the key of a sign (`,` `.` `/` `;` `'` `[` `]`
    /// `-` `=` `` ` `` `\`).
    Char(char),
    F(u8),
    Space,
    Enter,
    Tab,
    Delete,
    Backspace,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Left,
    Right,
    Up,
    Down,
}

/// The named keys, as `settings.toml` writes them.
const NAMED: &[(&str, KeyName)] = &[
    ("Space", KeyName::Space),
    ("Enter", KeyName::Enter),
    ("Tab", KeyName::Tab),
    ("Delete", KeyName::Delete),
    ("Backspace", KeyName::Backspace),
    ("Insert", KeyName::Insert),
    ("Home", KeyName::Home),
    ("End", KeyName::End),
    ("PageUp", KeyName::PageUp),
    ("PageDown", KeyName::PageDown),
    ("Left", KeyName::Left),
    ("Right", KeyName::Right),
    ("Up", KeyName::Up),
    ("Down", KeyName::Down),
];

/// The keys of signs, where they are (US), and the sign each has there.
const SIGNS: &[(Code, char)] = &[
    (Code::Comma, ','),
    (Code::Period, '.'),
    (Code::Slash, '/'),
    (Code::Semicolon, ';'),
    (Code::Quote, '\''),
    (Code::BracketLeft, '['),
    (Code::BracketRight, ']'),
    (Code::Minus, '-'),
    (Code::Equal, '='),
    (Code::Backquote, '`'),
    (Code::Backslash, '\\'),
];

impl KeyName {
    fn of_char(c: char) -> Option<KeyName> {
        let c = c.to_ascii_lowercase();
        (c.is_ascii_lowercase() || c.is_ascii_digit() || SIGNS.iter().any(|(_, s)| *s == c)).then_some(KeyName::Char(c))
    }
}

impl Chord {
    /// The chord a key press is (see the module); none for a key no shortcut names (a lone
    /// modifier, Escape…).
    pub fn of(key: &Key, physical: Physical, modifiers: keyboard::Modifiers) -> Option<Chord> {
        let name = match key {
            Key::Named(named) => match named {
                Named::Space => Some(KeyName::Space),
                Named::Enter => Some(KeyName::Enter),
                Named::Tab => Some(KeyName::Tab),
                Named::Delete => Some(KeyName::Delete),
                Named::Backspace => Some(KeyName::Backspace),
                Named::Insert => Some(KeyName::Insert),
                Named::Home => Some(KeyName::Home),
                Named::End => Some(KeyName::End),
                Named::PageUp => Some(KeyName::PageUp),
                Named::PageDown => Some(KeyName::PageDown),
                Named::ArrowLeft => Some(KeyName::Left),
                Named::ArrowRight => Some(KeyName::Right),
                Named::ArrowUp => Some(KeyName::Up),
                Named::ArrowDown => Some(KeyName::Down),
                Named::F1 => Some(KeyName::F(1)),
                Named::F2 => Some(KeyName::F(2)),
                Named::F3 => Some(KeyName::F(3)),
                Named::F4 => Some(KeyName::F(4)),
                Named::F5 => Some(KeyName::F(5)),
                Named::F6 => Some(KeyName::F(6)),
                Named::F7 => Some(KeyName::F(7)),
                Named::F8 => Some(KeyName::F(8)),
                Named::F9 => Some(KeyName::F(9)),
                Named::F10 => Some(KeyName::F(10)),
                Named::F11 => Some(KeyName::F(11)),
                Named::F12 => Some(KeyName::F(12)),
                _ => None,
            },
            Key::Character(typed) => {
                let code = match physical {
                    Physical::Code(code) => Some(code),
                    Physical::Unidentified(_) => None,
                };
                let at = |keys: &[(Code, char)]| code.and_then(|code| keys.iter().find(|(c, _)| *c == code)).map(|(_, k)| *k);
                let mut chars = typed.chars();
                let typed = chars.next().filter(|_| chars.next().is_none()).map(|c| c.to_ascii_lowercase());
                let c = match typed {
                    // A Latin letter is the letter typed (AZERTY's A is A); a digit, the key of
                    // a digit (AZERTY types & on 1's), or a digit typed (on the keypad); a sign,
                    // the sign typed (AZERTY's comma is where M is on others); another letter of
                    // a Latin layout (ß, é), its key's; what Shift makes of a sign (<), its key's.
                    Some(t) if t < '\u{370}' => Some(t)
                        .filter(char::is_ascii_lowercase)
                        .or_else(|| at(DIGITS))
                        .or_else(|| Some(t).filter(char::is_ascii_digit))
                        .or_else(|| Some(t).filter(|t| SIGNS.iter().any(|(_, s)| s == t)))
                        .or_else(|| at(LETTERS))
                        .or_else(|| at(SIGNS)),
                    // Another script (Cyrillic, Greek…): the key's, as a US keyboard has it.
                    _ => at(LETTERS).or_else(|| at(DIGITS)).or_else(|| at(SIGNS)),
                };
                c.and_then(KeyName::of_char)
            }
            _ => None,
        }?;
        Some(Chord { ctrl: modifiers.command(), alt: modifiers.alt(), shift: modifiers.shift(), key: name })
    }
}

/// The keys of letters and digits, and what they type on a US keyboard.
const LETTERS: &[(Code, char)] = &[
    (Code::KeyA, 'a'),
    (Code::KeyB, 'b'),
    (Code::KeyC, 'c'),
    (Code::KeyD, 'd'),
    (Code::KeyE, 'e'),
    (Code::KeyF, 'f'),
    (Code::KeyG, 'g'),
    (Code::KeyH, 'h'),
    (Code::KeyI, 'i'),
    (Code::KeyJ, 'j'),
    (Code::KeyK, 'k'),
    (Code::KeyL, 'l'),
    (Code::KeyM, 'm'),
    (Code::KeyN, 'n'),
    (Code::KeyO, 'o'),
    (Code::KeyP, 'p'),
    (Code::KeyQ, 'q'),
    (Code::KeyR, 'r'),
    (Code::KeyS, 's'),
    (Code::KeyT, 't'),
    (Code::KeyU, 'u'),
    (Code::KeyV, 'v'),
    (Code::KeyW, 'w'),
    (Code::KeyX, 'x'),
    (Code::KeyY, 'y'),
    (Code::KeyZ, 'z'),
];
const DIGITS: &[(Code, char)] = &[
    (Code::Digit0, '0'),
    (Code::Digit1, '1'),
    (Code::Digit2, '2'),
    (Code::Digit3, '3'),
    (Code::Digit4, '4'),
    (Code::Digit5, '5'),
    (Code::Digit6, '6'),
    (Code::Digit7, '7'),
    (Code::Digit8, '8'),
    (Code::Digit9, '9'),
];

impl fmt::Display for Chord {
    /// As `settings.toml` writes it: "Ctrl+Shift+Z", "Delete", "F1", "Ctrl+,".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        match self.key {
            KeyName::Char(c) => write!(f, "{}", c.to_ascii_uppercase()),
            KeyName::F(n) => write!(f, "F{n}"),
            other => f.write_str(NAMED.iter().find(|(_, k)| *k == other).map_or("?", |(n, _)| n)),
        }
    }
}

impl Chord {
    /// As people read it on this system: ⌘ for Ctrl on macOS.
    pub fn shown(&self) -> String {
        let text = self.to_string();
        if cfg!(target_os = "macos") { text.replacen("Ctrl+", "⌘", 1) } else { text }
    }

    /// Read what [`fmt::Display`] writes (the modifiers in any order, any case); none for what
    /// names no chord.
    pub fn parse(text: &str) -> Option<Chord> {
        let text = text.trim();
        // The key is what follows the last "+", unless the key is "+" itself ("Ctrl++" is not
        // taken: "=" is the key).
        let (mods, key) = match text.rsplit_once('+') {
            Some((mods, key)) if !key.is_empty() => (mods, key),
            _ => ("", text),
        };
        let mut chord = Chord { ctrl: false, alt: false, shift: false, key: KeyName::Space };
        for m in mods.split('+').filter(|m| !m.is_empty()) {
            match m.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" | "command" => chord.ctrl = true,
                "alt" | "option" => chord.alt = true,
                "shift" => chord.shift = true,
                _ => return None,
            }
        }
        let key = key.trim();
        chord.key = if let Some(n) = key.strip_prefix(['F', 'f']).and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=12).contains(n)) {
            KeyName::F(n)
        } else if let Some((_, named)) = NAMED.iter().find(|(n, _)| n.eq_ignore_ascii_case(key)) {
            *named
        } else {
            let mut chars = key.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            KeyName::of_char(c)?
        };
        Some(chord)
    }
}

/// The keys people gave commands instead of those they have by default: what `settings.toml`
/// says under `[shortcuts]`. A command has at most [`Shortcuts::SLOTS`] of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shortcuts {
    changed: BTreeMap<Command, Vec<Chord>>,
}

impl Shortcuts {
    /// How many chords a command has at most.
    pub const SLOTS: usize = 2;

    /// The chords of `command` by default.
    pub fn defaults(command: Command) -> Vec<Chord> {
        command.defaults().iter().filter_map(|t| Chord::parse(t)).collect()
    }

    /// The chords `command` is given by.
    pub fn of(&self, command: Command) -> Vec<Chord> {
        self.changed.get(&command).cloned().unwrap_or_else(|| Shortcuts::defaults(command))
    }

    /// Give `command` `chords` (at most [`Shortcuts::SLOTS`], each once). A chord another
    /// command has where both are given from goes from that one: what it had to give it up, if
    /// anything.
    pub fn set(&mut self, command: Command, chords: Vec<Chord>) -> Vec<(Command, Chord)> {
        let mut unique = Vec::new();
        for chord in chords {
            if !unique.contains(&chord) && unique.len() < Shortcuts::SLOTS {
                unique.push(chord);
            }
        }
        let mut taken = Vec::new();
        for other in Command::ALL.iter().copied().filter(|c| *c != command && c.place().meets(command.place())) {
            let theirs = self.of(other);
            if theirs.iter().any(|c| unique.contains(c)) {
                let kept: Vec<Chord> = theirs.iter().copied().filter(|c| !unique.contains(c)).collect();
                taken.extend(theirs.iter().copied().filter(|c| unique.contains(c)).map(|c| (other, c)));
                self.put(other, kept);
            }
        }
        self.put(command, unique);
        taken
    }

    fn put(&mut self, command: Command, chords: Vec<Chord>) {
        if chords == Shortcuts::defaults(command) {
            self.changed.remove(&command);
        } else {
            self.changed.insert(command, chords);
        }
    }

    /// The command `chord` gives where the pointer is (`over_graph`, `over_2d`) and whether a
    /// text is being typed: of those given from there, the first that has it; if none does, the
    /// first that has it without Shift, or Alt, or both held (as the keys always were: Shift+Del
    /// deletes, Ctrl+Shift+C copies). While typing, only a chord with Ctrl or a function key is a
    /// command (the others type).
    pub fn command(&self, chord: Chord, typing: bool, over_graph: bool, over_2d: bool) -> Option<Command> {
        let types = !chord.ctrl && !matches!(chord.key, KeyName::F(_));
        let given = |chord: Chord| {
            Command::ALL.iter().copied().find(|c| {
                let here = match c.place() {
                    Place::Anywhere => !typing || !types,
                    Place::Window => !typing,
                    Place::Graph => !typing && over_graph,
                    Place::View2d => !typing && over_2d,
                };
                here && self.of(*c).contains(&chord)
            })
        };
        let without = [chord, Chord { shift: false, ..chord }, Chord { alt: false, ..chord }, Chord { shift: false, alt: false, ..chord }];
        without.into_iter().find_map(given)
    }

    /// The shortcuts as `settings.toml` says them, and what it says that cannot be taken. A
    /// command it gives no key it can read keeps its own; a key two commands would have where
    /// both are given from is the first's: of those the file names, in its order, then of the
    /// others.
    pub fn read(table: Option<&dyn toml_edit::TableLike>) -> (Shortcuts, Vec<String>) {
        let mut shortcuts = Shortcuts::default();
        let mut problems = Vec::new();
        let mut named = Vec::new();
        for (id, item) in table.into_iter().flat_map(|t| t.iter()) {
            // (A command this version does not have is left alone.)
            let Some(command) = Command::ALL.iter().copied().find(|c| c.id() == id) else { continue };
            let values: Vec<&toml_edit::Value> = match item.as_value() {
                Some(toml_edit::Value::Array(a)) => a.iter().collect(),
                Some(one) => vec![one],
                None => {
                    problems.push(format!("shortcuts.{id} is not a list of keys"));
                    continue;
                }
            };
            let mut chords = Vec::new();
            for value in &values {
                match value.as_str().and_then(Chord::parse) {
                    Some(chord) if !chords.contains(&chord) && chords.len() < Shortcuts::SLOTS => chords.push(chord),
                    Some(chord) => problems.push(format!("shortcuts.{id}: {chord} is one key too many")),
                    None => problems.push(format!("shortcuts.{id}: {} is no key this version reads", value.to_string().trim())),
                }
            }
            if chords.is_empty() && !values.is_empty() {
                continue;
            }
            shortcuts.put(command, chords);
            named.push(command);
        }
        let others = Command::ALL.iter().copied().filter(|c| !named.contains(c));
        let mut given: Vec<(Command, Chord)> = Vec::new();
        for command in named.clone().into_iter().chain(others) {
            let chords = shortcuts.of(command);
            let (kept, lost): (Vec<Chord>, Vec<Chord>) = chords
                .into_iter()
                .partition(|chord| !given.iter().any(|(other, c)| c == chord && other.place().meets(command.place())));
            for chord in &lost {
                let first = given.iter().find(|(other, c)| c == chord && other.place().meets(command.place())).map(|(o, _)| *o);
                let first = first.map_or("another command", Command::label);
                let whose = if named.contains(&command) { "" } else { " (its key by default)" };
                problems.push(format!("shortcuts: {chord} is taken by \"{first}\", not \"{}\"{whose}", command.label()));
            }
            if !lost.is_empty() {
                shortcuts.put(command, kept.clone());
            }
            given.extend(kept.into_iter().map(|c| (command, c)));
        }
        (shortcuts, problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::Modifiers;

    /// Every chord written is read back as it was; what names no chord is not read.
    #[test]
    fn chords_are_read_as_they_are_written() {
        for text in ["Ctrl+Shift+Z", "Delete", "F1", "Ctrl+,", "Alt+F12", "Ctrl+Alt+Shift+9", "Space", "Ctrl+\\", "Shift+Left"] {
            let chord = Chord::parse(text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(chord.to_string(), text);
        }
        assert_eq!(Chord::parse(" shift + ctrl + z ").map(|c| c.to_string()).as_deref(), Some("Ctrl+Shift+Z"));
        for bad in ["", "Ctrl+", "Hyper+Z", "Ctrl+Я", "F13", "Ctrl+ZZ", "Ctrl++"] {
            assert_eq!(Chord::parse(bad), None, "{bad:?}");
        }
    }

    /// A key press is the same chord on every layout: a letter by the Latin one it types, a
    /// sign or a digit by its key.
    #[test]
    fn a_chord_is_the_same_on_every_layout() {
        let ctrl = Modifiers::CTRL;
        let z = |typed: &str, code| Chord::of(&Key::Character(typed.into()), Physical::Code(code), ctrl);
        let expected = Chord::parse("Ctrl+Z");
        assert_eq!(z("z", Code::KeyZ), expected);
        assert_eq!(z("я", Code::KeyZ), expected, "Russian");
        assert_eq!(z("z", Code::KeyW), expected, "AZERTY types z on W's key");
        let comma = Chord::of(&Key::Character("б".into()), Physical::Code(Code::Comma), ctrl);
        assert_eq!(comma, Chord::parse("Ctrl+,"), "the comma's key");
        let shifted = Chord::of(&Key::Character("<".into()), Physical::Code(Code::Comma), ctrl | Modifiers::SHIFT);
        assert_eq!(shifted, Chord::parse("Ctrl+Shift+,"));
        let digit = Chord::of(&Key::Character("&".into()), Physical::Code(Code::Digit1), Modifiers::default());
        assert_eq!(digit, Chord::parse("1"), "AZERTY types & on 1's key");
        let azerty = |typed: &str, code| Chord::of(&Key::Character(typed.into()), Physical::Code(code), ctrl);
        assert_eq!(azerty(",", Code::KeyM), Chord::parse("Ctrl+,"), "AZERTY's comma");
        assert_eq!(azerty("m", Code::Semicolon), Chord::parse("Ctrl+M"), "AZERTY's M");
        assert_eq!(azerty(";", Code::Comma), Chord::parse("Ctrl+;"));
        assert_eq!(azerty("ù", Code::Quote), Chord::parse("Ctrl+'"), "a letter no key name has: its key's");
        let option = Chord::of(&Key::Character("ß".into()), Physical::Code(Code::KeyS), Modifiers::ALT);
        assert_eq!(option, Chord::parse("Alt+S"), "what Option types on a Mac");
        let omega = Chord::of(&Key::Character("Ω".into()), Physical::Code(Code::KeyZ), Modifiers::ALT);
        assert_eq!(omega, Chord::parse("Alt+Z"));
        assert_eq!(Chord::of(&Key::Named(Named::Escape), Physical::Code(Code::Escape), Modifiers::default()), None);
        assert_eq!(Chord::of(&Key::Named(Named::Shift), Physical::Code(Code::ShiftLeft), Modifiers::SHIFT), None);
    }

    /// The defaults say what the program always did, and no two commands given from one place
    /// share a chord.
    #[test]
    fn the_defaults_are_whole() {
        let shortcuts = Shortcuts::default();
        for command in Command::ALL {
            assert_eq!(Shortcuts::defaults(*command).len(), command.defaults().len(), "{command:?}");
            for other in Command::ALL.iter().filter(|o| *o != command && o.place().meets(command.place())) {
                let shared: Vec<_> = shortcuts.of(*command).into_iter().filter(|c| shortcuts.of(*other).contains(c)).collect();
                assert!(shared.is_empty(), "{command:?} and {other:?} share {shared:?}");
            }
        }
        let chord = |t: &str| Chord::parse(t).unwrap();
        assert_eq!(shortcuts.command(chord("Ctrl+Shift+Z"), false, false, false), Some(Command::Redo));
        assert_eq!(shortcuts.command(chord("Ctrl+Z"), true, false, false), None, "not while typing");
        assert_eq!(shortcuts.command(chord("Ctrl+S"), true, false, false), Some(Command::Save), "saving, even so");
        let mut typed = shortcuts.clone();
        typed.set(Command::Save, vec![chord("S"), chord("F2")]);
        assert_eq!(typed.command(chord("S"), true, false, false), None, "a key that types, types");
        assert_eq!(typed.command(chord("F2"), true, false, false), Some(Command::Save));
        assert_eq!(typed.command(chord("S"), false, false, false), Some(Command::Save));
        assert_eq!(shortcuts.command(chord("Space"), false, true, false), Some(Command::AddNode));
        assert_eq!(shortcuts.command(chord("Space"), false, false, true), Some(Command::ShowTiling));
        assert_eq!(shortcuts.command(chord("Space"), false, false, false), None);
        // As the keys always were: Shift or Alt held too, where no command has the chord.
        for (pressed, command) in [
            ("Shift+Delete", Command::Delete),
            ("Alt+Backspace", Command::Delete),
            ("Shift+F", Command::Frame),
            ("Shift+F1", Command::Help),
            ("Ctrl+Shift+C", Command::Copy),
            ("Ctrl+Alt+Z", Command::Undo),
            ("Ctrl+Shift+Y", Command::Redo),
            ("Ctrl+Alt+Shift+V", Command::Paste),
            ("Ctrl+Shift+S", Command::SaveAs),
            ("Ctrl+Shift+Z", Command::Redo),
        ] {
            assert_eq!(shortcuts.command(chord(pressed), false, false, false), Some(command), "{pressed}");
        }
        assert_eq!(shortcuts.command(chord("Shift+Space"), false, true, false), Some(Command::AddNode));
        assert_eq!(shortcuts.command(chord("Ctrl+F"), false, false, false), None, "Ctrl is never dropped");
        assert_eq!(shortcuts.command(chord("Shift+F"), true, false, false), None, "typed");
        // A chord given with Shift is taken before the one without.
        let mut given = shortcuts.clone();
        given.set(Command::AddComment, vec![chord("Shift+F")]);
        assert_eq!(given.command(chord("Shift+F"), false, true, false), Some(Command::AddComment));
        assert_eq!(given.command(chord("F"), false, true, false), Some(Command::Frame));
    }

    /// A chord given to a command goes from the one that had it; a command's own chords are
    /// each once and at most two; given back its defaults, it has nothing changed.
    #[test]
    fn a_chord_given_to_one_goes_from_the_other() {
        let chord = |t: &str| Chord::parse(t).unwrap();
        let mut shortcuts = Shortcuts::default();
        let taken = shortcuts.set(Command::Frame, vec![chord("Ctrl+D"), chord("Ctrl+D"), chord("F2"), chord("F3")]);
        assert_eq!(taken, [(Command::Duplicate, chord("Ctrl+D"))]);
        assert_eq!(shortcuts.of(Command::Frame), [chord("Ctrl+D"), chord("F2")]);
        assert!(shortcuts.of(Command::Duplicate).is_empty());
        assert_eq!(shortcuts.command(chord("Ctrl+D"), false, false, false), Some(Command::Frame));
        // Space over the graph and over the 2D view are apart: one does not take the other's.
        assert!(shortcuts.set(Command::AddNode, vec![chord("Space")]).is_empty());
        assert_eq!(shortcuts.of(Command::ShowTiling), [chord("Space")]);
        shortcuts.set(Command::Frame, Shortcuts::defaults(Command::Frame));
        shortcuts.set(Command::Duplicate, Shortcuts::defaults(Command::Duplicate));
        assert_eq!(shortcuts, Shortcuts::default(), "nothing changed");
    }

    /// What `settings.toml` says: keys given as the dialog gives them are read back as they
    /// were, in any order; a key given twice is the first's, said; what cannot be read is
    /// said, and a command given nothing it can read keeps its own keys.
    #[test]
    fn shortcuts_are_read_as_the_file_says() {
        let read = |text: &str| {
            let doc: toml_edit::DocumentMut = text.parse().unwrap();
            Shortcuts::read(doc.get("shortcuts").and_then(toml_edit::Item::as_table_like))
        };
        let chord = |t: &str| Chord::parse(t).unwrap();
        // As the dialog writes it: Frame took Duplicate's key.
        let mut given = Shortcuts::default();
        given.set(Command::Frame, vec![chord("Ctrl+D")]);
        for text in ["[shortcuts]\nframe = [\"Ctrl+D\"]\nduplicate = []\n", "[shortcuts]\nduplicate = []\nframe = [\"Ctrl+D\"]\n"] {
            assert_eq!(read(text), (given.clone(), Vec::new()), "{text}");
        }
        // By hand, Duplicate not said: it keeps nothing of Frame's, and that is said.
        let (shortcuts, problems) = read("[shortcuts]\nframe = \"Ctrl+D\"\n");
        assert_eq!(shortcuts, given);
        assert_eq!(problems, ["shortcuts: Ctrl+D is taken by \"Frame Selection / All\", not \"Duplicate\" (its key by default)"]);
        // Two commands said with one key: the first keeps it.
        let (shortcuts, problems) = read("[shortcuts]\nundo = [\"F5\"]\nredo = [\"F5\", \"F6\"]\n");
        assert_eq!((shortcuts.of(Command::Undo), shortcuts.of(Command::Redo)), (vec![chord("F5")], vec![chord("F6")]));
        assert_eq!(problems.len(), 1, "{problems:?}");
        // What cannot be read.
        let (shortcuts, problems) = read("[shortcuts]\nredo = [\"Hyper+Y\"]\nfly = [\"F9\"]\nundo = 5\nsave = [\"F2\", \"F3\", \"F4\"]\n");
        assert_eq!(shortcuts.of(Command::Redo), Shortcuts::defaults(Command::Redo), "nothing it can read: its own");
        assert_eq!(shortcuts.of(Command::Undo), Shortcuts::defaults(Command::Undo));
        assert_eq!(shortcuts.of(Command::Save), [chord("F2"), chord("F3")]);
        assert_eq!(problems.len(), 3, "a command this version does not have is left alone: {problems:?}");
    }
}
