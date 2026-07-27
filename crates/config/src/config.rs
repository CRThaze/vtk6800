//! Local config-file storage. The keymap is the source of truth (the firmware
//! has no keymap read-back), and an "applied" snapshot enables `diff`. Each
//! board variant gets its own keymap file so multiple models can be managed
//! side by side; `settings.toml` records which variant is the default.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use vtk6800_core::{Keymap, Variant};

pub struct Paths {
    pub keymap: PathBuf,
    pub applied: PathBuf,
}

impl Paths {
    /// Resolve config paths for `variant`. Without `--config`, the keymap lives
    /// at `<config-dir>/<variant>-keymap.toml` so each model is tracked
    /// separately; an explicit `--config` overrides that with a single file.
    pub fn resolve(explicit: Option<PathBuf>, variant: Variant) -> Result<Paths> {
        let keymap = match explicit {
            Some(p) => p,
            None => default_dir()?.join(keymap_filename(variant)),
        };
        let applied = keymap.with_extension("applied.yaml");
        Ok(Paths { keymap, applied })
    }
}

fn keymap_filename(variant: Variant) -> String {
    format!("{}-keymap.yaml", variant.as_str())
}

fn default_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "vtk6800")
        .context("locating the platform config directory")?;
    Ok(dirs.config_dir().to_path_buf())
}

/// Directory holding named presets (per-variant saved keymaps).
fn presets_dir() -> Result<PathBuf> {
    Ok(default_dir()?.join("presets"))
}

fn valid_preset_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Path to the named preset for `variant`. Rejects names that aren't a single
/// safe filename component (no path separators, `.` or `..`).
pub fn preset_path(variant: Variant, name: &str) -> Result<PathBuf> {
    if !valid_preset_name(name) {
        bail!("invalid preset name {name:?} (use letters, digits, '-', '_', '.')");
    }
    Ok(presets_dir()?.join(format!("{}-{}.yaml", variant.as_str(), name)))
}

/// Names of the existing presets for `variant`, sorted.
pub fn list_presets(variant: Variant) -> Result<Vec<String>> {
    let prefix = format!("{}-", variant.as_str());
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(presets_dir()?) {
        for e in entries.flatten() {
            let fname = e.file_name();
            let fname = fname.to_string_lossy();
            if let Some(name) = fname
                .strip_prefix(&prefix)
                .and_then(|r| r.strip_suffix(".yaml"))
            {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Global settings, persisted in `<config-dir>/settings.toml`, independent of
/// any single keymap.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Variant used when `--variant` is omitted. Captured on first use, or set
    /// explicitly via the `default` command.
    pub default_variant: Option<String>,
}

impl Settings {
    pub fn path() -> Result<PathBuf> {
        Ok(default_dir()?.join("settings.toml"))
    }

    pub fn load() -> Result<Settings> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Settings::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).context("serializing settings")?;
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
    }

    /// The configured default variant, if set and recognized.
    pub fn default_variant(&self) -> Option<Variant> {
        self.default_variant.as_deref().and_then(Variant::parse)
    }
}

pub fn load(path: &Path) -> Result<Keymap> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    vtk6800_core::document::from_yaml(&text).with_context(|| format!("parsing {}", path.display()))
}

pub fn save(km: &Keymap, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let text = vtk6800_core::document::to_yaml(km).context("serializing keymap")?;
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Load the config, or seed it from the variant default on first use.
pub fn load_or_seed(variant: Variant, paths: &Paths) -> Result<Keymap> {
    if paths.keymap.exists() {
        load(&paths.keymap)
    } else {
        let km = variant.default_keymap()?;
        save(&km, &paths.keymap)?;
        Ok(km)
    }
}

pub fn load_applied(paths: &Paths) -> Result<Option<Keymap>> {
    if paths.applied.exists() {
        Ok(Some(load(&paths.applied)?))
    } else {
        Ok(None)
    }
}

pub fn save_applied(km: &Keymap, paths: &Paths) -> Result<()> {
    save(km, &paths.applied)
}
