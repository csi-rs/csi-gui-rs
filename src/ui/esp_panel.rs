use crate::protocol::config::{
    CollectionMode, CsiDelivery, CsiPreset, EspConfig, Ht40, LogMode, PhyRate, WifiMode,
    WifiProtocol, ESP_CONFIG_FIELDS, valid_channel,
};
use crate::shared::EspSharedState;

/// Persistent edit buffers for numeric ESP config fields (see `channel_text`).
pub struct ConfigFieldBuffers {
    pub traffic_hz: String,
    pub he_stbc: String,
    pub he_val_scale_cfg: String,
    pub start_duration: String,
}

impl ConfigFieldBuffers {
    pub fn from_cfg(cfg: &EspConfig) -> Self {
        Self {
            traffic_hz: cfg.traffic_hz.to_string(),
            he_stbc: cfg.he_stbc.to_string(),
            he_val_scale_cfg: cfg.he_val_scale_cfg.to_string(),
            start_duration: cfg
                .start_duration_sec
                .map(|v| v.to_string())
                .unwrap_or_default(),
        }
    }
}

/// UI shell state for the right-hand ESP control panel.
pub struct EspPanel {
    /// Whether the side panel is expanded.
    pub open: bool,
}

impl Default for EspPanel {
    fn default() -> Self {
        Self { open: true }
    }
}

/// Render the scrollable grid of the `set-*` config knobs for one device's
/// `cfg` into the given `ui`.
///
/// Mode-specific fields (STA / AP credentials, ESP-NOW peer/HT40) are greyed
/// out when the selected [`WifiMode`] doesn't use them, so the panel only
/// invites edits that will actually take effect. `channel_buf` and
/// `field_bufs` are the session's persistent edit buffers.
pub fn config_grid(
    ui: &mut egui::Ui,
    cfg: &mut EspConfig,
    channel_buf: &mut String,
    field_bufs: &mut ConfigFieldBuffers,
    shared: &EspSharedState,
) {
    let mode = cfg.wifi_mode;
    egui::Grid::new("esp-config-grid")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for (i, label) in ESP_CONFIG_FIELDS.iter().enumerate() {
                let enabled = field_enabled(i, mode);
                ui.add_enabled_ui(enabled, |ui| {
                    ui.label(*label);
                });
                ui.add_enabled_ui(enabled, |ui| {
                    render_value(ui, i, cfg, channel_buf, field_bufs, shared);
                });
                ui.end_row();
            }
        });
}

/// Commit edit buffers into `cfg` and validate. Returns `Ok(())`
/// when the config is safe to push to the device.
pub fn commit_and_validate(
    cfg: &mut EspConfig,
    channel_buf: &str,
    field_bufs: &mut ConfigFieldBuffers,
) -> Result<(), String> {
    commit_channel(cfg, channel_buf);
    commit_numeric_fields(cfg, field_bufs);
    cfg.validate()
}

/// Whether the config row at `idx` (index into `ESP_CONFIG_FIELDS`) applies
/// to the current [`WifiMode`]. Credentials / peer settings that the mode
/// doesn't use are greyed out. Indices must track `ESP_CONFIG_FIELDS`.
fn field_enabled(idx: usize, mode: WifiMode) -> bool {
    let is_esp_now = matches!(
        mode,
        WifiMode::EspNowCentral
            | WifiMode::EspNowPeripheral
            | WifiMode::EspNowFastCollector
            | WifiMode::EspNowFastSource
    );
    match idx {
        // STA SSID / STA Password
        3 | 4 => mode == WifiMode::Station,
        // AP SSID / AP Password / AP DHCP
        5..=7 => mode == WifiMode::WifiAp,
        // Peer MAC / HT40 (ESP-NOW only)
        8 | 9 => is_esp_now,
        _ => true,
    }
}

fn commit_channel(cfg: &mut EspConfig, buf: &str) {
    if let Ok(v) = buf.trim().parse::<u8>() {
        cfg.channel = v; // `cfg.validate()` surfaces out-of-range values to the user.
    }
}

