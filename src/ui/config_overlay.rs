use crate::protocol::config::{
    valid_channel, CollectionMode, CsiDelivery, EspConfig, PhyRate, WifiMode, ESP_CONFIG_FIELDS,
};

pub struct ConfigOverlay {
    pub open: bool,
    pub cfg: EspConfig,
    pub selected: usize,
    pub editing_text: Option<String>,
    pub channel_text: String,
    prev_open: bool,
    pub error: Option<String>,
    pub is_collecting: bool,
}

impl Default for ConfigOverlay {
    fn default() -> Self {
        let cfg = EspConfig::default();
        Self {
            open: false,
            channel_text: cfg.channel.to_string(),
            cfg,
            selected: 0,
            editing_text: None,
            prev_open: false,
            error: None,
            is_collecting: false,
        }
    }
}

pub enum ConfigOverlayResult {
    None,
    Apply(EspConfig),
    Cancel,
}

impl ConfigOverlay {
    pub fn show(&mut self, ctx: &egui::Context) -> ConfigOverlayResult {
        if !self.open {
            self.prev_open = false;
            return ConfigOverlayResult::None;
        }
        // Sync per-field buffers from cfg on each false→true open transition,
        // so external template loads / resets are reflected, but mid-edit
        // intermediate strings aren't clobbered every frame.
        if !self.prev_open {
            self.channel_text = self.cfg.channel.to_string();
            self.prev_open = true;
        }

        let mut result = ConfigOverlayResult::None;

        egui::Window::new("Configure ESP")
            .collapsible(false)
            .resizable(true)
            .default_width(640.0)
            .default_height(520.0)
            .show(ctx, |ui| {
                if self.is_collecting {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 200, 60),
                        "Note: collection is in progress — Stop before applying.",
                    );
                    ui.separator();
                }
                if let Some(err) = &self.error {
                    ui.colored_label(egui::Color32::from_rgb(230, 80, 80), err);
                    ui.separator();
                }

                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        egui::Grid::new("esp-config-grid")
                            .num_columns(2)
                            .spacing([12.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                for (i, label) in ESP_CONFIG_FIELDS.iter().enumerate() {
                                    let selected = i == self.selected;
                                    let label_text = if selected {
                                        egui::RichText::new(format!("▶ {label}")).strong()
                                    } else {
                                        egui::RichText::new(format!("  {label}"))
                                    };
                                    if ui.button(label_text).clicked() {
                                        self.selected = i;
                                    }
                                    render_value(
                                        ui,
                                        i,
                                        &mut self.cfg,
                                        &mut self.editing_text,
                                        &mut self.channel_text,
                                    );
                                    ui.end_row();
                                }
                            });
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(
                        "[↑/↓] move  [Enter] edit/cycle  [s] Save & Apply  [Esc/q] close",
                    );
                    if ui.button("Save & Apply").clicked() {
                        commit_channel(&mut self.cfg, &self.channel_text);
                        match self.cfg.validate() {
                            Ok(()) => {
                                result = ConfigOverlayResult::Apply(self.cfg.clone());
                                self.open = false;
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                    if ui.button("Close").clicked() {
                        self.open = false;
                        result = ConfigOverlayResult::Cancel;
                    }
                });

                // Keyboard navigation
                ctx.input(|i| {
                    if i.key_pressed(egui::Key::ArrowDown) {
                        self.selected = (self.selected + 1) % ESP_CONFIG_FIELDS.len();
                    }
                    if i.key_pressed(egui::Key::ArrowUp) {
                        self.selected = (self.selected + ESP_CONFIG_FIELDS.len() - 1)
                            % ESP_CONFIG_FIELDS.len();
                    }
                    if i.key_pressed(egui::Key::S) && self.editing_text.is_none() {
                        commit_channel(&mut self.cfg, &self.channel_text);
                        match self.cfg.validate() {
                            Ok(()) => {
                                result = ConfigOverlayResult::Apply(self.cfg.clone());
                                self.open = false;
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                    if i.key_pressed(egui::Key::Escape)
                        || (i.key_pressed(egui::Key::Q) && self.editing_text.is_none())
                    {
                        self.open = false;
                        result = ConfigOverlayResult::Cancel;
                    }
                });
            });

        result
    }
}

fn commit_channel(cfg: &mut EspConfig, buf: &str) {
    if let Ok(v) = buf.trim().parse::<u8>() {
        cfg.channel = v; // `cfg.validate()` surfaces out-of-range values to the user.
    }
}

fn render_value(
    ui: &mut egui::Ui,
    idx: usize,
    cfg: &mut EspConfig,
    edit: &mut Option<String>,
    channel_buf: &mut String,
) {
    match idx {
        0 => {
            if ui.button(cfg.wifi_mode.cli()).clicked() {
                cfg.wifi_mode = cfg.wifi_mode.cycle();
            }
        }
        1 => {
            // Persistent buffer so intermediate strings like "15" (when the
            // user is reaching for 157) aren't discarded between frames.
            ui.horizontal(|ui| {
                if ui
                    .add(egui::TextEdit::singleline(channel_buf).desired_width(80.0))
                    .changed()
                {
                    if let Ok(v) = channel_buf.trim().parse::<u8>() {
                        if valid_channel(v) {
                            cfg.channel = v;
                        }
                    }
                }
                ui.label(
                    egui::RichText::new("1-14, 36/40/44/48, 149/153/157/161/165")
                        .weak()
                        .small(),
                );
            });
            let _ = edit;
        }
        2 => {
            let mut s = cfg.sta_ssid.clone();
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(240.0)).changed() {
                if s.len() <= 32 {
                    cfg.sta_ssid = s;
                }
            }
        }
        3 => {
            let mut s = cfg.sta_password.clone();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut s)
                        .password(true)
                        .desired_width(240.0),
                )
                .changed()
                && s.len() <= 32
            {
                cfg.sta_password = s;
            }
        }
        4 => {
            let mut s = cfg.traffic_hz.to_string();
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(120.0)).changed() {
                if let Ok(v) = s.parse::<u64>() {
                    cfg.traffic_hz = v;
                }
            }
        }
        5 => {
            if ui.button(cfg.collection_mode.cli()).clicked() {
                cfg.collection_mode = cfg.collection_mode.cycle();
            }
        }
        6 => {
            let suffix = if cfg.log_mode.host_parses() {
                "(host parses)"
            } else {
                "(device-only)"
            };
            if ui
                .button(format!("{} {suffix}", cfg.log_mode.cli()))
                .clicked()
            {
                cfg.log_mode = cfg.log_mode.cycle();
            }
        }
        7 => {
            if ui.button(cfg.phy_rate.cli()).clicked() {
                cfg.phy_rate = cfg.phy_rate.cycle();
            }
        }
        8 => toggle(ui, &mut cfg.tx_task),
        9 => toggle(ui, &mut cfg.rx_task),
        10 => {
            if ui.button(cfg.csi_delivery.cli()).clicked() {
                cfg.csi_delivery = cfg.csi_delivery.cycle();
            }
        }
        11 => toggle(ui, &mut cfg.csi_logging),
        12 => toggle(ui, &mut cfg.classic_disable_lltf),
        13 => toggle(ui, &mut cfg.classic_disable_htltf),
        14 => toggle(ui, &mut cfg.classic_disable_stbc_htltf),
        15 => toggle(ui, &mut cfg.classic_disable_ltf_merge),
        16 => toggle(ui, &mut cfg.he_disable_csi),
        17 => toggle(ui, &mut cfg.he_disable_csi_legacy),
        18 => toggle(ui, &mut cfg.he_disable_csi_ht20),
        19 => toggle(ui, &mut cfg.he_disable_csi_ht40),
        20 => toggle(ui, &mut cfg.he_disable_csi_su),
        21 => toggle(ui, &mut cfg.he_disable_csi_mu),
        22 => toggle(ui, &mut cfg.he_disable_csi_dcm),
        23 => toggle(ui, &mut cfg.he_disable_csi_beamformed),
        24 => {
            let mut s = cfg.he_stbc.to_string();
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(60.0)).changed() {
                if let Ok(v) = s.parse::<u32>() {
                    if v <= 2 {
                        cfg.he_stbc = v;
                    }
                }
            }
        }
        25 => {
            let mut s = cfg.he_val_scale_cfg.to_string();
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(60.0)).changed() {
                if let Ok(v) = s.parse::<u32>() {
                    if v <= 3 {
                        cfg.he_val_scale_cfg = v;
                    }
                }
            }
        }
        26 => {
            let mut s = cfg
                .start_duration_sec
                .map(|v| v.to_string())
                .unwrap_or_default();
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(100.0)).changed() {
                cfg.start_duration_sec = s.parse::<u64>().ok();
            }
        }
        _ => {
            ui.label("?");
        }
    }
    let _ = WifiMode::Sniffer;
    let _ = CollectionMode::Collector;
    let _ = CsiDelivery::Off;
    let _ = PhyRate::R1m;
}

fn toggle(ui: &mut egui::Ui, b: &mut bool) {
    let label = if *b { "on" } else { "off" };
    if ui.button(label).clicked() {
        *b = !*b;
    }
}
