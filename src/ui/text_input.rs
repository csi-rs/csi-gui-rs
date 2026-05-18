/// Simple modal text-prompt used for "Save template" and "Export data" flows.
#[derive(Default)]
pub struct TextPrompt {
    pub open: bool,
    pub title: String,
    pub label: String,
    pub buffer: String,
    pub purpose: TextPromptPurpose,
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextPromptPurpose {
    #[default]
    None,
    SaveTemplate,
    ExportData,
    RerunConnect,
    RerunFile,
    RerunExport,
}

pub enum TextPromptResult {
    None,
    Submit(String, TextPromptPurpose),
    Cancel,
}

impl TextPrompt {
    pub fn show(&mut self, ctx: &egui::Context) -> TextPromptResult {
        if !self.open {
            return TextPromptResult::None;
        }
        let mut result = TextPromptResult::None;
        let mut still_open = true;
        egui::Window::new(self.title.clone())
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.label(&self.label);
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.buffer)
                        .desired_width(320.0)
                        .hint_text("name"),
                );
                if !resp.has_focus() {
                    resp.request_focus();
                }
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        result = TextPromptResult::Submit(self.buffer.clone(), self.purpose);
                        still_open = false;
                    }
                    if ui.button("Cancel").clicked() {
                        result = TextPromptResult::Cancel;
                        still_open = false;
                    }
                });
                ctx.input(|i| {
                    if i.key_pressed(egui::Key::Enter) && !self.buffer.is_empty() {
                        result = TextPromptResult::Submit(self.buffer.clone(), self.purpose);
                        still_open = false;
                    }
                    if i.key_pressed(egui::Key::Escape) {
                        result = TextPromptResult::Cancel;
                        still_open = false;
                    }
                });
            });
        if !still_open {
            self.open = false;
            self.buffer.clear();
        }
        result
    }
}
