use super::{title_bar, ViewCtx};
use std::sync::atomic::Ordering;

pub fn render(ui: &mut egui::Ui, ctx: &mut ViewCtx<'_>) {
    title_bar(ui, "Dashboard", ctx);

    let raw_fps = ctx.history.raw_fps_smoothed.round() as i64;
    let decode_pct = (ctx.shared.decode_rate() * 100.0).round() as i32;
    let total = ctx.shared.frames_total.load(Ordering::Relaxed);
    let connected = ctx.shared.connected.load(Ordering::Relaxed);

    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Firmware").strong());
            match ctx.device {
                Some(info) => {
                    ui.label(format!("{} v{}", info.name, info.version));
                    ui.label(format!("· {}", info.chip.label()));
                    ui.label(format!("· proto {}", info.protocol));
                }
                None => {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 80, 80),
                        "not connected",
                    );
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            let color = if !connected {
                egui::Color32::from_rgb(220, 80, 80)
            } else if decode_pct < 80 {
                egui::Color32::from_rgb(220, 200, 60)
            } else {
                egui::Color32::from_rgb(80, 220, 110)
            };
            ui.colored_label(color, if connected { "● streaming" } else { "● offline" });
            ui.label(format!("{raw_fps} fps raw"));
            ui.label(format!("{decode_pct}% decoded"));
            ui.label(format!("{total} total"));
        });
    });

    ui.add_space(6.0);

    let live = ctx.history.last();
    let stats = match ctx.view_anchor() {
        Some(id) => ctx.history.get(id),
        None => live,
    };

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
    let (ts, mac) = match stats {
        Some(s) => (
            s.timestamp,
            s.csi.as_ref().map(|c| c.mac.clone()).unwrap_or_default(),
        ),
        None => (0, String::new()),
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Latest packet").weak());
        ui.label(format!("t = {ts} ms"));
        if !mac.is_empty() {
            ui.label("·");
            ui.label(format!("MAC {mac}"));
        }
    });
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
            .fill(ctx.theme.gauge_color)
            .text(text.to_string())
            .desired_width(f32::INFINITY);
        ui.add(bar);
    });
}
