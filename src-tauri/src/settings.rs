//! Settings kept between runs, in one small JSON file in the app's config
//! folder. Besides the memory notes (not built yet) this is the only thing
//! the app writes to disk. It never holds problems, answers or history.
//!
//! "Start with Windows" is not kept here: Windows holds that itself.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::calc;

/// How problems that need reasoning are answered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    /// Claude is not connected. The app is complete without it.
    #[default]
    None,
    /// The user's own installed Claude Code, on their plan.
    ClaudeCode,
}

/// Which colours the window uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Dark when Windows is dark, light when it is light.
    #[default]
    System,
    Dark,
    Light,
}

/// What to do with a definite error in the user's formula.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Correction {
    /// Work out what was typed, and flag the error.
    #[default]
    Flag,
    /// Show the corrected result, with a note of what was changed.
    Correct,
}

/// Which model answers, on the Claude Code route.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Model {
    /// Sonnet at low effort: the one measured against the reference set.
    #[default]
    Fast,
    /// Opus at medium effort: slower, for harder problems.
    Deeper,
}

impl Model {
    /// The model and effort as Claude Code names them.
    pub fn as_claude_code(self) -> (&'static str, &'static str) {
        match self {
            Model::Fast => ("sonnet", "low"),
            Model::Deeper => ("opus", "medium"),
        }
    }
}

/// The shortcut the app starts with, in the form the shortcut library reads.
pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+KeyC";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Set only after the user has agreed to connect Claude.
    pub claude: Route,
    /// Top-left corner of the window in physical pixels, if the user has
    /// dragged it away from the default corner.
    pub window_position: Option<(i32, i32)>,
    /// The shortcut that summons the window, as modifiers and a key code
    /// joined with `+` (`Ctrl+Alt+KeyC`).
    pub hotkey: String,
    pub theme: Theme,
    /// Significant figures answers are shown to.
    pub figures: u8,
    pub correction: Correction,
    pub model: Model,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            claude: Route::default(),
            window_position: None,
            hotkey: DEFAULT_HOTKEY.to_string(),
            theme: Theme::default(),
            figures: calc::DEFAULT_SIG_FIGS,
            correction: Correction::default(),
            model: Model::default(),
        }
    }
}

/// The shortcut as people write it: `Ctrl+Alt+C`, not `Ctrl+Alt+KeyC`.
pub fn hotkey_label(hotkey: &str) -> String {
    hotkey
        .split('+')
        .map(|part| part.strip_prefix("Key").or_else(|| part.strip_prefix("Digit")).unwrap_or(part))
        .collect::<Vec<_>>()
        .join("+")
}

fn file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join("settings.json"))
}

/// Reads settings from the text of the file. Anything missing takes its
/// default, so a file written by an older version still loads.
fn parse(text: &str) -> Settings {
    let mut settings: Settings = serde_json::from_str(text).unwrap_or_default();
    if !calc::SIG_FIGS_RANGE.contains(&settings.figures) {
        settings.figures = calc::DEFAULT_SIG_FIGS;
    }
    settings
}

/// The folder the settings were kept in while the app was called
/// Clawculator (up to 0.8.2). It sits beside the present one.
const OLD_FOLDER: &str = "com.clawculator.desktop";

/// Where the settings file was under the old name, given where it is now.
fn old_file(current: &Path) -> Option<PathBuf> {
    Some(current.parent()?.parent()?.join(OLD_FOLDER).join("settings.json"))
}

/// Missing or unreadable settings are not an error: the defaults apply.
///
/// The first time the app runs under its new name it has no settings file
/// of its own. It then takes the old one, so the shortcut, the window's
/// place and the Claude connection carry over, and saves it as its own so
/// that removing the old app does not lose them.
pub fn load(app: &AppHandle) -> Settings {
    let Some(path) = file(app) else { return Settings::default() };
    if let Ok(text) = fs::read_to_string(&path) {
        return parse(&text);
    }
    match old_file(&path).and_then(|old| fs::read_to_string(old).ok()) {
        Some(text) => {
            let settings = parse(&text);
            save(app, &settings);
            settings
        }
        None => Settings::default(),
    }
}

pub fn save(app: &AppHandle, settings: &Settings) {
    let Some(path) = file(app) else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(settings) {
        let _ = fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_from_an_older_version_still_loads() {
        // What 0.6 wrote: only these two fields.
        let settings = parse(r#"{ "claude": "claude_code", "window_position": [100, 200] }"#);
        assert_eq!(settings.claude, Route::ClaudeCode);
        assert_eq!(settings.window_position, Some((100, 200)));
        assert_eq!(settings.hotkey, DEFAULT_HOTKEY);
        assert_eq!(settings.figures, 4);
        assert_eq!(settings.theme, Theme::System);
        assert_eq!(settings.correction, Correction::Flag);
        assert_eq!(settings.model, Model::Fast);
    }

    #[test]
    fn settings_survive_being_written_and_read() {
        let chosen = Settings {
            hotkey: "Ctrl+Shift+F9".into(),
            theme: Theme::Light,
            figures: 6,
            correction: Correction::Correct,
            model: Model::Deeper,
            ..Settings::default()
        };
        let text = serde_json::to_string(&chosen).unwrap();
        assert_eq!(parse(&text), chosen);
    }

    #[test]
    fn the_old_settings_file_is_looked_for_beside_the_new_folder() {
        let now = Path::new("C:/Users/someone/AppData/Roaming/io.github.pawedfox.slipstick/settings.json");
        assert_eq!(
            old_file(now),
            Some(PathBuf::from("C:/Users/someone/AppData/Roaming").join(OLD_FOLDER).join("settings.json"))
        );
        // Nowhere to look if the path has no folders above it.
        assert_eq!(old_file(Path::new("settings.json")), None);
    }

    #[test]
    fn nonsense_falls_back_to_defaults() {
        assert_eq!(parse("not json"), Settings::default());
        assert_eq!(parse(r#"{ "figures": 40 }"#).figures, 4);
    }

    #[test]
    fn the_shortcut_is_labelled_as_people_write_it() {
        assert_eq!(hotkey_label("Ctrl+Alt+KeyC"), "Ctrl+Alt+C");
        assert_eq!(hotkey_label("Ctrl+Shift+Digit1"), "Ctrl+Shift+1");
        assert_eq!(hotkey_label("Alt+F9"), "Alt+F9");
        assert_eq!(hotkey_label("Ctrl+Alt+Space"), "Ctrl+Alt+Space");
    }
}
