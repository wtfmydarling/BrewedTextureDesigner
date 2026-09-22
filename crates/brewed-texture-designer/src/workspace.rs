//! How the window was laid out when it was closed: where its panels are split, how big it
//! was, whether it filled the screen, and whether the 3D view drew frame after frame. Kept in
//! a file of its own in the user's settings folder, read when the program starts and written
//! when it quits. (Not where the window was: that screen may not be there next time.)

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    /// Where the panels are split, in the order `app::panes` makes the splits in.
    pub splits: Vec<f32>,
    /// The window's size in logical pixels, when it last did not fill the screen.
    pub size: Option<[f32; 2]>,
    pub maximized: bool,
    pub realtime: Option<bool>,
}

/// The folder of the program's settings: where each system keeps them.
fn folder() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        var("APPDATA").map(|d| d.join("Brewed Texture Designer"))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library/Application Support/Brewed Texture Designer"))
    } else {
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config"))).map(|d| d.join("brewed-texture-designer"))
    }
}

fn path() -> Option<PathBuf> {
    folder().map(|f| f.join("workspace.json"))
}

impl Workspace {
    /// What was saved; the defaults when nothing was, or what was cannot be read.
    pub fn load() -> Workspace {
        let Some(path) = path() else { return Workspace::default() };
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                log::warn!("{}: {e}; the layout starts afresh", path.display());
                Workspace::default()
            }),
            Err(_) => Workspace::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = path().ok_or("There is no folder for settings")?;
        let folder = path.parent().ok_or("There is no folder for settings")?;
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let json = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        // Written beside it first: a crash never leaves half a file.
        let written = path.with_extension("json.new");
        std::fs::write(&written, json).map_err(|e| format!("{}: {e}", written.display()))?;
        std::fs::rename(&written, &path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workspace_reads_back_and_old_files_still_read() {
        let w = Workspace {
            splits: vec![0.2, 0.7, 0.55, 0.5, 0.3],
            size: Some([1400.0, 900.0]),
            maximized: true,
            realtime: Some(false),
        };
        let json = serde_json::to_string(&w).unwrap();
        assert_eq!(serde_json::from_str::<Workspace>(&json).unwrap(), w);
        // What a file does not say keeps its default.
        assert_eq!(serde_json::from_str::<Workspace>("{}").unwrap(), Workspace::default());
        assert_eq!(serde_json::from_str::<Workspace>(r#"{"maximized": true}"#).unwrap().splits, Vec::<f32>::new());
    }
}
