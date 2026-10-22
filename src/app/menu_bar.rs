//! Top menu bar.

use super::App;
use crate::layout::{LayoutPreset, SplitDirection};
use crate::theme::ThemeVariant;
#[cfg(feature = "rerun")]
use crate::{rerun_sink::Transport, ui::text_input::TextPromptPurpose};

impl App {
    pub(super) fn render_menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Import capture…").clicked() {
                    ui.close();
                    self.run_import_flow();
                }
                if ui.button("Export captures…").clicked() {
                    ui.close();
                    self.prompt_export();
                }
                if ui.button("Open data folder").clicked() {
                    ui.close();
                    let dir = crate::paths::exports_dir();
                    let _ = std::fs::create_dir_all(&dir);
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab(format!("file://{}", dir.display())));
                }
                ui.separator();
                if ui.button("Save template…").clicked() {
                    ui.close();
                    self.prompt_save_template();
                }
                if ui.button("Load template…").clicked() {
                    ui.close();
                    self.open_template_picker();
                }
                ui.separator();
                if ui.button("Preprocessing…").clicked() {
                    ui.close();
                    self.open_preprocess();
                }
                ui.separator();
                if ui.button("Quit   Q").clicked() {
                    ui.close();
                    self.quit_confirm_open = true;
                }
            });

            ui.menu_button("Devices", |ui| {
                let free: Vec<(String, String)> = self
                    .devices
                    .free_ports()
                    .into_iter()
                    .map(|p| (p.name.clone(), p.describe()))
                    .collect();
                if ui.button("Attach all ESP32 ports").clicked() {
                    ui.close();
                    let n = self.devices.attach_all();
                    self.toasts.info(format!(
                        "attached {n} port{}",
                        if n == 1 { "" } else { "s" }
                    ));
                }
                ui.menu_button("Add port", |ui| {
                    if free.is_empty() {
                        ui.label(egui::RichText::new("no free ports").weak());
                    }
                    for (name, desc) in &free {
                        if ui.button(desc).on_hover_text(name).clicked() {
                            ui.close();
                            let i = self.devices.add(name);
                            self.devices.active = Some(i);
                        }
                    }
                    ui.separator();
                    if ui.button("Rescan").clicked() {
                        self.devices.rescan();
                    }
                });
                if ui.button("Add simulated device").clicked() {
                    ui.close();
                    self.add_simulated();
                }
                ui.checkbox(&mut self.devices.auto_attach, "Auto-attach new boards");
                ui.separator();
                let running = self.devices.any_collecting();
                if ui
                    .button(if running {
                        "■ Stop all   S"
                    } else {
                        "▶ Start all   S"
                    })
                    .clicked()
                {
                    ui.close();
                    self.toggle_run_all();
                }
                if ui.button("Apply config to all").clicked() {
                    ui.close();
                    let targets = self.devices.targets();
                    let n = self.devices.apply_all(&targets);
                    self.notify(n);
                }
                ui.checkbox(
                    &mut self.devices.record_on_capture,
                    "Record while collecting",
                );
            });

            ui.menu_button("View", |ui| {
                if ui.button("Command palette…   Ctrl+K").clicked() {
                    ui.close();
                    self.palette.open();
                }
                ui.separator();
                ui.menu_button("Layout", |ui| {
                    for p in LayoutPreset::ALL {
                        if ui.button(format!("{}  {}", p.glyph(), p.label())).clicked() {
                            ui.close();
                            self.tiling.apply_preset(p);
                        }
                    }
                });
                ui.menu_button("Theme", |ui| {
                    for v in ThemeVariant::ALL {
                        if ui
                            .selectable_label(self.theme_variant == v, v.label())
                            .clicked()
                        {
                            ui.close();
                            let ctx = ui.ctx().clone();
                            self.set_theme(&ctx, v);
                        }
                    }
                });
                if ui.button("Change focused view…   Enter").clicked() {
                    ui.close();
                    self.view_selector.open = true;
                    self.view_selector.selected = 0;
                }
                ui.separator();
                if ui.button("Split columns   Shift+→").clicked() {
                    ui.close();
                    self.tiling.split(SplitDirection::Horizontal);
                }
                if ui.button("Split rows   Shift+↓").clicked() {
                    ui.close();
                    self.tiling.split(SplitDirection::Vertical);
                }
                if ui.button("Fullscreen focused   Space").clicked() {
                    ui.close();
                    self.tiling.toggle_fullscreen();
                }
                if ui.button("Close focused   Del").clicked() {
                    ui.close();
                    self.tiling.close_focused();
                }
                ui.separator();
                ui.checkbox(&mut self.esp_panel.open, "Device panel   D");
            });

            #[cfg(feature = "rerun")]
            ui.menu_button("Rerun", |ui| {
                ui.label(
                    egui::RichText::new(format!("transport: {}", self.rerun.transport().label()))
                        .weak()
                        .small(),
                );
                ui.separator();
                if ui
                    .selectable_label(matches!(self.rerun.transport(), Transport::Off), "Off")
                    .clicked()
                {
                    self.rerun.set_transport(Transport::Off);
                    ui.close();
                }
                if ui
                    .selectable_label(
                        matches!(self.rerun.transport(), Transport::Spawn),
                        "Spawn viewer",
                    )
                    .clicked()
                {
                    self.rerun.set_transport(Transport::Spawn);
                    ui.close();
                }
                if ui.button("Connect to viewer…").clicked() {
                    ui.close();
                    self.prompt_rerun(TextPromptPurpose::RerunConnect);
                }
                if ui.button("Record to file…").clicked() {
                    ui.close();
                    self.prompt_rerun(TextPromptPurpose::RerunFile);
                }
                ui.separator();
                let mut live = self.rerun.is_live();
                if ui.checkbox(&mut live, "Live streaming   Shift+R").clicked() {
                    self.rerun.toggle_live();
                }
                let mut rec = self.rerun.is_recording();
                if ui.checkbox(&mut rec, "RRD recording   Shift+L").clicked() {
                    self.rerun.toggle_rrd();
                }
                ui.separator();
                if ui.button("Export history to RRD…").clicked() {
                    ui.close();
                    self.prompt_rerun(TextPromptPurpose::RerunExport);
                }
                if let Some(err) = self.rerun.last_error() {
                    ui.separator();
                    ui.colored_label(egui::Color32::from_rgb(220, 80, 80), err);
                }
            });

            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard shortcuts   H").clicked() {
                    ui.close();
                    self.help.open = true;
                }
                if ui.button("Command palette   Ctrl+K").clicked() {
                    ui.close();
                    self.palette.open();
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "csi-gui-rs {} · firmware protocol {}–{}",
                        env!("CARGO_PKG_VERSION"),
                        crate::protocol::info::MIN_SUPPORTED_PROTOCOL,
                        crate::protocol::info::SUPPORTED_PROTOCOL
                    ))
                    .weak()
                    .small(),
                );
                ui.hyperlink_to("Project page", env!("CARGO_PKG_REPOSITORY"));
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let arrow = if self.esp_panel.open { "»" } else { "«" };
                if ui
                    .button(format!("{arrow} Devices"))
                    .on_hover_text("Toggle the device panel (D)")
                    .clicked()
                {
                    self.esp_panel.open = !self.esp_panel.open;
                }
                ui.separator();
                let running = self.devices.any_collecting();
                let (label, fill) = if running {
                    ("■ Stop all", egui::Color32::from_rgb(170, 60, 60))
                } else {
                    ("▶ Start all", egui::Color32::from_rgb(40, 130, 80))
                };
                let enabled = running || self.devices.sessions.iter().any(|s| s.is_ready());
                if ui
                    .add_enabled(
                        enabled,
                        egui::Button::new(
                            egui::RichText::new(label)
                                .color(egui::Color32::WHITE)
                                .strong(),
                        )
                        .fill(fill),
                    )
                    .on_hover_text("Start / stop every ticked device under one shared session (S)")
                    .on_disabled_hover_text("No device is ready yet")
                    .clicked()
                {
                    self.toggle_run_all();
                }
            });
        });
    }

    /// Add the next free simulated device.
    pub(super) fn add_simulated(&mut self) {
        let n = (0..64)
            .map(|i| format!("{}{i}", crate::sim::SIM_PREFIX))
            .find(|p| !self.devices.has_port(p))
            .unwrap_or_else(|| format!("{}0", crate::sim::SIM_PREFIX));
        let i = self.devices.add(&n);
        self.devices.active = Some(i);
    }
}
