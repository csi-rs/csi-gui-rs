use crate::layout::LayoutPreset;

/// Modal that offers one-click tiling arrangements. Opened from the
/// `View → Layouts…` menu; the chosen preset is applied by
/// [`crate::layout::TilingManager::apply_preset`].
#[derive(Default)]
pub struct LayoutPicker {
    pub open: bool,
    pub selected: usize,
}

impl LayoutPicker {
    pub fn show(&mut self, ctx: &egui::Context) -> Option<LayoutPreset> {
        if !self.open {
            return None;
        }
        let mut chosen = None;
        egui::Window::new("Choose a layout")
            .collapsible(false)
            .resizable(false)
            .default_width(300.0)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Pick a starting arrangement — you can still split, close, \
                         and re-assign each pane afterwards.",
                    )
                    .weak()
                    .small(),
                );
                ui.add_space(6.0);
                for (i, preset) in LayoutPreset::ALL.iter().enumerate() {
                    let selected = i == self.selected;
                    let text = format!("{}   {}", preset.glyph(), preset.label());
                    let text = if selected {
                        egui::RichText::new(format!("▶ {text}")).strong()
                    } else {
                        egui::RichText::new(format!("  {text}"))
                    };
                    if ui
                        .add_sized([ui.available_width(), 28.0], egui::Button::new(text))
                        .clicked()
                    {
                        chosen = Some(*preset);
                        self.open = false;
                    }
                }
                ctx.input(|i| {
                    if i.key_pressed(egui::Key::ArrowDown) {
                        self.selected = (self.selected + 1) % LayoutPreset::ALL.len();
                    }
                    if i.key_pressed(egui::Key::ArrowUp) {
                        self.selected =
                            (self.selected + LayoutPreset::ALL.len() - 1) % LayoutPreset::ALL.len();
                    }
                    if i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space) {
                        chosen = Some(LayoutPreset::ALL[self.selected]);
                        self.open = false;
                    }
                    if i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Q) {
                        self.open = false;
                    }
                });
            });
        chosen
    }
}
