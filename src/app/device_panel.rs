//! The right-hand device panel: the device list with the multi-device
//! actions on top, then the active device's details, configuration,
//! pairing and console.

use super::App;
use crate::device::Notice;
use crate::protocol::command::EspCommand;
use crate::reader::ConnState;
use crate::views::devices::state_badge;
use std::sync::atomic::Ordering;

const WARN: egui::Color32 = egui::Color32::from_rgb(220, 180, 60);
const BAD: egui::Color32 = egui::Color32::from_rgb(230, 85, 85);

impl App {
    pub(super) fn render_device_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading("Devices");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.add_menu(ui);
            });
        });
        ui.separator();
        self.device_list(ui);
        ui.separator();

        let Some(ai) = self.devices.active.filter(|&i| i < self.devices.sessions.len()) else {
            ui.add_space(8.0);
            ui.label("No device selected.");
            ui.label(
                egui::RichText::new(
                    "Plug in an ESP32 running esp-csi-cli-rs and use + Add, or tick \
                     \"Auto-attach\". No hardware? Add a simulated device.",
                )
                .weak()
                .small(),
            );
            return;
        };

        egui::ScrollArea::vertical()
            .id_salt("device-detail")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                self.problem_card(ui, ai);
                self.device_section(ui, ai);
                if self.devices.sessions[ai].live {
                    self.config_section(ui, ai);
                    self.pairing_section(ui, ai);
                    self.console_section(ui, ai);
                }
            });
    }

    fn add_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("+ Add", |ui| {
            let free: Vec<(String, String, bool)> = self
                .devices
                .free_ports()
                .into_iter()
                .map(|p| (p.name.clone(), p.describe(), p.is_likely_esp()))
                .collect();
            if free.iter().any(|(_, _, esp)| *esp) && ui.button("Attach all ESP32 ports").clicked() {
                ui.close();
                self.devices.attach_all();
            }
            if free.is_empty() {
                ui.label(egui::RichText::new("no free serial ports").weak());
            }
            for (name, desc, esp) in &free {
                let text = if *esp {
                    egui::RichText::new(desc)
                } else {
                    egui::RichText::new(desc).weak()
                };
                if ui.button(text).on_hover_text(name).clicked() {
                    ui.close();
                    let i = self.devices.add(name);
                    self.devices.active = Some(i);
                }
            }
            ui.separator();
            if ui.button("Simulated device").on_hover_text("A virtual esp-csi-cli-rs board, for trying things out").clicked() {
                ui.close();
                self.add_simulated();
            }
            if ui.button("Rescan ports").clicked() {
                self.devices.rescan();
            }
        });
    }

    /// The list, one row per device, with the bulk actions above it.
    fn device_list(&mut self, ui: &mut egui::Ui) {
        let targets = self.devices.targets();
        let n_ready = targets.iter().filter(|&&i| self.devices.sessions[i].is_ready()).count();
        let running = targets.iter().any(|&i| self.devices.sessions[i].is_collecting());
        let ticked = self.devices.sessions.iter().filter(|s| s.selected && s.live).count();
        let scope = if ticked == 0 || ticked == self.devices.sessions.iter().filter(|s| s.live).count() {
            "all".to_string()
        } else {
            format!("{ticked} ticked")
        };

        ui.horizontal_wrapped(|ui| {
            if running {
                if ui.button(format!("■ Stop {scope}")).clicked() {
                    self.devices.stop_all(&targets);
                    self.rerun.mark_session_boundary();
                }
            } else if ui
                .add_enabled(n_ready > 0, egui::Button::new(format!("▶ Start {scope}")))
                .on_hover_text("One shared session id and wall-clock epoch, so the devices' frames line up")
                .on_disabled_hover_text("No ticked device is ready")
                .clicked()
            {
                let (_, n) = self.devices.start_all(&targets);
                self.notify(n);
                self.go_live();
            }
            if ui
                .add_enabled(!running && !targets.is_empty(), egui::Button::new(format!("Apply {scope}")))
                .on_hover_text("Push each device's own config")
                .clicked()
            {
                let n = self.devices.apply_all(&targets);
                self.notify(n);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.devices.auto_attach, "Auto-attach")
                .on_hover_text("Attach every ESP32 port as it appears");
            ui.checkbox(&mut self.devices.record_on_capture, "Record")
                .on_hover_text(format!(
                    "Write every packet to JSONL while collecting, one file per device, in {}",
                    crate::paths::exports_dir().display()
                ));
        });
        ui.add_space(4.0);

        let mut select = None;
        let mut toggle = None;
        for (i, s) in self.devices.sessions.iter_mut().enumerate() {
            let active = self.devices.active == Some(i);
            let frame = egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::symmetric(6, 4))
                .fill(if active {
                    ui.visuals().selection.bg_fill.gamma_multiply(0.35)
                } else {
                    egui::Color32::TRANSPARENT
                });
            let resp = frame
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if s.live {
                            ui.checkbox(&mut s.selected, "")
                                .on_hover_text("Include in Start / Stop / Apply");
                        }
                        ui.label(egui::RichText::new("●").color(s.color).size(15.0));
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(s.label()).strong());
                            let chip = s
                                .info
                                .as_ref()
                                .map(|i| format!("{} · v{}", i.chip.label(), i.version))
                                .unwrap_or_else(|| if s.live { s.port.clone() } else { "imported capture".into() });
                            ui.label(egui::RichText::new(chip).weak().small());
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if s.is_ready() {
                                let (sym, tip) = if s.is_collecting() { ("■", "Stop") } else { ("▶", "Start") };
                                let enabled = s.is_collecting() || s.config_applied();
                                if ui
                                    .add_enabled(enabled, egui::Button::new(sym).small())
                                    .on_hover_text(tip)
                                    .on_disabled_hover_text("Waiting for the config to apply")
                                    .clicked()
                                {
                                    toggle = Some(i);
                                }
                            }
                            let (col, state) = if s.live {
                                state_badge(&s.conn_state, s.is_collecting())
                            } else {
                                (egui::Color32::GRAY, "replay")
                            };
                            ui.colored_label(col, egui::RichText::new(state).small());
                            if s.is_collecting() {
                                let pps = s.history.last().map(|t| t.pps).unwrap_or(0);
                                ui.label(egui::RichText::new(format!("{pps} pps")).small());
                            }
                            if s.recorder.is_recording() {
                                ui.colored_label(BAD, "⏺").on_hover_text("recording");
                            }
                        });
                    });
                })
                .response
                .interact(egui::Sense::click());
            if resp.clicked() {
                select = Some(i);
            }
        }
        if let Some(i) = select {
            self.devices.active = Some(i);
        }
        if let Some(i) = toggle {
            let s = &mut self.devices.sessions[i];
            if s.is_collecting() {
                s.stop();
            } else {
                s.start(None);
                self.go_live();
            }
        }
    }

    /// Firmware / connection trouble on the active device, with the fix.
    fn problem_card(&mut self, ui: &mut egui::Ui, ai: usize) {
        let s = &self.devices.sessions[ai];
        if !s.live || s.problem_dismissed || !s.conn_state.is_problem() {
            return;
        }
        let (title, body): (&str, String) = match &s.conn_state {
            ConnState::UnknownFirmware => (
                "No esp-csi-cli-rs on this port",
                "The board didn't answer `info` or print the ESP-CSI-CLI banner after a reset. \
                 Flash esp-csi-cli-rs 0.10 (see the README), check it's the right port, or retry."
                    .into(),
            ),
            ConnState::Incompatible(why) => ("Incompatible firmware", why.clone()),
            ConnState::Error(e) => ("Can't open the port", format!("{e}\n\nOn Linux, check you're in the dialout group.")),
            _ => return,
        };
        let mut retry = false;
        let mut dismiss = false;
        egui::Frame::group(ui.style())
            .stroke(egui::Stroke::new(1.0_f32, BAD))
            .show(ui, |ui| {
                ui.colored_label(BAD, egui::RichText::new(title).strong());
                ui.label(body);
                ui.horizontal(|ui| {
                    if ui.button("Retry (reset board)").clicked() {
                        retry = true;
                    }
                    if ui.button("Dismiss").clicked() {
                        dismiss = true;
                    }
                });
            });
        let s = &mut self.devices.sessions[ai];
        if retry {
            s.send(EspCommand::Reset);
        }
        if dismiss {
            s.problem_dismissed = true;
        }
    }

    fn device_section(&mut self, ui: &mut egui::Ui, ai: usize) {
        let mut remove = false;
        egui::CollapsingHeader::new("Device")
            .id_salt("sec-device")
            .default_open(true)
            .show(ui, |ui| {
                let s = &mut self.devices.sessions[ai];
                egui::Grid::new("device-facts").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
                    ui.label("Name");
                    ui.add(
                        egui::TextEdit::singleline(&mut s.alias)
                            .hint_text(s.port.rsplit('/').next().unwrap_or(&s.port))
                            .char_limit(32)
                            .desired_width(160.0),
                    )
                    .on_hover_text("A name for this board; remembered by its MAC");
                    ui.end_row();
                    ui.label("Colour");
                    ui.color_edit_button_srgba(&mut s.color);
                    ui.end_row();
                    if s.live {
                        ui.label("Port");
                        ui.label(&s.port);
                        ui.end_row();
                        let (col, state) = state_badge(&s.conn_state, s.is_collecting());
                        ui.label("State");
                        ui.colored_label(col, state);
                        ui.end_row();
                    }
                    if let Some(info) = &s.info {
                        ui.label("Firmware");
                        ui.label(format!("{} {}", info.name, info.version));
                        ui.end_row();
                        ui.label("Chip");
                        ui.label(format!("{} · protocol {}", info.chip.label(), info.protocol));
                        ui.end_row();
                        if let Some(mac) = &info.mac {
                            ui.label("MAC");
                            ui.label(egui::RichText::new(mac).monospace());
                            ui.end_row();
                        }
                        if !info.transport.is_empty() {
                            ui.label("Transport");
                            ui.label(&info.transport);
                            ui.end_row();
                        }
                    }
                    let sid = s.shared.session_id.load(Ordering::Relaxed);
                    if sid != 0 {
                        ui.label("Session");
                        ui.label(egui::RichText::new(format!("{sid:08x}")).monospace());
                        ui.end_row();
                    }
                    if s.live && s.shared.frames_total.load(Ordering::Relaxed) > 0 {
                        ui.label("Frames");
                        ui.label(format!(
                            "{} · {:.0}% decoded · {} lost · {} dropped",
                            s.shared.frames_total.load(Ordering::Relaxed),
                            s.shared.decode_rate() * 100.0,
                            s.shared.frames_lost.load(Ordering::Relaxed),
                            s.shared.frames_dropped.load(Ordering::Relaxed)
                        ));
                        ui.end_row();
                    }
                    if let Some(rec) = s.recorder.last_recording() {
                        ui.label("Last file");
                        ui.label(
                            egui::RichText::new(rec.path.file_name().and_then(|n| n.to_str()).unwrap_or("?"))
                                .small(),
                        )
                        .on_hover_text(rec.path.display().to_string());
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    if s.live {
                        if ui.button("Reset").on_hover_text("Reset the board and re-identify").clicked() {
                            s.send(EspCommand::Reset);
                        }
                        if ui.button("Restart").on_hover_text("Soft reboot via the `restart` command").clicked() {
                            s.send(EspCommand::Restart);
                        }
                    }
                    if ui.button("Remove").on_hover_text("Disconnect and forget this session").clicked() {
                        remove = true;
                    }
                });
            });
        if remove {
            self.devices.remove(ai);
        }
    }

    fn config_section(&mut self, ui: &mut egui::Ui, ai: usize) {
        let mut notices = Vec::new();
        let mut copy_to: Option<Vec<usize>> = None;
        let others: Vec<(usize, String)> = self
            .devices
            .sessions
            .iter()
            .enumerate()
            .filter(|(i, s)| *i != ai && s.live)
            .map(|(i, s)| (i, s.label()))
            .collect();
        egui::CollapsingHeader::new("Configuration")
            .id_salt("sec-config")
            .default_open(true)
            .show(ui, |ui| {
                let s = &mut self.devices.sessions[ai];
                let collecting = s.is_collecting();
                if collecting {
                    ui.colored_label(WARN, "Collecting: the firmware ignores changes until you stop.");
                }
                let chip = s.chip();
                let changed = ui
                    .add_enabled_ui(!collecting, |ui| {
                        crate::ui::esp_panel::config_form(ui, ("cfg", ai), &mut s.cfg, chip)
                    })
                    .inner;
                if changed {
                    s.shared.mark_config_dirty();
                }
                let problem = s.cfg.validate_for(chip).err();
                if let Some(err) = problem.as_ref().or(s.config_error.as_ref()) {
                    ui.colored_label(BAD, err);
                } else if !s.config_applied() && s.is_ready() && !collecting {
                    ui.colored_label(WARN, "Not applied yet.");
                }
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    let can_apply = !collecting && problem.is_none() && s.info.is_some();
                    let apply = egui::Button::new(egui::RichText::new("Apply").strong());
                    if ui
                        .add_enabled(can_apply, apply)
                        .on_disabled_hover_text("Fix the config, stop collecting, and wait for the device")
                        .clicked()
                    {
                        if let Err(e) = s.apply_config() {
                            notices.push(Notice::Error(e));
                        }
                    }
                    if ui.add_enabled(!collecting, egui::Button::new("Defaults")).clicked() {
                        s.cfg = crate::protocol::config::EspConfig::default();
                        s.shared.mark_config_dirty();
                    }
                    if !others.is_empty() {
                        ui.menu_button("Copy to…", |ui| {
                            if ui.button("all other devices").clicked() {
                                copy_to = Some(others.iter().map(|(i, _)| *i).collect());
                                ui.close();
                            }
                            ui.separator();
                            for (i, label) in &others {
                                if ui.button(label).clicked() {
                                    copy_to = Some(vec![*i]);
                                    ui.close();
                                }
                            }
                        })
                        .response
                        .on_hover_text("Copy this config (except the peer MAC) and apply it there");
                    }
                });
            });
        if let Some(to) = copy_to {
            notices.extend(self.devices.copy_config(ai, &to));
            notices.push(Notice::Info(format!("config copied to {} device(s)", to.len())));
        }
        if !notices.is_empty() {
            self.devices.remember(ai);
        }
        self.notify(notices);
    }

    /// ESP-NOW pairing between the active device and another.
    fn pairing_section(&mut self, ui: &mut egui::Ui, ai: usize) {
        let others: Vec<(usize, String)> = self
            .devices
            .sessions
            .iter()
            .enumerate()
            .filter(|(i, s)| *i != ai && s.is_ready())
            .map(|(i, s)| (i, s.label()))
            .collect();
        if others.is_empty() || !self.devices.sessions[ai].is_ready() {
            return;
        }
        let mut pair = None;
        egui::CollapsingHeader::new("ESP-NOW pairing")
            .id_salt("sec-pair")
            .default_open(false)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Make this device the initiator and another the responder, on this \
                         device's channel, each with the other's MAC as its peer.",
                    )
                    .weak()
                    .small(),
                );
                let sel = self.esp_panel.pair_with.filter(|i| others.iter().any(|(j, _)| j == i));
                let sel_label = sel
                    .and_then(|i| others.iter().find(|(j, _)| *j == i))
                    .map(|(_, l)| l.clone())
                    .unwrap_or_else(|| "pick a responder".into());
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("pair-with")
                        .selected_text(sel_label)
                        .show_ui(ui, |ui| {
                            for (i, l) in &others {
                                ui.selectable_value(&mut self.esp_panel.pair_with, Some(*i), l);
                            }
                        });
                    if ui.add_enabled(sel.is_some(), egui::Button::new("Pair")).clicked() {
                        pair = sel;
                    }
                });
            });
        if let Some(r) = pair {
            match self.devices.pair_esp_now(ai, r) {
                Ok(()) => self.toasts.info(format!(
                    "{} ⇄ {} paired over ESP-NOW",
                    self.devices.sessions[ai].label(),
                    self.devices.sessions[r].label()
                )),
                Err(e) => self.toasts.error(format!("pairing failed: {e}")),
            }
        }
    }

    fn console_section(&mut self, ui: &mut egui::Ui, ai: usize) {
        egui::CollapsingHeader::new("Console & commands")
            .id_salt("sec-console")
            .default_open(false)
            .show(ui, |ui| {
                let s = &self.devices.sessions[ai];
                let collecting = s.is_collecting();
                ui.add_enabled_ui(!collecting, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for cmd in ["info", "show-config", "show-stats", "version", "help"] {
                            if ui.small_button(cmd).clicked() {
                                s.send(EspCommand::Raw(cmd.into()));
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut self.esp_panel.raw_command)
                                .hint_text("any CLI command, e.g. set-rate --rate=24m")
                                .desired_width(ui.available_width() - 60.0),
                        );
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if (ui.button("Send").clicked() || enter) && !self.esp_panel.raw_command.trim().is_empty() {
                            s.send(EspCommand::Raw(self.esp_panel.raw_command.trim().to_string()));
                            self.esp_panel.raw_command.clear();
                        }
                    });
                })
                .response
                .on_disabled_hover_text("The firmware only accepts `q` while collecting");
                ui.add_space(4.0);
                let lines = s.console.snapshot(Some(12));
                egui::Frame::NONE
                    .fill(ui.visuals().extreme_bg_color)
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        if lines.is_empty() {
                            ui.label(egui::RichText::new("(no output yet)").weak().italics());
                        }
                        for (_, l) in lines {
                            ui.label(egui::RichText::new(l).monospace().small());
                        }
                    });
                ui.label(
                    egui::RichText::new("Full log: add a Console pane bound to this device.")
                        .weak()
                        .small(),
                );
            });
    }
}
