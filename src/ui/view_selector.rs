use crate::layout::ViewType;

#[derive(Default)]
pub struct ViewSelector {
    pub open: bool,
    pub selected: usize,
}

impl ViewSelector {
    pub fn show(&mut self, ctx: &egui::Context) -> Option<ViewType> {
        if !self.open {
            return None;
        }
        let mut chosen = None;
        egui::Window::new("Select View")
            .collapsible(false)
            .resizable(false)
            .default_width(280.0)
            .show(ctx, |ui| {
                for (i, v) in ViewType::ALL_SELECTABLE.iter().enumerate() {
                    let selected = i == self.selected;
                    let text = if selected {
                        egui::RichText::new(format!("▶ {}", v.label())).strong()
                    } else {
                        egui::RichText::new(format!("  {}", v.label()))
                    };
                    if ui.button(text).clicked() {
                        chosen = Some(*v);
                        self.open = false;
                    }
                }
                ctx.input(|i| {
                    if i.key_pressed(egui::Key::ArrowDown) {
                        self.selected = (self.selected + 1) % ViewType::ALL_SELECTABLE.len();
                    }
                    if i.key_pressed(egui::Key::ArrowUp) {
                        self.selected =
                            (self.selected + ViewType::ALL_SELECTABLE.len() - 1)
                                % ViewType::ALL_SELECTABLE.len();
                    }
                    if i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space) {
                        chosen = Some(ViewType::ALL_SELECTABLE[self.selected]);
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
