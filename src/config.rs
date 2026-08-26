//! Persisted settings: `~/.config/glimpse/config.toml`.
//!
//! Deliberately small. Everything here is a user choice that must survive a
//! restart; anything derived, computed or per-session belongs elsewhere.
//!
//! Loading never fails. A missing file is the default config, and a corrupt one
//! is reported and then ignored — losing a preference is annoying, but refusing
//! to start a screen recorder because a settings file has a stray bracket in it
//! is worse.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::encode::OutputFormat;

/// Which palette to paint. `System` follows the desktop's dark-mode preference
/// and keeps following it while the app runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub fn all() -> [Theme; 3] {
        [Theme::System, Theme::Light, Theme::Dark]
    }

    pub fn id(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::System => "Follow system",
            Theme::Light => "Light",
            Theme::Dark => "Dark",
        }
    }

    pub fn from_id(id: &str) -> Option<Theme> {
        Theme::all().into_iter().find(|t| t.id() == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme: Theme,
    pub format: OutputFormat,
    /// Directory recordings are written to. The filename is still chosen by
    /// Glimpse, and still disambiguated rather than overwritten.
    pub output_dir: PathBuf,
    pub framerate: u32,
    pub capture_mouse: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            format: OutputFormat::default(),
            output_dir: default_output_dir(),
            framerate: 15,
            capture_mouse: true,
        }
    }
}

/// Where recordings go before the user says otherwise.
///
/// `XDG_VIDEOS_DIR` if the user has one, else `$HOME`. Not `/tmp`: a recording
/// somebody just made is not scratch data.
pub fn default_output_dir() -> PathBuf {
    if let Some(dir) = xdg_user_dir("VIDEOS") {
        return dir;
    }
    home()
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Read one entry out of `~/.config/user-dirs.dirs` without pulling in a crate
/// for it. Lines look like `XDG_VIDEOS_DIR="$HOME/Videos"`.
fn xdg_user_dir(name: &str) -> Option<PathBuf> {
    if let Some(v) = std::env::var_os(format!("XDG_{name}_DIR")) {
        return Some(PathBuf::from(v));
    }
    let text = std::fs::read_to_string(config_home().join("user-dirs.dirs")).ok()?;
    let key = format!("XDG_{name}_DIR=");
    let raw = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| l.strip_prefix(&key))?
        .trim_matches('"');
    let expanded = raw.strip_prefix("$HOME").map_or_else(
        || PathBuf::from(raw),
        |rest| home().join(rest.trim_start_matches('/')),
    );
    expanded.is_dir().then_some(expanded)
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
}

pub fn config_path() -> PathBuf {
    config_home().join("glimpse").join("config.toml")
}

impl Config {
    /// Load, falling back to defaults. Never fails; see the module docs.
    pub fn load() -> Self {
        Self::load_from(&config_path())
    }

    pub fn load_from(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match toml::from_str(&text) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("glimpse: ignoring {}: {e}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&config_path())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, body)
    }

    /// The full path a new recording should be written to, before collision
    /// handling. The filename is Glimpse's; only the directory is the user's.
    pub fn destination(&self) -> PathBuf {
        self.output_dir
            .join(format!("glimpse.{}", self.format.extension()))
    }
}
