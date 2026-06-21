#[derive(Default)]
pub struct HelpOverlay {
    pub open: bool,
}


impl HelpOverlay {
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut close = false;
        egui::Window::new("Help")
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("Keyboard").strong());
                for (k, v) in KEYS {
                    ui.horizontal(|ui| {
                        ui.add_sized([130.0, 18.0], egui::Label::new(*k));
                        ui.label(*v);
                    });
                }
                #[cfg(feature = "rerun")]
                for (k, v) in RERUN_KEYS {
                    ui.horizontal(|ui| {
                        ui.add_sized([130.0, 18.0], egui::Label::new(*k));
                        ui.label(*v);
                    });
                }
                ui.separator();
                ui.label(egui::RichText::new("Replay").strong());
                ui.label(
                    "Add a Replay pane (Enter → Replay) for a scrubber, play/pause, and speed \
                     control. While the replay anchor is set, every pane (Dashboard, Polar, \
                     Phase, Spectrogram, CIR, I/Q density) follows it. Press R or ⏭ Live to \
                     return everything to LIVE.",
                );
                #[cfg(feature = "rerun")]
                {
                    ui.separator();
                    ui.label(egui::RichText::new("Rerun").strong());
                    ui.label(
                        "Optional out-of-process visualization sink (rerun.io). Enable at \
                         launch via --rerun=spawn|connect[:host:port]|file:PATH, or use the \
                         Rerun menu. Shift+R toggles live streaming, Shift+L toggles RRD file \
                         recording. Export History to RRD replays the entire averaged history \
                         to a .rrd file.",
                    );
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });

                ctx.input(|i| {
                    if i.key_pressed(egui::Key::Escape)
                        || i.key_pressed(egui::Key::H)
                        || i.key_pressed(egui::Key::Q)
                    {
                        close = true;
                    }
                });
            });
        if close {
            self.open = false;
        }
    }
}

const KEYS: &[(&str, &str)] = &[
    ("q", "Quit confirm"),
    ("h", "Toggle this help"),
    ("m", "Main menu"),
    ("t", "Cycle theme"),
    ("Tab", "Focus next pane"),
    ("Delete", "Close focused pane"),
    ("Space", "Toggle fullscreen on focused pane"),
    ("Enter", "Pick a view for the focused pane"),
    ("r", "Return all panes to LIVE (cancel replay)"),
    ("1..9, 0", "Focus pane by id"),
    ("←/→", "Step replay anchor by one packet"),
    ("Shift+←/→", "Split focused pane horizontally"),
    ("Shift+↑/↓", "Split focused pane vertically"),
];

#[cfg(feature = "rerun")]
const RERUN_KEYS: &[(&str, &str)] = &[
    ("Shift+R", "Toggle Rerun live streaming"),
    ("Shift+L", "Toggle Rerun RRD recording"),
];
