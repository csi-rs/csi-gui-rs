use super::ViewCtx;
use std::time::Instant;

/// Replay control surface. The actual replay anchor lives on `App::replay`
/// (see `ReplayState`) so all panes follow it. This view just renders the
/// scrubber and play controls.
pub fn render(ui: &mut egui::Ui, ctx: &mut ViewCtx<'_>) {
    ui.heading("Replay");
    ui.separator();

    let Some(first) = ctx.history.first_id() else {
        ui.label("No packets captured yet. Start collection to enable replay.");
        return;
    };
    let last = ctx.history.last_id().unwrap_or(first);
    if last <= first {
        ui.label("Need at least two packets in history to scrub.");
        return;
    }

    // Current position — None means LIVE, anchored value means a specific id.
    let mut anchor = ctx.replay.anchor.unwrap_or(last);
    if anchor < first {
        anchor = first;
    }
    if anchor > last {
        anchor = last;
    }

    let pkt_count = ctx.history.entries.len();
    let stats_at_anchor = ctx.history.get(anchor);

    ui.horizontal(|ui| {
        let live = ctx.replay.anchor.is_none();
        if live {
            ui.colored_label(
                egui::Color32::from_rgb(80, 200, 110),
                egui::RichText::new("● LIVE").size(16.0).strong(),
            );
        } else {
            ui.colored_label(
                egui::Color32::from_rgb(220, 180, 60),
                egui::RichText::new(format!("▶ REPLAY  packet #{anchor}"))
                    .size(16.0)
                    .strong(),
            );
        }
        ui.label(format!("({pkt_count} captured)"));
    });

    ui.add_space(8.0);

    // Scrubber.
    let mut new_anchor = anchor;
    let slider = egui::Slider::new(&mut new_anchor, first..=last)
        .show_value(true)
        .text("packet id");
    let resp = ui.add_sized([ui.available_width(), 28.0], slider);
    if resp.changed() {
        ctx.replay.anchor = Some(new_anchor);
        ctx.replay.playing = false;
    }

    ui.add_space(4.0);

    ui.horizontal(|ui| {
        // Play / pause
        let play_label = if ctx.replay.playing {
            "❚❚ Pause"
        } else {
            "▶ Play"
        };
        if ui.button(play_label).clicked() {
            if ctx.replay.anchor.is_none() {
                ctx.replay.anchor = Some(first);
            }
            ctx.replay.playing = !ctx.replay.playing;
            ctx.replay.last_advance = Instant::now();
        }

        // Step buttons
        if ui.button("« Step").clicked() {
            ctx.replay.playing = false;
            let cur = ctx.replay.anchor.unwrap_or(last);
            if cur > first {
                ctx.replay.anchor = Some(cur - 1);
            }
        }
        if ui.button("Step »").clicked() {
            ctx.replay.playing = false;
            let cur = ctx.replay.anchor.unwrap_or(first);
            if cur < last {
                ctx.replay.anchor = Some(cur + 1);
            } else {
                ctx.replay.anchor = None; // reached the end → snap to live
            }
        }
        if ui.button("⏮ Start").clicked() {
            ctx.replay.playing = false;
            ctx.replay.anchor = Some(first);
        }
        if ui.button("● Live").clicked() {
            ctx.replay.playing = false;
            ctx.replay.anchor = None;
        }
    });

    ui.add_space(4.0);

    // Speed control.
    ui.horizontal(|ui| {
        ui.label("Speed:");
        ui.add(egui::Slider::new(&mut ctx.replay.speed, 0.1..=20.0).text("× live"));
        if ui.button("1×").clicked() {
            ctx.replay.speed = 1.0;
        }
        if ui.button("4×").clicked() {
            ctx.replay.speed = 4.0;
        }
    });

    ui.add_space(8.0);
    ui.separator();

    // Show what's at the anchor.
    if let Some(s) = stats_at_anchor {
        egui::Grid::new("replay_anchor_grid")
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                ui.label("Packet ID:");
                ui.label(s.id.to_string());
                ui.end_row();

                ui.label("Timestamp:");
                ui.label(format!("{} ms", s.timestamp));
                ui.end_row();

                ui.label("MAC:");
                let mac = s.csi.as_ref().map(|c| c.mac.as_str()).unwrap_or("—");
                ui.label(mac);
                ui.end_row();

                ui.label("RSSI:");
                ui.label(format!("{} dBm", s.rssi));
                ui.end_row();

                ui.label("SNR:");
                ui.label(format!("{} dB", s.snr));
                ui.end_row();

                ui.label("Subcarriers:");
                ui.label(
                    s.csi
                        .as_ref()
                        .map(|c| c.subcarriers().to_string())
                        .unwrap_or_else(|| "—".into()),
                );
                ui.end_row();
            });
    }

    if pkt_count >= ctx.history.capacity() {
        ui.add_space(8.0);
        ui.colored_label(
            egui::Color32::from_rgb(220, 180, 60),
            format!(
                "History is at capacity ({pkt_count}). Older packets are being dropped. \
                 Pass --history-size N to increase."
            ),
        );
    }
}
