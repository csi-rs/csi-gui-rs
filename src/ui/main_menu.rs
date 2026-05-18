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
    pub fn show(&mut self, ctx: &egui::Context, is_collecting: bool) -> Option<MainMenuItem> {
        if !self.open {
            return None;
        }
        let mut chosen = None;
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
                    if ui.button(text).clicked() {
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
                        chosen = Some(MainMenuItem::ALL[self.selected]);
                        self.open = false;
                    }
                    if i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::M) {
                        self.open = false;
                    }
                });
            });
        chosen
    }
}
