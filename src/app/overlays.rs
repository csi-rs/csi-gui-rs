//! Modal overlays (menus, pickers, prompts) and the import / export flows
//! they trigger.

use super::App;
use crate::data::CsiData;
use crate::device::Notice;
use crate::persistence::{self, ExportFormat};
use crate::rerun_sink::{self, Transport};
use crate::ui::{
    main_menu::MainMenuItem,
    preprocess_overlay::PreprocessOverlayResult,
    template_picker::TemplatePickerResult,
    text_input::{TextPromptPurpose, TextPromptResult},
    theme_picker::ThemePickerResult,
};

impl App {
    pub(super) fn handle_overlays(&mut self, ctx: &egui::Context) {
        self.help.show(ctx);
        self.alert.show(ctx);

        if let Some(item) = self.main_menu.show(ctx, false) {
            match item {
                MainMenuItem::Theme => self.open_theme_picker(),
                MainMenuItem::SaveTemplate => self.prompt_save_template(),
                MainMenuItem::LoadTemplate => self.open_template_picker(),
                MainMenuItem::ExportData => self.prompt_export(),
                MainMenuItem::ImportData => self.run_import_flow(),
                MainMenuItem::Preprocess => self.open_preprocess(),
                MainMenuItem::Close => {}
            }
        }

        if let Some(view) = self.view_selector.show(ctx) {
            self.tiling.set_view(view);
        }
        if let Some(preset) = self.layout_picker.show(ctx) {
            self.tiling.apply_preset(preset);
        }
        if let ThemePickerResult::Pick(v) = self.theme_picker.show(ctx) {
            self.set_theme(ctx, v);
        }

        match self.preprocess_overlay.show(ctx) {
            PreprocessOverlayResult::Apply(cfg) => {
                self.preprocess = cfg;
                // New filters must not see packets from before they were on.
                self.devices.clear_preprocess_context();
            }
            PreprocessOverlayResult::Reset => {
                self.preprocess = crate::data::PreprocessConfig::default();
                self.devices.clear_preprocess_context();
            }
            PreprocessOverlayResult::Cancel | PreprocessOverlayResult::None => {}
        }

        match self.text_prompt.show(ctx) {
            TextPromptResult::Submit(name, TextPromptPurpose::SaveTemplate) => {
                match persistence::save_template(name.trim(), &self.workspace()) {
                    Ok(path) => self
                        .toasts
                        .info(format!("template saved: {}", path.display())),
                    Err(e) => self.toasts.error(format!("saving template: {e}")),
                }
            }
            TextPromptResult::SubmitExport(name, fmt) => self.export_data(&name, fmt),
            TextPromptResult::Submit(name, TextPromptPurpose::RerunConnect) => {
                self.rerun.set_transport(parse_host_port(&name));
            }
            TextPromptResult::Submit(name, TextPromptPurpose::RerunFile) => {
                let path = crate::paths::rerun_dir().join(rrd_leaf(&name, "csi"));
                self.rerun.set_transport(Transport::File { path });
            }
            TextPromptResult::Submit(name, TextPromptPurpose::RerunExport) => {
                let path = crate::paths::rerun_dir().join(rrd_leaf(&name, "history"));
                let ids: Vec<String> = self.devices.sessions.iter().map(|s| s.id()).collect();
                let histories: Vec<(&str, &crate::data::History)> = self
                    .devices
                    .sessions
                    .iter()
                    .zip(&ids)
                    .map(|(s, id)| (id.as_str(), &s.history))
                    .collect();
                match rerun_sink::export_history_to_rrd(&histories, &path) {
                    Ok(()) => self.toasts.info(format!("wrote {}", path.display())),
                    Err(e) => self.toasts.error(format!("RRD export failed: {e}")),
                }
            }
            TextPromptResult::Submit(
                _,
                TextPromptPurpose::None | TextPromptPurpose::ExportData,
            )
            | TextPromptResult::Cancel
            | TextPromptResult::None => {}
        }

        match self.template_picker.show(ctx) {
            TemplatePickerResult::Load(tpl) => self.load_workspace(ctx, *tpl),
            TemplatePickerResult::MakeDefault(name) => {
                match persistence::set_default(&name) {
                    Ok(()) => self.toasts.info(format!("\"{name}\" loads at startup")),
                    Err(e) => self.toasts.error(format!("setting default template: {e}")),
                }
                self.template_picker.refresh();
            }
            TemplatePickerResult::Cancel | TemplatePickerResult::None => {}
        }
    }

    pub(super) fn open_theme_picker(&mut self) {
        self.theme_picker.variant = self.theme_variant;
        self.theme_picker.open = true;
    }

    pub(super) fn open_template_picker(&mut self) {
        self.template_picker.refresh();
        self.template_picker.open = true;
    }

    pub(super) fn open_preprocess(&mut self) {
        self.preprocess_overlay.cfg = self.preprocess.clone();
        self.preprocess_overlay.open = true;
    }

    pub(super) fn prompt_save_template(&mut self) {
        self.text_prompt.open = true;
        self.text_prompt.title = "Save template".into();
        self.text_prompt.label = "Template name:".into();
        self.text_prompt.purpose = TextPromptPurpose::SaveTemplate;
    }

    pub(super) fn prompt_export(&mut self) {
        self.text_prompt.open = true;
        self.text_prompt.title = "Export captures".into();
        self.text_prompt.label = format!(
            "File name prefix (one file per device, into {}):",
            crate::paths::exports_dir().display()
        );
        self.text_prompt.buffer = "capture".into();
        self.text_prompt.purpose = TextPromptPurpose::ExportData;
        self.text_prompt.export_format = ExportFormat::Csv;
    }

