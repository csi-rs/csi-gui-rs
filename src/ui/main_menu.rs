/// Items in the modal main menu (M key / "File" menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainMenuItem {
    /// Open the theme picker overlay.
    Theme,
    /// Persist the current tiling layout as a named template.
    SaveTemplate,
    /// Load a previously saved template.
    LoadTemplate,
    /// Export captured history to a CSV / JSONL / Parquet file.
    ExportData,
    /// Import a previously exported file and enter replay mode.
    ImportData,
    /// Open the post-capture preprocessing overlay.
    Preprocess,
    /// Pulse DTR/RTS to reset the connected ESP and re-identify.
    ResetEsp,
    /// Open the device-config overlay (`set-*` knobs).
    ConfigureEsp,
    /// Toggle Start/Stop collection (label flips on `is_collecting`).
    StartStop,
    /// Close the menu without taking any action.
    Close,
}

impl MainMenuItem {
    /// Items rendered in this order, top-to-bottom, by [`MainMenu::show`].
    pub const ALL: [MainMenuItem; 10] = [
        MainMenuItem::Theme,
        MainMenuItem::SaveTemplate,
        MainMenuItem::LoadTemplate,
        MainMenuItem::ExportData,
        MainMenuItem::ImportData,
        MainMenuItem::Preprocess,
        MainMenuItem::ResetEsp,
        MainMenuItem::ConfigureEsp,
        MainMenuItem::StartStop,
        MainMenuItem::Close,
    ];

    /// Human label for the row; the Start/Stop label flips on
    /// `is_collecting`.
    pub fn label(self, is_collecting: bool) -> &'static str {
        match self {
            MainMenuItem::Theme => "Theme…",
            MainMenuItem::SaveTemplate => "Save Template",
            MainMenuItem::LoadTemplate => "Load Template",
            MainMenuItem::ExportData => "Export Data",
            MainMenuItem::ImportData => "Import Data (Replay)",
            MainMenuItem::Preprocess => "Preprocessing…",
            MainMenuItem::ResetEsp => "Reset ESP",
            MainMenuItem::ConfigureEsp => "Configure ESP",
            MainMenuItem::StartStop => {
                if is_collecting {
                    "Stop Collection"
                } else {
                    "Start Collection"
                }
            }
            MainMenuItem::Close => "Close",
        }
    }
}

#[derive(Default)]
pub struct MainMenu {
    pub open: bool,
    pub selected: usize,
}

impl MainMenu {
    /// Render the main menu window.
    ///
    /// `is_collecting` flips the Start/Stop label. `start_enabled` gates
    /// the Start/Stop row when no esp-csi-cli-rs firmware is identified —
    /// while collecting the row stays enabled (so the user can still
    /// Stop), but Start is disabled offline. Keyboard `Enter` and the
    /// arrow keys honour the same gate.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        is_collecting: bool,
        start_enabled: bool,
    ) -> Option<MainMenuItem> {
        if !self.open {
            return None;
        }
        let mut chosen = None;
        let is_enabled = |item: MainMenuItem| match item {
            MainMenuItem::StartStop => is_collecting || start_enabled,
            // Importing a capture would clobber the live stream — keep
            // it locked while collection is active.
            MainMenuItem::ImportData => !is_collecting,
            _ => true,
        };
        egui::Window::new("Main Menu")
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .show(ctx, |ui| {
                for (i, item) in MainMenuItem::ALL.iter().enumerate() {
                    let selected = i == self.selected;
                    let label = item.label(is_collecting);
                    let text = if selected {
                        egui::RichText::new(format!("▶ {label}")).strong()
                    } else {
                        egui::RichText::new(format!("  {label}"))
                    };
                    let enabled = is_enabled(*item);
                    let resp = ui.add_enabled(enabled, egui::Button::new(text));
                    let resp = match (item, enabled) {
                        (MainMenuItem::StartStop, false) => resp.on_disabled_hover_text(
                            "Connect an ESP32 running esp-csi-cli-rs firmware first.",
                        ),
                        (MainMenuItem::ImportData, false) => resp.on_disabled_hover_text(
                            "Stop the live collection first.",
                        ),
                        _ => resp,
                    };
                    if resp.clicked() {
                        chosen = Some(*item);
                        self.open = false;
                    }
                }
                ctx.input(|i| {
                    if i.key_pressed(egui::Key::ArrowDown) {
                        self.selected = (self.selected + 1) % MainMenuItem::ALL.len();
                    }
                    if i.key_pressed(egui::Key::ArrowUp) {
                        self.selected =
                            (self.selected + MainMenuItem::ALL.len() - 1) % MainMenuItem::ALL.len();
                    }
                    if i.key_pressed(egui::Key::Enter) {
                        let item = MainMenuItem::ALL[self.selected];
                        if is_enabled(item) {
                            chosen = Some(item);
                            self.open = false;
                        }
                    }
                    if i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::M) {
                        self.open = false;
                    }
                });
            });
        chosen
    }
}