fn commit_numeric_fields(cfg: &mut EspConfig, bufs: &ConfigFieldBuffers) {
    if let Ok(v) = bufs.traffic_hz.trim().parse::<u64>() {
        cfg.traffic_hz = v;
    }
    if let Ok(v) = bufs.he_stbc.trim().parse::<u32>() {
        if v <= 2 {
            cfg.he_stbc = v;
        }
    }
    if let Ok(v) = bufs.he_val_scale_cfg.trim().parse::<u32>() {
        if v <= 3 {
            cfg.he_val_scale_cfg = v;
        }
    }
    cfg.start_duration_sec = if bufs.start_duration.trim().is_empty() {
        None
    } else {
        bufs.start_duration.trim().parse().ok()
    };
}

fn enum_combo<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut T,
    options: &[(T, &'static str)],
    shared: &EspSharedState,
) {
    let current = options
        .iter()
        .find(|(v, _)| *v == *value)
        .map(|(_, l)| *l)
        .unwrap_or("?");
    egui::ComboBox::from_id_salt(id)
        .selected_text(current)
        .width(140.0)
        .show_ui(ui, |ui| {
            for (v, label) in options {
                if ui.selectable_label(*value == *v, *label).clicked() {
                    *value = *v;
                    shared.mark_config_dirty();
                }
            }
        });
}

fn render_value(
    ui: &mut egui::Ui,
    idx: usize,
    cfg: &mut EspConfig,
    channel_buf: &mut String,
    field_bufs: &mut ConfigFieldBuffers,
    shared: &EspSharedState,
) {
    // Index order MUST match `ESP_CONFIG_FIELDS` in `protocol::config`.
    match idx {
        0 => {
            enum_combo(
                ui,
                "wifi_mode",
                &mut cfg.wifi_mode,
                &WifiMode::ALL.map(|m| (m, m.cli())),
                shared,
            );
        }
        1 => {
            enum_combo(
                ui,
                "protocol",
                &mut cfg.protocol,
                &WifiProtocol::ALL.map(|m| (m, m.cli())),
                shared,
            );
        }
        2 => {
            ui.horizontal(|ui| {
                if ui
                    .add(egui::TextEdit::singleline(channel_buf).desired_width(70.0))
                    .changed()
                {
                    shared.mark_config_dirty();
                    if let Ok(v) = channel_buf.trim().parse::<u8>() {
                        if valid_channel(v) {
                            cfg.channel = v;
                        }
                    }
                }
                ui.label(
                    egui::RichText::new("1-14, 36/40/44/48, 149…165")
                        .weak()
                        .small(),
                );
            });
        }
        3 => text_field(ui, &mut cfg.sta_ssid, 32, false, shared),
        4 => text_field(ui, &mut cfg.sta_password, 32, true, shared),
        5 => text_field(ui, &mut cfg.ap_ssid, 32, false, shared),
        6 => text_field(ui, &mut cfg.ap_password, 32, true, shared),
        7 => toggle(ui, &mut cfg.ap_dhcp, shared),
        8 => {
            let mut s = cfg.peer_mac.clone();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut s)
                        .hint_text("aa:bb:cc:dd:ee:ff (blank = auto)")
                        .desired_width(180.0),
                )
                .changed()
            {
                cfg.peer_mac = s;
                shared.mark_config_dirty();
            }
        }
        9 => {
            enum_combo(
                ui,
                "ht40",
                &mut cfg.ht40,
                &Ht40::ALL.map(|m| (m, m.cli())),
                shared,
            );
        }
        10 => {
            if ui
                .add(
                    egui::TextEdit::singleline(&mut field_bufs.traffic_hz)
                        .desired_width(100.0),
                )
                .changed()
            {
                shared.mark_config_dirty();
                if let Ok(v) = field_bufs.traffic_hz.trim().parse::<u64>() {
                    cfg.traffic_hz = v;
                }
            }
        }
        11 => toggle(ui, &mut cfg.traffic_unsolicited, shared),
        12 => {
            enum_combo(
                ui,
                "collection_mode",
                &mut cfg.collection_mode,
                &CollectionMode::ALL.map(|m| (m, m.cli())),
                shared,
            );
        }
        13 => {
            let selected = format!(
                "{} {}",
                cfg.log_mode.cli(),
                if cfg.log_mode.host_parses() {
                    "(host parses)"
                } else {
                    "(device-only)"
                }
            );
            egui::ComboBox::from_id_salt("log_mode")
                .selected_text(selected)
                .width(180.0)
                .show_ui(ui, |ui| {
                    for mode in LogMode::ALL {
                        let label = format!(
                            "{} {}",
                            mode.cli(),
                            if mode.host_parses() {
                                "(host parses)"
                            } else {
                                "(device-only)"
                            }
                        );
                        if ui
                            .selectable_label(cfg.log_mode == mode, label)
                            .clicked()
                        {
                            cfg.log_mode = mode;
                            shared.mark_config_dirty();
                        }
                    }
                });
        }
        14 => {
            enum_combo(
                ui,
                "phy_rate",
                &mut cfg.phy_rate,
                &PhyRate::ALL.map(|m| (m, m.cli())),
                shared,
            );
        }
        15 => toggle(ui, &mut cfg.tx_task, shared),
        16 => toggle(ui, &mut cfg.rx_task, shared),
        17 => {
            enum_combo(
                ui,
                "csi_delivery",
                &mut cfg.csi_delivery,
                &CsiDelivery::ALL.map(|m| (m, m.cli())),
                shared,
            );
        }
        18 => toggle(ui, &mut cfg.csi_logging, shared),
        19 => toggle(ui, &mut cfg.classic_lltf_en, shared),
        20 => toggle(ui, &mut cfg.classic_htltf_en, shared),
        21 => toggle(ui, &mut cfg.classic_stbc_htltf_en, shared),
        22 => toggle(ui, &mut cfg.classic_ltf_merge_en, shared),
        23 => toggle(ui, &mut cfg.he_csi_en, shared),
        24 => toggle(ui, &mut cfg.he_csi_legacy_en, shared),
        25 => toggle(ui, &mut cfg.he_csi_ht20_en, shared),
        26 => toggle(ui, &mut cfg.he_csi_ht40_en, shared),
        27 => toggle(ui, &mut cfg.he_csi_su_en, shared),
        28 => toggle(ui, &mut cfg.he_csi_mu_en, shared),
        29 => toggle(ui, &mut cfg.he_csi_dcm_en, shared),
        30 => toggle(ui, &mut cfg.he_csi_beamformed_en, shared),
        31 => {
            if ui
                .add(
                    egui::TextEdit::singleline(&mut field_bufs.he_stbc).desired_width(50.0),
                )
                .changed()
            {
                shared.mark_config_dirty();
                if let Ok(v) = field_bufs.he_stbc.trim().parse::<u32>() {
                    if v <= 2 {
                        cfg.he_stbc = v;
                    }
                }
            }
        }
        32 => {
            if ui
                .add(
                    egui::TextEdit::singleline(&mut field_bufs.he_val_scale_cfg)
                        .desired_width(50.0),
                )
                .changed()
            {
                shared.mark_config_dirty();
                if let Ok(v) = field_bufs.he_val_scale_cfg.trim().parse::<u32>() {
                    if v <= 3 {
                        cfg.he_val_scale_cfg = v;
                    }
                }
            }
        }
        33 => {
            enum_combo(
                ui,
                "csi_preset",
                &mut cfg.csi_preset,
                &CsiPreset::ALL.map(|m| (m, m.cli())),
                shared,
            );
        }
        34 => toggle(ui, &mut cfg.csi_dump_ack, shared),
        35 => toggle(ui, &mut cfg.c5_csi_force_lltf, shared),
        36 => toggle(ui, &mut cfg.c5_csi_vht, shared),
        37 => {
            if ui
                .add(
                    egui::TextEdit::singleline(&mut field_bufs.start_duration)
                        .hint_text("empty = unlimited")
                        .desired_width(90.0),
                )
                .changed()
            {
                shared.mark_config_dirty();
                cfg.start_duration_sec = if field_bufs.start_duration.trim().is_empty() {
                    None
                } else {
                    field_bufs.start_duration.trim().parse().ok()
                };
            }
        }
        _ => {
            ui.label("?");
        }
    }
}

/// Length-capped single-line text field (optionally masked).
fn text_field(
    ui: &mut egui::Ui,
    field: &mut String,
    max: usize,
    password: bool,
    shared: &EspSharedState,
) {
    let mut s = field.clone();
    if ui
        .add(
            egui::TextEdit::singleline(&mut s)
                .password(password)
                .desired_width(180.0),
        )
        .changed()
        && s.len() <= max
    {
        *field = s;
        shared.mark_config_dirty();
    }
}

fn toggle(ui: &mut egui::Ui, b: &mut bool, shared: &EspSharedState) {
    let label = if *b { "on" } else { "off" };
    if ui.button(label).clicked() {
        *b = !*b;
        shared.mark_config_dirty();
    }
}
