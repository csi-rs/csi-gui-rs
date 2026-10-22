//! On-disk persistence: layout templates and CSI captures.
//!
//! - Templates are JSON files in [`crate::paths::templates_dir`], plus
//!   the read-only templates bundled into the binary ([`BUILTIN_TEMPLATES`]).
//! - Captures (CSV / JSONL / Parquet) are in [`export`]; live per-device
//!   JSONL recording is [`SessionRecorder`].

pub mod export;
pub mod recording;

pub use export::{ExportFormat, export_packets, import_packets};
pub use recording::{RecordingSummary, SessionRecorder};

use crate::data::PreprocessConfig;
use crate::layout::TilingManager;
use crate::views::ViewStates;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Templates compiled into the binary, as `(name, json)`. They show up in
/// the template picker next to the user's own and can't be overwritten;
/// saving under the same name creates a user copy that shadows them.
pub const BUILTIN_TEMPLATES: &[(&str, &str)] = &[(
    "showcase",
    include_str!("../../assets/templates/showcase.json"),
)];

/// A saved workspace template — the tiling layout, the active
/// preprocessing config, and the per-pane view knobs (mode toggles,
/// window sizes, …). The wrapping struct is versioned so older
/// templates (which serialised a bare `TilingManager`) keep loading.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Template {
    /// Tiling layout (panes, splits, focused id, theme variant).
    pub tiling: TilingManager,
    /// Active preprocessing filters at save time.
    #[serde(default)]
    pub preprocess: PreprocessConfig,
    /// Per-pane view knobs (magnitude scale, phase mode, …).
    #[serde(default)]
    pub view_states: ViewStates,
}

impl From<TilingManager> for Template {
    fn from(tm: TilingManager) -> Self {
        Self {
            tiling: tm,
            preprocess: PreprocessConfig::default(),
            view_states: ViewStates::default(),
        }
    }
}

/// Where a listed template comes from.
#[derive(Clone, Debug)]
pub enum TemplateSource {
    /// Bundled into the binary; the JSON text.
    Builtin(&'static str),
    /// A file in [`crate::paths::templates_dir`].
    File(PathBuf),
}

/// Listing entry for the template picker overlay.
#[derive(Clone, Debug)]
pub struct TemplateEntry {
    /// Display name (file stem without `.json`).
    pub name: String,
    /// Where to load it from.
    pub source: TemplateSource,
    /// Whether this template is the auto-load-on-startup default.
    pub is_default: bool,
}

impl TemplateEntry {
    /// Parse the template this entry points at.
    pub fn load(&self) -> std::io::Result<Template> {
        match &self.source {
            TemplateSource::Builtin(json) => parse_template(json),
            TemplateSource::File(path) => read_template(path),
        }
    }

    /// `true` for templates bundled into the binary.
    pub fn is_builtin(&self) -> bool {
        matches!(self.source, TemplateSource::Builtin(_))
    }
}

/// Persist a template as `<templates_dir>/<name>.json`.
pub fn save_template(name: &str, tpl: &Template) -> std::io::Result<PathBuf> {
    let dir = crate::paths::templates_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.json"));
    let s = serde_json::to_string_pretty(tpl).map_err(std::io::Error::other)?;
    fs::write(&path, s)?;
    Ok(path)
}

/// Enumerate the user's templates plus any bundled ones they haven't
/// shadowed, sorted by name.
pub fn list_templates() -> Vec<TemplateEntry> {
    let mut out = Vec::new();
    let entries = fs::read_dir(crate::paths::templates_dir())
        .into_iter()
        .flatten();
    for ent in entries.flatten() {
        let path = ent.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let is_default = read_template(&path)
            .map(|tpl| tpl.tiling.is_default)
            .unwrap_or(false);
        out.push(TemplateEntry {
            name,
            source: TemplateSource::File(path),
            is_default,
        });
    }
    for (name, json) in BUILTIN_TEMPLATES {
        if !out.iter().any(|e| e.name == *name) {
            out.push(TemplateEntry {
                name: (*name).to_string(),
                source: TemplateSource::Builtin(json),
                is_default: false,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Parse a template file. Falls back to the legacy "bare
/// `TilingManager`" layout so templates saved before the `Template`
/// wrapper landed still load — they come back with default
/// preprocessing and empty view states.
pub fn read_template(path: &Path) -> std::io::Result<Template> {
    parse_template(&fs::read_to_string(path)?)
}

/// Parse template JSON. See [`read_template`].
pub fn parse_template(s: &str) -> std::io::Result<Template> {
    if let Ok(tpl) = serde_json::from_str::<Template>(s) {
        return Ok(tpl);
    }
    let tm: TilingManager = serde_json::from_str(s)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(tm.into())
}

/// Look up the template marked `is_default = true`, if any.
pub fn load_startup_template() -> Option<Template> {
    list_templates()
        .into_iter()
        .find(|e| e.is_default)
        .and_then(|e| e.load().ok())
}

/// Toggle the `is_default` flag so exactly the named template loads on
/// next startup. Other templates have their `is_default` cleared in
/// the same pass.
///
/// Marking a bundled template as default first saves a user copy of it,
/// since the bundled one can't be written.
pub fn set_default(name: &str) -> std::io::Result<()> {
    for ent in list_templates() {
        let Ok(mut tpl) = ent.load() else { continue };
        let want = ent.name == name;
        match &ent.source {
            TemplateSource::File(path) => {
                tpl.tiling.is_default = want;
                let s = serde_json::to_string_pretty(&tpl).map_err(std::io::Error::other)?;
                fs::write(path, s)?;
            }
            TemplateSource::Builtin(_) if want => {
                tpl.tiling.is_default = true;
                save_template(name, &tpl)?;
            }
            TemplateSource::Builtin(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod showcase_template_tests {
    use super::*;

    /// Make sure the bundled showcase template keeps parsing through the
    /// current `Template` schema — if a future refactor renames a serde
    /// tag or adds a required field, this catches it before the binary
    /// tries to load it.
    #[test]
    fn bundled_showcase_template_parses() {
        let (_, json) = BUILTIN_TEMPLATES[0];
        let tpl = parse_template(json).expect("showcase.json must parse");
        // Sanity: layout has five panes (Magnitude / Phase /
        // Dashboard / Spectrogram / Evolution).
        let mut ids = Vec::new();
        tpl.tiling.root.collect_panes(&mut ids);
        assert_eq!(
            ids.len(),
            5,
            "showcase template should have 5 panes, found {ids:?}"
        );
        // Sanity: per-pane state matches each pane id.
        for id in &ids {
            assert!(
                tpl.view_states.per_pane.contains_key(id),
                "missing view_state for pane #{id}"
            );
        }
    }
}
