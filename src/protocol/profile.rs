//! Pluggable capability seam for optional, out-of-tree CSI extensions.
//!
//! This crate knows only the acquisition flags and presets the firmware it
//! ships against accepts. A [`CsiProfile`] lets a build extend the preset set
//! without this crate naming any of the extra values: install one profile at
//! startup and the config panel enumerates its presets while
//! [`render_apply_config`](crate::protocol::command::render_apply_config)
//! resolves the selected name into the `set-csi` line to push.
//!
//! All methods are defaulted, so [`StandardCsiProfile`] is a transparent no-op
//! and an embedder overrides only what it needs.

use super::config::EspConfig;
use std::sync::OnceLock;

/// Optional, embedder-supplied CSI capability extensions.
pub trait CsiProfile: Send + Sync {
    /// Preset names offered by the config panel, in display order. Empty (the
    /// default) hides the preset row entirely.
    fn preset_names(&self) -> &'static [&'static str] {
        &[]
    }

    /// Full `set-csi …` command line a named preset pushes. `None` means this
    /// profile does not recognise the name, and the individual acquisition
    /// flags are emitted instead.
    fn preset_cli(&self, name: &str) -> Option<String> {
        let _ = name;
        None
    }

    /// Fold a named preset into `cfg` so the panel's individual toggles show
    /// what the device will actually run. `false` means the name is unknown.
    fn apply_preset(&self, name: &str, cfg: &mut EspConfig) -> bool {
        let _ = (name, cfg);
        false
    }
}

/// The default no-op profile: no presets to offer or resolve.
pub struct StandardCsiProfile;

impl CsiProfile for StandardCsiProfile {}

static STANDARD: StandardCsiProfile = StandardCsiProfile;
static ACTIVE: OnceLock<&'static dyn CsiProfile> = OnceLock::new();

/// Install the process-wide profile, ideally before the first frame. The first
/// call wins so the seam cannot change under a panel that is already rendering
/// a preset list.
pub fn install_profile(profile: &'static dyn CsiProfile) {
    let _ = ACTIVE.set(profile);
}

/// The installed profile, or [`StandardCsiProfile`] when none was installed.
pub fn active_profile() -> &'static dyn CsiProfile {
    match ACTIVE.get() {
        Some(p) => *p,
        None => &STANDARD,
    }
}
