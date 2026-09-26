//! How the window was laid out when it was closed: where its panels are split, how big it
//! was, whether it filled the screen, whether the 3D view drew frame after frame and what lit
//! it. Kept in a file of its own in the user's settings folder, read when the program starts
//! and written when it quits. (Not where the window was: that screen may not be there next
//! time.)

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::view::view3d::{Lighting, RECENT};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    /// Where the panels are split, in the order `app::panes` makes the splits in.
    pub splits: Vec<f32>,
    /// The window's size in logical pixels, when it last did not fill the screen.
    pub size: Option<[f32; 2]>,
    pub maximized: bool,
    pub realtime: Option<bool>,
    /// The environment image that lit the 3D view; `None`, the studio.
    pub environment: Option<String>,
    /// The environment images used last, the latest first.
    pub environments: Vec<String>,
    /// The 3D view's brightness, in stops.
    pub exposure: Option<f32>,
    /// Whether the environment image showed behind the model, and how blurred.
    pub background: Option<bool>,
    pub blur: Option<f32>,
}

impl Workspace {
    /// What lit the 3D view (the image itself is read again).
    pub fn lighting(&self) -> Lighting {
        let default = Lighting::default();
        let finite = |v: Option<f32>| v.filter(|v| v.is_finite());
        let mut lighting = Lighting {
            file: self.environment.clone(),
            recent: self.environments.iter().take(RECENT).cloned().collect(),
            exposure: finite(self.exposure).map_or(default.exposure, |e| e.clamp(-12.0, 12.0)),
            background: self.background.unwrap_or(default.background),
            blur: finite(self.blur).map_or(default.blur, |b| b.clamp(0.0, 1.0)),
            ..default
        };
        if let Some(file) = self.environment.clone() {
            lighting.remember(&file);
        }
        lighting
    }

    /// This workspace, with what lights the 3D view.
    pub fn with_lighting(self, lighting: &Lighting) -> Workspace {
        Workspace {
            environment: lighting.file.clone(),
            environments: lighting.recent.clone(),
            exposure: Some(lighting.exposure),
            background: Some(lighting.background),
            blur: Some(lighting.blur),
            ..self
        }
    }
}

/// The folder of the program's settings: where each system keeps them.
pub fn folder() -> Option<PathBuf> {
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
        // Whole or not at all, whatever stops meanwhile.
        brewed_texture_core::files::write_whole(&path, &json)
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
            environment: Some("C:/skies/noon.hdr".into()),
            environments: vec!["C:/skies/noon.hdr".into(), "/home/me/dusk.exr".into()],
            exposure: Some(-1.5),
            background: Some(false),
            blur: Some(0.6),
        };
        let json = serde_json::to_string(&w).unwrap();
        assert_eq!(serde_json::from_str::<Workspace>(&json).unwrap(), w);
        // What a file does not say keeps its default.
        assert_eq!(serde_json::from_str::<Workspace>("{}").unwrap(), Workspace::default());
        assert_eq!(serde_json::from_str::<Workspace>(r#"{"maximized": true}"#).unwrap().splits, Vec::<f32>::new());
    }

    #[test]
    fn the_lighting_comes_back_as_it_was() {
        let mut lighting = Lighting { exposure: 2.5, background: false, blur: 0.1, ..Lighting::default() };
        lighting.remember("/skies/a.hdr");
        lighting.remember("/skies/b.exr");
        lighting.file = Some("/skies/b.exr".into());
        let back = Workspace::default().with_lighting(&lighting).lighting();
        assert_eq!(back.file, lighting.file);
        assert_eq!(back.recent, vec!["/skies/b.exr".to_string(), "/skies/a.hdr".to_string()]);
        assert_eq!((back.exposure, back.background, back.blur), (2.5, false, 0.1));
        assert!(back.shown.is_none() && !back.menu);
        // The studio, when nothing is said; nothing out of bounds, whatever the file says.
        let studio = Workspace::default().lighting();
        assert_eq!(studio.file, None);
        assert_eq!(studio.exposure, 0.0);
        let odd = Workspace { exposure: Some(f32::NAN), blur: Some(7.0), environment: Some("x.hdr".into()), ..Workspace::default() }.lighting();
        assert_eq!((odd.exposure, odd.blur), (0.0, 1.0));
        assert_eq!(odd.recent, vec!["x.hdr".to_string()]);
    }
}
