use crate::persistence::{list_templates, TemplateEntry};

#[derive(Default)]
pub struct TemplatePicker {
    pub open: bool,
    pub entries: Vec<TemplateEntry>,
    pub selected: usize,
}

pub enum TemplatePickerResult {
    None,
    Load(std::path::PathBuf),
    MakeDefault(String),
    Cancel,
}

impl TemplatePicker {
    pub fn refresh(&mut self) {
        self.entries = list_templates();
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }

    pub fn show(&mut self, ctx: &egui::Context) -> TemplatePickerResult {
        if !self.open {
            return TemplatePickerResult::None;
        }
        let mut result = TemplatePickerResult::None;
        let mut still_open = true;
        egui::Window::new("Load Template")
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                if self.entries.is_empty() {
                    ui.label("No templates saved.");
                }
                for (i, entry) in self.entries.iter().enumerate() {
                    let selected = i == self.selected;
                    let default_marker = if entry.is_default { " ★" } else { "" };
                    let label = format!("{}{}", entry.name, default_marker);
                    let text = if selected {
                        egui::RichText::new(format!("▶ {label}")).strong()
                    } else {
                        egui::RichText::new(format!("  {label}"))
                    };
                    if ui.button(text).clicked() {
                        self.selected = i;
                    }
                }
                ui.separator();
                ui.label("Enter: load   d: mark default   Esc/q: cancel");
                ctx.input(|i| {
                    if i.key_pressed(egui::Key::ArrowDown) && !self.entries.is_empty() {
                        self.selected = (self.selected + 1) % self.entries.len();
                    }
                    if i.key_pressed(egui::Key::ArrowUp) && !self.entries.is_empty() {
                        self.selected =
                            (self.selected + self.entries.len() - 1) % self.entries.len();
                    }
                    if i.key_pressed(egui::Key::Enter) {
                        if let Some(e) = self.entries.get(self.selected) {
                            result = TemplatePickerResult::Load(e.path.clone());
                            still_open = false;
                        }
                    }
                    if i.key_pressed(egui::Key::D) {
                        if let Some(e) = self.entries.get(self.selected) {
                            result = TemplatePickerResult::MakeDefault(e.name.clone());
                        }
                    }
                    if i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Q) {
                        result = TemplatePickerResult::Cancel;
                        still_open = false;
                    }
                });
            });
        if !still_open {
            self.open = false;
        }
        result
    }
}
