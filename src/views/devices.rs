//! Devices pane: every connected board in one table, live.

use super::{title_bar, ViewCtx};
use crate::reader::ConnState;
use std::sync::atomic::Ordering;

/// Short state text and colour for a connection state.
pub fn state_badge(s: &ConnState, collecting: bool) -> (egui::Color32, &'static str) {
    match s {
        ConnState::Streaming if collecting => (egui::Color32::from_rgb(80, 220, 110), "collecting"),
        ConnState::Streaming => (egui::Color32::from_rgb(120, 190, 255), "ready"),
        ConnState::Opening | ConnState::Identifying => (egui::Color32::from_rgb(220, 200, 60), "connecting"),
        ConnState::Disconnected => (egui::Color32::from_gray(150), "offline"),
        ConnState::UnknownFirmware => (egui::Color32::from_rgb(230, 80, 80), "unknown fw"),
        ConnState::Incompatible(_) => (egui::Color32::from_rgb(230, 80, 80), "incompatible"),
        ConnState::Error(_) => (egui::Color32::from_rgb(230, 80, 80), "error"),
    }
}

/// Render the Devices table.
pub fn render(ui: &mut egui::Ui, ctx: &mut ViewCtx<'_>) {
    title_bar(ui, "Devices", ctx);
    if ctx.devices.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("No devices. Use the Devices panel → Add, or Ctrl+K → \"attach\".")
                    .weak()
                    .italics(),
            );
        });
        return;
    }
    egui::ScrollArea::horizontal().show(ui, |ui| {
        egui::Grid::new("devices-table")
            .striped(true)
            .spacing([14.0, 6.0])
            .show(ui, |ui| {
                for h in [
                    "", "Device", "State", "Chip / firmware", "Mode", "Ch", "pps", "RSSI", "Decoded",
                    "Lost", "Dropped", "Session",
                ] {
                    ui.label(egui::RichText::new(h).strong().small());
                }
                ui.end_row();
                for d in ctx.devices {
                    let collecting = d.shared.is_collecting.load(Ordering::Relaxed);
                    let last = d.history.last();
                    ui.label(egui::RichText::new("●").color(d.color).size(16.0));
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&d.label).strong());
                        ui.label(egui::RichText::new(d.port).weak().small());
                    });
                    let (col, text) = state_badge(d.state, collecting);
                    let mut r = ui.colored_label(col, text);
                    if let ConnState::Incompatible(why) | ConnState::Error(why) = d.state {
                        r = r.on_hover_text(why);
                    }
                    if d.recording {
                        r.on_hover_text("recording");
                    }
                    match d.info {
                        Some(i) => ui.label(format!("{} · v{}", i.chip.label(), i.version)),
                        None if !d.live => ui.label(egui::RichText::new("imported").weak()),
                        None => ui.label(egui::RichText::new("—").weak()),
                    };
                    ui.label(d.cfg.wifi_mode.cli());
                    ui.label(
                        last.and_then(|s| s.csi.as_ref())
                            .map(|c| c.channel.to_string())
                            .unwrap_or_else(|| d.cfg.channel.to_string()),
                    );
                    ui.label(last.map(|s| s.pps.to_string()).unwrap_or_else(|| "0".into()));
                    ui.label(last.map(|s| format!("{} dBm", s.rssi)).unwrap_or_else(|| "—".into()));
                    let pct = d.shared.decode_rate() * 100.0;
                    let pct_col = if pct < 90.0 {
                        egui::Color32::from_rgb(220, 180, 60)
                    } else {
                        ctx.theme.text_normal
                    };
                    ui.colored_label(pct_col, format!("{pct:.0}%"));
                    ui.label(d.shared.frames_lost.load(Ordering::Relaxed).to_string());
                    ui.label(d.shared.frames_dropped.load(Ordering::Relaxed).to_string());
                    let sid = d.shared.session_id.load(Ordering::Relaxed);
                    ui.label(if sid == 0 { "—".to_string() } else { format!("{sid:08x}") });
                    ui.end_row();
                }
            });
    });
}
