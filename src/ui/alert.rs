//! Tiny modal alert window for surfacing success/error messages that
//! deserve more attention than the log line they'd otherwise live in.
//!
//! Owned by `App`; opened by calling [`Alert::open_info`] or
//! [`Alert::open_error`]. The window dismisses on its OK button, the
//! Esc / Enter keys, or its close affordance.

use egui::Color32;

/// Visual severity for the alert window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AlertKind {
    /// Information / success — neutral chrome.
    #[default]
    Info,
    /// Error — title rendered in the theme's error red.
    Error,
}

/// Modal-overlay state for the alert window.
#[derive(Default)]
pub struct Alert {
    /// `true` while the alert window is rendered.
    pub open: bool,
    title: String,
    body: String,
    kind: AlertKind,
}

impl Alert {
    /// Pop up an informational message (e.g. "Imported 800 packets").
    pub fn open_info(&mut self, title: impl Into<String>, body: impl Into<String>) {
        self.title = title.into();
        self.body = body.into();
        self.kind = AlertKind::Info;
        self.open = true;
    }

    /// Pop up an error message.
    pub fn open_error(&mut self, title: impl Into<String>, body: impl Into<String>) {
        self.title = title.into();
        self.body = body.into();
        self.kind = AlertKind::Error;
        self.open = true;
    }

    /// Render the alert this frame. Returns whether the window is still
    /// open after handling input — callers can ignore the value; the
    /// `open` field is also kept in sync.
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        if !self.open {
            return false;
        }
        let mut close = false;
        let title_color = match self.kind {
            AlertKind::Info => Color32::from_rgb(110, 200, 255),
            AlertKind::Error => Color32::from_rgb(220, 80, 80),
        };
        egui::Window::new(&self.title)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.colored_label(title_color, egui::RichText::new(&self.title).strong());
                ui.add_space(4.0);
                ui.label(&self.body);
                ui.separator();
                if ui.button("OK").clicked() {
                    close = true;
                }
                ctx.input(|i| {
                    if i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape) {
                        close = true;
                    }
                });
            });
        if close {
            self.open = false;
            self.title.clear();
            self.body.clear();
        }
        self.open
    }
}
