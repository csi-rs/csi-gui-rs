//! Global keyboard shortcuts. See [`crate::ui::help`] for the table.

use super::App;
use crate::layout::SplitDirection;

impl App {
    /// `true` while a modal overlay owns the keyboard.
    fn overlay_open(&self) -> bool {
        self.help.open
            || self.main_menu.open
            || self.view_selector.open
            || self.layout_picker.open
            || self.text_prompt.open
            || self.template_picker.open
            || self.theme_picker.open
            || self.preprocess_overlay.open
            || self.alert.open
            || self.palette.open
            || self.quit_confirm_open
    }

    pub(super) fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.quit_confirm_open {
            self.handle_quit_confirm(ctx);
            return;
        }
        // Ctrl+K works even while typing in a field.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::K)) {
            self.palette.open();
            return;
        }
        if self.overlay_open() || ctx.egui_wants_keyboard_input() {
            return;
        }

        let shift = ctx.input(|i| i.modifiers.shift);
        let mut cycle_theme = false;
        let mut toggle_run = false;
        let bounds = self
            .devices
            .active()
            .and_then(|s| Some((s.history.first_id()?, s.history.last_id()?)));

        ctx.input(|i| {
            if i.modifiers.command {
                return; // leave Ctrl/Cmd chords to egui
            }
            if i.key_pressed(egui::Key::Q) {
                self.quit_confirm_open = true;
            }
            if i.key_pressed(egui::Key::H) || i.key_pressed(egui::Key::F1) {
                self.help.open = !self.help.open;
            }
            if i.key_pressed(egui::Key::M) {
                self.main_menu.open = !self.main_menu.open;
            }
            if i.key_pressed(egui::Key::T) {
                cycle_theme = true;
            }
            if i.key_pressed(egui::Key::D) {
                self.esp_panel.open = !self.esp_panel.open;
            }
            if i.key_pressed(egui::Key::S) {
                toggle_run = true;
            }
            if i.key_pressed(egui::Key::Tab) {
                self.tiling.focus_next();
            }
            if i.key_pressed(egui::Key::Delete) {
                self.tiling.close_focused();
            }
            if i.key_pressed(egui::Key::Space) {
                self.tiling.toggle_fullscreen();
            }
            if i.key_pressed(egui::Key::Enter) {
                self.view_selector.open = true;
                self.view_selector.selected = 0;
            }
            if !shift && i.key_pressed(egui::Key::R) {
                self.replay.anchor = None;
                self.replay.playing = false;
            }
            #[cfg(feature = "rerun")]
            if shift && i.key_pressed(egui::Key::R) {
                self.rerun.toggle_live();
            }
            #[cfg(feature = "rerun")]
            if shift && i.key_pressed(egui::Key::L) {
                self.rerun.toggle_rrd();
            }
            for (k, id) in [
                (egui::Key::Num1, 1),
                (egui::Key::Num2, 2),
                (egui::Key::Num3, 3),
                (egui::Key::Num4, 4),
                (egui::Key::Num5, 5),
                (egui::Key::Num6, 6),
                (egui::Key::Num7, 7),
                (egui::Key::Num8, 8),
                (egui::Key::Num9, 9),
                (egui::Key::Num0, 10),
            ] {
                if i.key_pressed(k) {
                    self.tiling.focus_by_id(id);
                }
            }

            if shift {
                // Split shortcuts must not auto-repeat: holding Shift+→
                // would otherwise spam splits up to MAX_PANES.
                if key_first_pressed(i, egui::Key::ArrowLeft) || key_first_pressed(i, egui::Key::ArrowRight) {
                    self.tiling.split(SplitDirection::Horizontal);
                }
                if key_first_pressed(i, egui::Key::ArrowUp) || key_first_pressed(i, egui::Key::ArrowDown) {
                    self.tiling.split(SplitDirection::Vertical);
                }
            } else if let Some((first, last)) = bounds {
                // Arrows scrub the global replay anchor.
                if last > first {
                    let cur = self.replay.anchor.unwrap_or(last);
                    if i.key_pressed(egui::Key::ArrowLeft) {
                        self.replay.anchor = Some(cur.saturating_sub(1).max(first));
                        self.replay.playing = false;
                    }
                    if i.key_pressed(egui::Key::ArrowRight) {
                        self.replay.anchor = if cur >= last { None } else { Some(cur + 1) };
                        self.replay.playing = false;
                    }
                }
            }
        });

        if cycle_theme {
            let next = self.theme_variant.next();
            self.set_theme(ctx, next);
        }
        if toggle_run {
            self.toggle_run_all();
        }
    }

    /// S / the toolbar: stop everything if anything runs, else start all.
    pub(super) fn toggle_run_all(&mut self) {
        let targets = self.devices.targets();
        if self.devices.any_collecting() {
            self.devices.stop_all(&targets);
            self.rerun.mark_session_boundary();
        } else {
            let (_, notices) = self.devices.start_all(&targets);
            if notices.is_empty() {
                self.toasts.push(crate::device::Notice::Warn(
                    "no device ready to start: connect one and wait for its config to apply".into(),
                ));
            }
            self.notify(notices);
            self.go_live();
        }
    }

    fn handle_quit_confirm(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut confirm = false;
        let collecting = self.devices.any_collecting();
        egui::Window::new("Quit?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("Really quit?");
                if collecting {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 180, 60),
                        "Devices are still collecting; they will be stopped and recordings closed.",
                    );
                }
                ui.horizontal(|ui| {
                    if ui.button("Quit").clicked() {
                        confirm = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
                ctx.input(|i| {
                    if i.key_pressed(egui::Key::Y) || i.key_pressed(egui::Key::Enter) {
                        confirm = true;
                    }
                    if i.key_pressed(egui::Key::N) || i.key_pressed(egui::Key::Escape) {
                        close = true;
                    }
                });
            });
        if confirm {
            let targets: Vec<usize> = (0..self.devices.sessions.len()).collect();
            self.devices.stop_all(&targets);
            self.should_close = true;
        }
        if close || confirm {
            self.quit_confirm_open = false;
        }
    }
}

/// `true` only for the initial press of `key` this frame (OS key-repeat
/// filtered out).
fn key_first_pressed(i: &egui::InputState, key: egui::Key) -> bool {
    i.events.iter().any(|e| {
        matches!(
            e,
            egui::Event::Key {
                key: k,
                pressed: true,
                repeat: false,
                ..
            } if *k == key
        )
    })
}
