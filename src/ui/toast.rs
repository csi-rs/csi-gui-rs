//! Transient notifications in the bottom-right corner.
//!
//! Routine events (a device came up, a recording was saved) used to open a
//! modal alert that had to be dismissed; with several boards that is a
//! lot of clicking. Toasts stack, fade on their own, and can be clicked
//! away. Errors stay longer.

use crate::device::Notice;
use std::time::{Duration, Instant};

const INFO_TTL: Duration = Duration::from_secs(4);
const ERROR_TTL: Duration = Duration::from_secs(10);
const MAX_SHOWN: usize = 6;

struct Toast {
    notice: Notice,
    born: Instant,
}

/// The toast stack.
#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

impl Toasts {
    /// Queue a notice.
    pub fn push(&mut self, notice: Notice) {
        // Collapse an identical toast still on screen instead of stacking it.
        self.items.retain(|t| t.notice != notice);
        self.items.push(Toast { notice, born: Instant::now() });
        if self.items.len() > 32 {
            self.items.remove(0);
        }
    }

    /// Shorthand for an info toast.
    pub fn info(&mut self, msg: impl Into<String>) {
        self.push(Notice::Info(msg.into()));
    }

    /// Shorthand for an error toast.
    pub fn error(&mut self, msg: impl Into<String>) {
        self.push(Notice::Error(msg.into()));
    }

    /// Draw the stack and drop expired toasts.
    pub fn show(&mut self, ctx: &egui::Context) {
        self.items.retain(|t| {
            let ttl = match t.notice {
                Notice::Error(_) => ERROR_TTL,
                _ => INFO_TTL,
            };
            t.born.elapsed() < ttl
        });
        if self.items.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        let mut dismiss = None;
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -36.0))
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_max_width(380.0);
                let start = self.items.len().saturating_sub(MAX_SHOWN);
                for (i, t) in self.items.iter().enumerate().skip(start) {
                    let (accent, icon, text) = match &t.notice {
                        Notice::Info(s) => (egui::Color32::from_rgb(80, 180, 120), "●", s),
                        Notice::Warn(s) => (egui::Color32::from_rgb(220, 170, 50), "▲", s),
                        Notice::Error(s) => (egui::Color32::from_rgb(225, 85, 85), "×", s),
                    };
                    let frame = egui::Frame::popup(ui.style())
                        .stroke(egui::Stroke::new(1.0_f32, accent))
                        .inner_margin(egui::Margin::symmetric(10, 6));
                    let resp = frame
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.colored_label(accent, icon);
                                ui.label(text.as_str());
                            });
                        })
                        .response
                        .interact(egui::Sense::click())
                        .on_hover_text("click to dismiss");
                    if resp.clicked() {
                        dismiss = Some(i);
                    }
                    ui.add_space(4.0);
                }
            });
        if let Some(i) = dismiss {
            self.items.remove(i);
        }
    }
}
