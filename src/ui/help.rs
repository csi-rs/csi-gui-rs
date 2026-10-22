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
                ui.label(egui::RichText::new("Devices").strong());
                ui.label(
                    "Every board in the Devices panel (D) captures at the same time. Add boards \
                     with + Add or Auto-attach; click one to configure it. Tick boards to choose \
                     which ones Start / Stop / Apply act on (none ticked = all). Start all (S) \
                     gives them one shared session id so their frames line up. Boards are \
                     remembered by MAC: alias, colour and config come back when replugged.",
                );
                ui.separator();
                ui.label(egui::RichText::new("Panes").strong());
                ui.label(
                    "Each pane has a view picker, a device picker (↻ = follow the selected \
                     device, ● = pinned to one) and a ☰ menu to split, fullscreen or close it. \
                     The Devices view lists every board; Evolution → Compare devices overlays them.",
                );
                ui.separator();
                ui.label(egui::RichText::new("Replay & recording").strong());
                ui.label(
                    "Add a Replay pane to scrub the in-memory history; ← / → step, R returns to \
                     live. Tick Record to write every packet of every collecting board to JSONL \
                     in the data folder (File → Open data folder). Import opens a capture as a \
                     read-only replay device.",
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
                        || i.key_pressed(egui::Key::F1)
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
    ("Ctrl+K", "Command palette: every action, searchable"),
    ("s", "Start all / stop all"),
    ("d", "Toggle the device panel"),
    ("q", "Quit"),
    ("h, F1", "Toggle this help"),
    ("m", "Main menu"),
    ("t", "Cycle theme"),
    ("Tab", "Focus next pane"),
    ("Delete", "Close focused pane"),
    ("Space", "Toggle fullscreen on focused pane"),
    ("Enter", "Pick a view for the focused pane"),
    ("r", "Return all panes to LIVE (cancel replay)"),
    ("1..9, 0", "Focus pane by id"),
    ("←/→", "Step the replay anchor by one tick"),
    ("Shift+←/→", "Split focused pane horizontally"),
    ("Shift+↑/↓", "Split focused pane vertically"),
];

#[cfg(feature = "rerun")]
const RERUN_KEYS: &[(&str, &str)] = &[
    ("Shift+R", "Toggle Rerun live streaming"),
    ("Shift+L", "Toggle Rerun RRD recording"),
];