    #[cfg_attr(not(feature = "rerun"), allow(dead_code))]
    pub(super) fn prompt_rerun(&mut self, purpose: TextPromptPurpose) {
        self.text_prompt.open = true;
        self.text_prompt.purpose = purpose;
        self.text_prompt.buffer.clear();
        match purpose {
            TextPromptPurpose::RerunConnect => {
                self.text_prompt.title = "Connect to Rerun".into();
                self.text_prompt.label = "host:port (default 127.0.0.1:9876):".into();
                self.text_prompt.buffer = "127.0.0.1:9876".into();
            }
            TextPromptPurpose::RerunFile => {
                self.text_prompt.title = "Record to RRD".into();
                self.text_prompt.label =
                    format!("File name (in {}):", crate::paths::rerun_dir().display());
            }
            _ => {
                self.text_prompt.title = "Export history to RRD".into();
                self.text_prompt.label =
                    format!("File name (in {}):", crate::paths::rerun_dir().display());
            }
        }
    }

    /// Write one file per device. The source, best first: the device's last
    /// full recording (every packet), an imported capture, or — when there
    /// is neither — the in-memory history (one averaged packet per tick).
    fn export_data(&mut self, name: &str, fmt: ExportFormat) {
        let prefix = std::path::Path::new(name.trim())
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("capture")
            .to_string();
        let dir = crate::paths::exports_dir();
        let mut written = 0;
        let mut notices = Vec::new();
        for s in &self.devices.sessions {
            let (packets, source): (Vec<CsiData>, &str) =
                if let Some(rec) = s.recorder.last_recording() {
                    match persistence::import_packets(&rec.path) {
                        Ok(p) => (p, "full recording"),
                        Err(e) => {
                            notices.push(Notice::Error(format!(
                                "{}: reading {}: {e}",
                                s.label(),
                                rec.path.display()
                            )));
                            continue;
                        }
                    }
                } else if !s.history.raw_history.is_empty() {
                    (
                        s.history.raw_history.iter().cloned().collect(),
                        "imported capture",
                    )
                } else {
                    let p: Vec<CsiData> = s
                        .history
                        .entries
                        .iter()
                        .filter_map(|e| e.csi.clone())
                        .collect();
                    (p, "history, averaged per tick")
                };
            if packets.is_empty() {
                continue;
            }
            let stem = format!("{prefix}_{}", s.label());
            match persistence::export_packets(&dir, &stem, &packets, fmt) {
                Ok(path) => {
                    written += 1;
                    notices.push(Notice::Info(format!(
                        "{}: {} packets ({source}) → {}",
                        s.label(),
                        packets.len(),
                        path.display()
                    )));
                }
                Err(e) => notices.push(Notice::Error(format!("{}: export failed: {e}", s.label()))),
            }
        }
        if written == 0 && notices.is_empty() {
            notices.push(Notice::Warn(
                "nothing to export yet: capture some data first (tick \"Record while collecting\" for every packet)".into(),
            ));
        }
        self.notify(notices);
    }

    /// Pick a capture file and open it as a read-only replay device.
    pub(super) fn run_import_flow(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import CSI capture")
            .set_directory(crate::paths::exports_dir())
            .add_filter(
                "CSI captures",
                &["csv", "jsonl", "json", "ndjson", "parquet", "pq"],
            )
            .add_filter("All files", &["*"])
            .pick_file()
        else {
            return;
        };
        self.import_file(&path);
    }

    /// Open `path` as a read-only replay device and point the replay at it.
    pub fn import_file(&mut self, path: &std::path::Path) {
        match persistence::import_packets(path) {
            Ok(pkts) if pkts.iter().all(|p| !p.is_decoded()) => {
                self.alert.open_error(
                    "Import failed",
                    format!(
                        "No decoded packets in\n{}\n\nSupported: .csv / .jsonl / .parquet written by csi-gui-rs.",
                        path.display()
                    ),
                );
            }
            Ok(pkts) => {
                let n = pkts.len();
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("import")
                    .to_string();
                let i = self.devices.add_imported(format!("▶ {name}"), pkts);
                self.devices.active = Some(i);
                self.replay.anchor = self.devices.sessions[i].history.first_id();
                self.replay.playing = false;
                self.toasts.info(format!(
                    "imported {n} packets as \"{name}\". Add a Replay pane to scrub; R returns to live."
                ));
            }
            Err(e) => self
                .alert
                .open_error("Import failed", format!("{}\n\n{e}", path.display())),
        }
    }
}

fn rrd_leaf(name: &str, default: &str) -> String {
    let t = name.trim();
    let unix = crate::paths::unix_now();
    if t.is_empty() {
        format!("{default}_{unix}.rrd")
    } else if t.ends_with(".rrd") {
        t.to_string()
    } else {
        format!("{}_{unix}.rrd", crate::paths::sanitize_file_stem(t))
    }
}

/// Parse "host:port" into a `Transport::Connect`, defaulting the missing
/// pieces to 127.0.0.1:9876.
fn parse_host_port(s: &str) -> Transport {
    let s = s.trim();
    if s.is_empty() {
        return Transport::default_connect();
    }
    let mut parts = s.rsplitn(2, ':');
    let port_str = parts.next();
    let host_str = parts.next();
    let (host, port) = match (host_str, port_str) {
        (Some(h), Some(p)) => (h.to_string(), p.parse::<u16>().unwrap_or(9876)),
        (None, Some(p)) => match p.parse::<u16>() {
            Ok(port) => ("127.0.0.1".into(), port),
            Err(_) => (p.to_string(), 9876),
        },
        _ => ("127.0.0.1".into(), 9876),
    };
    Transport::Connect { host, port }
}
