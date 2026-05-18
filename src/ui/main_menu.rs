#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainMenuItem {
    Theme,
    SaveTemplate,
    LoadTemplate,
    ExportData,
    ResetEsp,
    ConfigureEsp,
    StartStop,
    Close,
}

impl MainMenuItem {
    pub const ALL: [MainMenuItem; 8] = [
        MainMenuItem::Theme,
        MainMenuItem::SaveTemplate,
        MainMenuItem::LoadTemplate,
        MainMenuItem::ExportData,
        MainMenuItem::ResetEsp,
        MainMenuItem::ConfigureEsp,
        MainMenuItem::StartStop,
        MainMenuItem::Close,
    ];

    pub fn label(self, is_collecting: bool) -> &'static str {
        match self {
            MainMenuItem::Theme => "Cycle Theme",
            MainMenuItem::SaveTemplate => "Save Template",
            MainMenuItem::LoadTemplate => "Load Template",
            MainMenuItem::ExportData => "Export Data",
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
                    let resp = if matches!(item, MainMenuItem::StartStop) && !enabled {
                        resp.on_disabled_hover_text(
                            "Connect an ESP32 running esp-csi-cli-rs firmware first.",
                        )
                    } else {
                        resp
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
