use super::{ViewCtx, title_bar};
use std::sync::atomic::Ordering;

/// Render the Dashboard pane for the pane's device: firmware, link
/// health counters, PPS / SNR / RSSI gauges, and the latest packet.
pub fn render(ui: &mut egui::Ui, ctx: &mut ViewCtx<'_>) {
    title_bar(ui, "Dashboard", ctx);

    let raw_fps = ctx.history.raw_fps_smoothed.round() as i64;
    let decode_pct = (ctx.shared.decode_rate() * 100.0).round() as i32;
    let total = ctx.shared.frames_total.load(Ordering::Relaxed);
    let lost = ctx.shared.frames_lost.load(Ordering::Relaxed);
    let dropped = ctx.shared.frames_dropped.load(Ordering::Relaxed);
    let connected = ctx.shared.connected.load(Ordering::Relaxed);
    let collecting = ctx.shared.is_collecting.load(Ordering::Relaxed);
    let session = ctx.shared.session_id.load(Ordering::Relaxed);

    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Firmware").strong());
            match ctx.device {
                Some(info) => {
                    ui.label(info.summary());
                    if let Some(mac) = &info.mac {
                        ui.label(egui::RichText::new(format!("· {mac}")).weak());
                    }
                }
                None => {
                    ui.colored_label(egui::Color32::from_rgb(220, 80, 80), "not connected");
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            let (color, text) = match (connected, collecting) {
                (false, _) => (egui::Color32::from_rgb(220, 80, 80), "● offline"),
                (true, false) => (egui::Color32::from_rgb(120, 190, 255), "● ready"),
                (true, true) if decode_pct < 80 => (egui::Color32::from_rgb(220, 200, 60), "● collecting"),
                (true, true) => (egui::Color32::from_rgb(80, 220, 110), "● collecting"),
            };
            ui.colored_label(color, text);
            ui.label(format!("{raw_fps} fps raw"));
            ui.label(format!("· {decode_pct}% decoded"));
            ui.label(format!("· {total} frames"));
            if lost > 0 {
                ui.colored_label(egui::Color32::from_rgb(220, 180, 60), format!("· {lost} lost"))
                    .on_hover_text("Gaps in the device's frame counter: lost between radio and host.");
            }
            if dropped > 0 {
                ui.colored_label(egui::Color32::from_rgb(220, 180, 60), format!("· {dropped} dropped"))
                    .on_hover_text("Decoded but dropped because the UI fell behind.");
            }
            if session != 0 {
                ui.label(egui::RichText::new(format!("· session {session:08x}")).weak());
            }
        });
    });

    ui.add_space(6.0);

    let stats = ctx.history.resolve_anchor(ctx.view_anchor());

    let pps = stats.map(|s| s.pps as f32).unwrap_or(0.0);
    gauge(ui, "Packets / s", pps, 0.0, 1000.0, &format!("{pps:.0}"), ctx);

    let snr = stats.map(|s| s.snr as f32).unwrap_or(0.0);
    gauge(ui, "SNR", snr, 0.0, 60.0, &format!("{snr:.0} dB"), ctx);

    let rssi_pct = stats
        .map(|s| ((s.rssi as f32) + 100.0).clamp(0.0, 100.0))
        .unwrap_or(0.0);
    let rssi_label = stats
        .map(|s| format!("{} dBm", s.rssi))
        .unwrap_or_else(|| "—".into());
    gauge(ui, "RSSI", rssi_pct, 0.0, 100.0, &rssi_label, ctx);

    ui.add_space(6.0);
    if let Some(c) = stats.and_then(|s| s.csi.as_ref()) {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Latest packet").weak());
            ui.label(format!("ch {}", c.channel));
            if c.bandwidth_mhz > 0 {
                ui.label(format!("· {} MHz", c.bandwidth_mhz));
            }
            if !c.ppdu.is_empty() {
                ui.label(format!("· {}", c.ppdu));
            }
            ui.label(format!("· {} subcarriers", c.subcarriers()));
            if !c.mac.is_empty() {
                ui.label(format!("· from {}", c.mac));
            }
        });
    }
}

fn gauge(
    ui: &mut egui::Ui,
    label: &str,
    value: f32,
    min: f32,
    max: f32,
    text: &str,
    ctx: &ViewCtx<'_>,
) {
    ui.horizontal(|ui| {
        ui.add_sized([90.0, 20.0], egui::Label::new(label));
        let pct = ((value - min) / (max - min)).clamp(0.0, 1.0);
        let bar = egui::ProgressBar::new(pct)
            .fill(ctx.device_color.gamma_multiply(0.85))
            .text(text.to_string())
            .desired_width(f32::INFINITY);
        ui.add(bar);
    });
}
