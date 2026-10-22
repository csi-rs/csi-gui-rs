//! The Ctrl+K command palette's entries and their actions.

use super::App;
use crate::layout::{LayoutPreset, SplitDirection, ViewType};
use crate::theme::ThemeVariant;
use crate::ui::command_palette::PaletteEntry;

impl App {
    fn palette_entries(&self) -> Vec<PaletteEntry> {
        let mut v = vec![
            PaletteEntry::new("run", "Devices: start / stop all", "S"),
            PaletteEntry::new("apply-all", "Devices: apply config to all", ""),
            PaletteEntry::new("attach", "Devices: attach all ESP32 ports", ""),
            PaletteEntry::new("sim", "Devices: add simulated device", ""),
            PaletteEntry::new("auto", "Devices: toggle auto-attach", ""),
            PaletteEntry::new("record", "Devices: toggle record while collecting", ""),
            PaletteEntry::new("panel", "View: toggle device panel", "D"),
            PaletteEntry::new("live", "Replay: back to live", "R"),
            PaletteEntry::new("export", "File: export captures…", ""),
            PaletteEntry::new("import", "File: import capture…", ""),
            PaletteEntry::new("save-tpl", "File: save template…", ""),
            PaletteEntry::new("load-tpl", "File: load template…", ""),
            PaletteEntry::new("preprocess", "File: preprocessing…", ""),
            PaletteEntry::new("split-h", "Pane: split into columns", "Shift+→"),
            PaletteEntry::new("split-v", "Pane: split into rows", "Shift+↓"),
            PaletteEntry::new("full", "Pane: toggle fullscreen", "Space"),
            PaletteEntry::new("close", "Pane: close", "Del"),
            PaletteEntry::new("follow", "Pane: follow the active device", ""),
            PaletteEntry::new("help", "Help: keyboard shortcuts", "H"),
            PaletteEntry::new("quit", "Quit", "Q"),
        ];
        for view in ViewType::ALL_SELECTABLE {
            v.push(PaletteEntry::new(format!("view:{}", view.label()), format!("Pane: show {}", view.label()), ""));
        }
        for (i, p) in LayoutPreset::ALL.iter().enumerate() {
            v.push(PaletteEntry::new(format!("layout:{i}"), format!("Layout: {}", p.label()), ""));
        }
        for t in ThemeVariant::ALL {
            v.push(PaletteEntry::new(format!("theme:{}", t.as_u8()), format!("Theme: {}", t.label()), ""));
        }
        for (i, s) in self.devices.sessions.iter().enumerate() {
            let l = s.label();
            v.push(PaletteEntry::new(format!("select:{i}"), format!("Device: select {l}"), ""));
            v.push(PaletteEntry::new(format!("bind:{i}"), format!("Pane: show device {l}"), ""));
            if s.is_ready() {
                let verb = if s.is_collecting() { "stop" } else { "start" };
                v.push(PaletteEntry::new(format!("toggle:{i}"), format!("Device: {verb} {l}"), ""));
            }
        }
        for p in self.devices.free_ports() {
            v.push(PaletteEntry::new(format!("add:{}", p.name), format!("Devices: add {}", p.describe()), ""));
        }
        v
    }

    pub(super) fn show_palette(&mut self, ctx: &egui::Context) {
        if !self.palette.open {
            return;
        }
        let entries = self.palette_entries();
        let Some(id) = self.palette.show(ctx, &entries) else { return };
        let focused = self.tiling.focused;
        match id.as_str() {
            "run" => self.toggle_run_all(),
            "apply-all" => {
                let t = self.devices.targets();
                let n = self.devices.apply_all(&t);
                self.notify(n);
            }
            "attach" => {
                self.devices.attach_all();
            }
            "sim" => self.add_simulated(),
            "auto" => self.devices.auto_attach = !self.devices.auto_attach,
            "record" => self.devices.record_on_capture = !self.devices.record_on_capture,
            "panel" => self.esp_panel.open = !self.esp_panel.open,
            "live" => self.go_live(),
            "export" => self.prompt_export(),
            "import" => self.run_import_flow(),
            "save-tpl" => self.prompt_save_template(),
            "load-tpl" => self.open_template_picker(),
            "preprocess" => self.open_preprocess(),
            "split-h" => self.tiling.split(SplitDirection::Horizontal),
            "split-v" => self.tiling.split(SplitDirection::Vertical),
            "full" => self.tiling.toggle_fullscreen(),
            "close" => self.tiling.close_focused(),
            "follow" => self.view_states.get_or_default(focused).device = None,
            "help" => self.help.open = true,
            "quit" => self.quit_confirm_open = true,
            other => {
                let (kind, arg) = other.split_once(':').unwrap_or((other, ""));
                match kind {
                    "view" => {
                        if let Some(v) = ViewType::ALL_SELECTABLE.into_iter().find(|v| v.label() == arg) {
                            self.tiling.set_view(v);
                        }
                    }
                    "layout" => {
                        if let Some(p) = arg.parse::<usize>().ok().and_then(|i| LayoutPreset::ALL.get(i)) {
                            self.tiling.apply_preset(*p);
                        }
                    }
                    "theme" => {
                        if let Ok(t) = arg.parse::<u8>() {
                            self.set_theme(ctx, ThemeVariant::from_u8(t));
                        }
                    }
                    "select" => {
                        if let Ok(i) = arg.parse() {
                            self.devices.active = Some(i);
                        }
                    }
                    "bind" => {
                        if let Some(id) = arg.parse::<usize>().ok().and_then(|i| self.devices.sessions.get(i)).map(|s| s.id()) {
                            self.view_states.get_or_default(focused).device = Some(id);
                        }
                    }
                    "toggle" => {
                        if let Some(s) = arg.parse::<usize>().ok().and_then(|i| self.devices.sessions.get_mut(i)) {
                            if s.is_collecting() {
                                s.stop();
                            } else {
                                s.start(None);
                            }
                        }
                    }
                    "add" => {
                        let i = self.devices.add(arg);
                        self.devices.active = Some(i);
                    }
                    _ => {}
                }
            }
        }
    }
}
