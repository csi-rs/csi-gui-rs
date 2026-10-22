//! Ctrl+K command palette: type to filter every action, Enter to run it.

/// One runnable entry. `id` is what the caller matches on.
#[derive(Clone, Debug)]
pub struct PaletteEntry {
    /// Caller-defined action id.
    pub id: String,
    /// What the user sees and searches.
    pub label: String,
    /// Shortcut hint shown on the right, if any.
    pub hint: String,
}

impl PaletteEntry {
    /// Build an entry.
    pub fn new(id: impl Into<String>, label: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            hint: hint.into(),
        }
    }
}

/// Palette state.
#[derive(Default)]
pub struct CommandPalette {
    /// Whether it's showing.
    pub open: bool,
    query: String,
    selected: usize,
    focus_pending: bool,
}

/// `true` when every whitespace-separated word of `query` occurs in
/// `label`, ignoring case.
pub fn matches(label: &str, query: &str) -> bool {
    let label = label.to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|w| label.contains(w))
}

impl CommandPalette {
    /// Open with an empty query.
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.focus_pending = true;
    }

    /// Draw over `entries`; returns the id of the chosen entry.
    pub fn show(&mut self, ctx: &egui::Context, entries: &[PaletteEntry]) -> Option<String> {
        if !self.open {
            return None;
        }
        let shown: Vec<&PaletteEntry> = entries
            .iter()
            .filter(|e| matches(&e.label, &self.query))
            .collect();
        self.selected = self.selected.min(shown.len().saturating_sub(1));
        let mut chosen = None;
        let mut close = false;
        egui::Window::new("Command palette")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size([460.0, 0.0])
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 80.0))
            .show(ctx, |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Type a command…  (Esc to close)")
                        .desired_width(f32::INFINITY),
                );
                if self.focus_pending {
                    r.request_focus();
                    self.focus_pending = false;
                }
                if r.changed() {
                    self.selected = 0;
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        if shown.is_empty() {
                            ui.label(egui::RichText::new("no matching command").weak().italics());
                        }
                        for (i, e) in shown.iter().enumerate() {
                            let sel = i == self.selected;
                            let resp = ui
                                .horizontal(|ui| {
                                    let r = ui.selectable_label(sel, &e.label);
                                    if !e.hint.is_empty() {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(&e.hint).weak().small(),
                                                );
                                            },
                                        );
                                    }
                                    r
                                })
                                .inner;
                            if sel {
                                resp.scroll_to_me(None);
                            }
                            if resp.clicked() {
                                chosen = Some(e.id.clone());
                            }
                        }
                    });
                ctx.input(|i| {
                    if i.key_pressed(egui::Key::ArrowDown) && !shown.is_empty() {
                        self.selected = (self.selected + 1) % shown.len();
                    }
                    if i.key_pressed(egui::Key::ArrowUp) && !shown.is_empty() {
                        self.selected = (self.selected + shown.len() - 1) % shown.len();
                    }
                    if i.key_pressed(egui::Key::Enter)
                        && let Some(e) = shown.get(self.selected)
                    {
                        chosen = Some(e.id.clone());
                    }
                    if i.key_pressed(egui::Key::Escape) {
                        close = true;
                    }
                });
            });
        if chosen.is_some() || close {
            self.open = false;
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn word_match_any_order() {
        assert!(matches("Start all devices", "all start"));
        assert!(matches("Export capture…", "exp"));
        assert!(!matches("Stop all devices", "start"));
        assert!(matches("anything", ""));
    }
}
