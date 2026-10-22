//! Bottom status bar: one pill per device, then global state.

use super::App;
use crate::views::devices::state_badge;
use std::sync::atomic::Ordering;

impl App {
    pub(super) fn render_status_bar(&mut self, ui: &mut egui::Ui) {
        let mut select = None;
        ui.horizontal_wrapped(|ui| {
            if self.devices.sessions.is_empty() {
                ui.label(egui::RichText::new("no devices").weak());
                if ui.small_button("attach all").clicked() {
                    self.devices.attach_all();
                }
                if ui.small_button("simulate one").clicked() {
                    self.add_simulated();
                }
            }
            for (i, s) in self.devices.sessions.iter().enumerate() {
                let active = self.devices.active == Some(i);
                let (col, state) = state_badge(&s.conn_state, s.is_collecting());
                let pps = s.history.last().map(|t| t.pps).unwrap_or(0);
                let mut text = egui::text::LayoutJob::default();
                let font = egui::TextFormat {
                    color: ui.visuals().text_color(),
                    ..Default::default()
                };
                text.append(
                    "● ",
                    0.0,
                    egui::TextFormat {
                        color: s.color,
                        ..font.clone()
                    },
                );
                text.append(&s.label(), 0.0, font.clone());
                text.append(
                    &format!("  {state}"),
                    0.0,
                    egui::TextFormat {
                        color: col,
                        ..font.clone()
                    },
                );
                if s.is_collecting() {
                    text.append(&format!("  {pps} pps"), 0.0, font.clone());
                }
                if s.recorder.is_recording() {
                    text.append(
                        "  ●",
                        0.0,
                        egui::TextFormat {
                            color: egui::Color32::from_rgb(230, 70, 70),
                            ..font.clone()
                        },
                    );
                }
                let decode = s.shared.decode_rate() * 100.0;
                let hover = format!(
                    "{}\n{}\n{:.0}% decoded · {} lost · {} dropped",
                    s.port,
                    s.info
                        .as_ref()
                        .map(|i| i.summary())
                        .unwrap_or_else(|| "not identified".into()),
                    decode,
                    s.shared.frames_lost.load(Ordering::Relaxed),
                    s.shared.frames_dropped.load(Ordering::Relaxed),
                );
                if ui
                    .selectable_label(active, text)
                    .on_hover_text(hover)
                    .clicked()
                {
                    select = Some(i);
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new("Ctrl+K commands · H help")
                        .weak()
                        .small(),
                );
                #[cfg(feature = "rerun")]
                {
                    ui.separator();
                    let (rcol, rtext) = self.rerun.status_summary();
                    let resp = ui.colored_label(rcol, rtext);
                    if let Some(err) = self.rerun.last_error() {
                        resp.on_hover_text(err);
                    }
                }
                if self.preprocess.is_active() {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!("preprocess: {}", self.preprocess.summary()))
                            .weak()
                            .small(),
                    )
                    .on_hover_text("File → Preprocessing… to edit filters");
                }
                if let Some(anchor) = self.replay.anchor {
                    ui.separator();
                    let resp = ui.add(
                        egui::Button::new(
                            egui::RichText::new("● Live")
                                .color(egui::Color32::WHITE)
                                .strong(),
                        )
                        .fill(egui::Color32::from_rgb(160, 110, 30)),
                    );
                    if resp.on_hover_text("Back to live (R)").clicked() {
                        self.go_live();
                    }
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 180, 60),
                        format!("replay @ {anchor}"),
                    );
                }
            });
        });
        if let Some(i) = select {
            self.devices.active = Some(i);
        }
    }
}
