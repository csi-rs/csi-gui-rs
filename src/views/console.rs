use crate::views::{title_bar, ViewCtx};

pub fn render(ui: &mut egui::Ui, ctx: &mut ViewCtx<'_>) {
    title_bar(ui, "Console (raw serial)", ctx);

    let mut clear = false;
    ui.horizontal(|ui| {
        ui.label(format!("{} lines", ctx.console.len()));
        if ui.button("Clear").clicked() {
            clear = true;
        }
        ui.checkbox(&mut ctx.view_state.console_autoscroll, "Auto-scroll");
    });
    if clear {
        ctx.console.clear();
    }

    let lines = ctx.console.snapshot(None);
    let autoscroll = ctx.view_state.console_autoscroll;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(autoscroll)
        .show(ui, |ui| {
            for (ms, text) in &lines {
                let secs = *ms as f64 / 1000.0;
                let stamp = format!("{secs:8.3} ");
                let row = format!("{stamp}{text}");
                let color = line_color(text, ctx.theme);
                ui.label(egui::RichText::new(row).monospace().color(color));
            }
            if lines.is_empty() {
                ui.label(
                    egui::RichText::new("(no serial output yet)")
                        .weak()
                        .italics(),
                );
            }
        });
}

fn line_color(text: &str, theme: &crate::theme::Theme) -> egui::Color32 {
    if let Some(stripped) = text.strip_prefix("» ") {
        let _ = stripped;
        // Outgoing command echoes — distinguish from device output.
        egui::Color32::from_rgb(120, 180, 255)
    } else if text.starts_with("ESP-CSI-CLI/") || text == "END-INFO" {
        egui::Color32::from_rgb(120, 220, 130)
    } else {
        theme.text_normal
    }
}
