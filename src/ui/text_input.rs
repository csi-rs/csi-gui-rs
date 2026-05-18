use crate::persistence::ExportFormat;

/// Simple modal text-prompt used for "Save template", "Export data",
/// "Import data" and the Rerun-related flows.
///
/// When [`Self::purpose`] is [`TextPromptPurpose::ExportData`] the
/// prompt also renders an explicit Format radio row (CSV / JSONL /
/// Parquet) — the chosen format is appended to the filename before
/// dispatch, so the caller doesn't have to remember an extension.
#[derive(Default)]
pub struct TextPrompt {
    pub open: bool,
    pub title: String,
    pub label: String,
    pub buffer: String,
    pub purpose: TextPromptPurpose,
    /// Currently-selected format for the Export flow. Initialised to
    /// `Csv` whenever the prompt is opened.
    pub export_format: ExportFormat,
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextPromptPurpose {
    #[default]
    None,
    SaveTemplate,
    /// Filename prompt for the Export-Data flow. The format is picked
    /// inline in the prompt; the result variant is
    /// [`TextPromptResult::SubmitExport`].
    ExportData,
    RerunConnect,
    RerunFile,
    RerunExport,
}

pub enum TextPromptResult {
    None,
    /// Free-text submit; payload is whatever the user typed.
    Submit(String, TextPromptPurpose),
    /// Export-flow submit: filename + the format the user picked.
    SubmitExport(String, ExportFormat),
    Cancel,
}

impl TextPrompt {
    pub fn show(&mut self, ctx: &egui::Context) -> TextPromptResult {
        if !self.open {
            return TextPromptResult::None;
        }
        let mut result = TextPromptResult::None;
        let mut still_open = true;
        let is_export = matches!(self.purpose, TextPromptPurpose::ExportData);
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
                if is_export {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Format").weak().small());
                    ui.horizontal(|ui| {
                        for (variant, label) in [
                            (ExportFormat::Csv, "CSV"),
                            (ExportFormat::Json, "JSONL"),
                            (ExportFormat::Parquet, "Parquet"),
                        ] {
                            if ui
                                .selectable_label(self.export_format == variant, label)
                                .clicked()
                            {
                                self.export_format = variant;
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        result = self.submit_result();
                        still_open = false;
                    }
                    if ui.button("Cancel").clicked() {
                        result = TextPromptResult::Cancel;
                        still_open = false;
                    }
                });
                ctx.input(|i| {
                    if i.key_pressed(egui::Key::Enter) && !self.buffer.is_empty() {
                        result = self.submit_result();
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
            self.export_format = ExportFormat::Csv;
        }
        result
    }

    fn submit_result(&self) -> TextPromptResult {
        if matches!(self.purpose, TextPromptPurpose::ExportData) {
            TextPromptResult::SubmitExport(self.buffer.clone(), self.export_format)
        } else {
            TextPromptResult::Submit(self.buffer.clone(), self.purpose)
        }
    }
}
