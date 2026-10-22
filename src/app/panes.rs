//! The tiling area: layout tree, pane headers, and the view inside each.
//!
//! Panes render against an immutable view of the devices while the header
//! controls (view, device, split, close) are collected as [`PaneIntent`]s
//! and applied afterwards, so no closure needs `&mut App`.

use super::App;
use crate::device::DeviceManager;
use crate::layout::{LayoutNode, MAX_PANES, SplitDirection, ViewType};
use crate::theme::Theme;
use crate::views::{render_view, DeviceView, ReplayState, ViewCtx, ViewStates};

/// A header action, applied after the frame's panes are drawn.
enum PaneIntent {
    Focus(usize),
    SetView(usize, ViewType),
    SetDevice(usize, Option<String>),
    Split(usize, SplitDirection),
    Fullscreen(usize),
    Close(usize),
}

struct PaneEnv<'a> {
    devices: &'a DeviceManager,
    views: Vec<DeviceView<'a>>,
    ids: Vec<String>,
    view_states: &'a mut ViewStates,
    replay: &'a mut ReplayState,
    theme: &'a Theme,
    focused: usize,
    pane_count: usize,
    fallback: (&'a crate::data::History, &'a std::sync::Arc<crate::shared::EspSharedState>, &'a std::sync::Arc<crate::console::RawConsole>),
    intents: Vec<PaneIntent>,
}

impl App {
    pub(super) fn render_panes(&mut self, ui: &mut egui::Ui) {
        let root = if self.tiling.fullscreen {
            LayoutNode::Pane {
                id: self.tiling.focused,
                view: self.tiling.focused_view(),
            }
        } else {
            self.tiling.root.clone()
        };
        let views: Vec<DeviceView<'_>> = self
            .devices
            .sessions
            .iter()
            .map(|s| DeviceView {
                label: s.label(),
                color: s.color,
                port: &s.port,
                state: &s.conn_state,
                info: s.info.as_ref(),
                cfg: &s.cfg,
                history: &s.history,
                shared: &s.shared,
                recording: s.recorder.is_recording(),
                live: s.live,
            })
            .collect();
        let mut env = PaneEnv {
            devices: &self.devices,
            ids: self.devices.sessions.iter().map(|s| s.id()).collect(),
            views,
            view_states: &mut self.view_states,
            replay: &mut self.replay,
            theme: &self.theme,
            focused: self.tiling.focused,
            pane_count: self.tiling.pane_count(),
            fallback: (&self.fallback_history, &self.fallback_shared, &self.fallback_console),
            intents: Vec::new(),
        };
        render_node(&mut env, ui, &root);
        let intents = std::mem::take(&mut env.intents);
        drop(env);
        for intent in intents {
            match intent {
                PaneIntent::Focus(id) => self.tiling.focus_by_id(id),
                PaneIntent::SetView(id, v) => {
                    self.tiling.focus_by_id(id);
                    self.tiling.set_view(v);
                }
                PaneIntent::SetDevice(id, dev) => self.view_states.get_or_default(id).device = dev,
                PaneIntent::Split(id, dir) => {
                    self.tiling.focus_by_id(id);
                    self.tiling.split(dir);
                }
                PaneIntent::Fullscreen(id) => {
                    self.tiling.focus_by_id(id);
                    self.tiling.toggle_fullscreen();
                }
                PaneIntent::Close(id) => {
                    self.tiling.focus_by_id(id);
                    self.tiling.close_focused();
                }
            }
        }
    }
}

fn render_node(env: &mut PaneEnv<'_>, ui: &mut egui::Ui, node: &LayoutNode) {
    match node {
        LayoutNode::Pane { id, view } => render_pane(env, ui, *id, *view),
        LayoutNode::Split {
            direction,
            ratio,
            children,
        } => {
            let rect = ui.available_rect_before_wrap();
            let frac = (*ratio as f32 / 100.0).clamp(0.10, 0.90);
            let (a, b) = match direction {
                SplitDirection::Horizontal => {
                    let x = rect.left() + rect.width() * frac;
                    (
                        egui::Rect::from_min_max(rect.min, egui::pos2(x, rect.bottom())),
                        egui::Rect::from_min_max(egui::pos2(x, rect.top()), rect.max),
                    )
                }
                SplitDirection::Vertical => {
                    let y = rect.top() + rect.height() * frac;
                    (
                        egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), y)),
                        egui::Rect::from_min_max(egui::pos2(rect.left(), y), rect.max),
                    )
                }
            };
            for (child, r) in children.iter().zip([a, b]) {
                // Clip to the child's rect so a pane can never paint over
                // its neighbour.
                ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                    ui.set_clip_rect(r.intersect(ui.clip_rect()));
                    render_node(env, ui, child);
                });
            }
        }
    }
}

fn render_pane(env: &mut PaneEnv<'_>, ui: &mut egui::Ui, id: usize, view: ViewType) {
    let focused = id == env.focused;
    let border = if focused { env.theme.focused_border } else { env.theme.normal_border };

    // Which device this pane shows: its binding, else the active device.
    let binding = env.view_states.get_or_default(id).device.clone();
    let bound = binding
        .as_deref()
        .and_then(|b| env.ids.iter().position(|i| i == b))
        .or(env.devices.active);
    let stale_binding = binding.is_some() && binding.as_deref().and_then(|b| env.ids.iter().position(|i| i == b)).is_none();

    egui::Frame::default()
        .stroke(egui::Stroke::new(if focused { 2.0_f32 } else { 1.0_f32 }, border))
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            let resp = ui.interact(
                ui.available_rect_before_wrap(),
                egui::Id::new(("pane-bg", id)),
                egui::Sense::click(),
            );
            if resp.clicked() {
                env.intents.push(PaneIntent::Focus(id));
            }

            ui.push_id(("pane-header", id), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(format!("#{id}"))
                            .strong()
                            .size(15.0)
                            .color(if focused { env.theme.focused_border } else { env.theme.text_highlight }),
                    );
                    egui::ComboBox::from_id_salt(("pane-view", id))
                        .selected_text(view.label())
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for v in ViewType::ALL_SELECTABLE {
                                if ui.selectable_label(v == view, v.label()).clicked() {
                                    env.intents.push(PaneIntent::SetView(id, v));
                                }
                            }
                        });
                    if view != ViewType::Devices && !env.views.is_empty() {
                        device_picker(env, ui, id, binding.as_deref(), bound, stale_binding);
                    }
                    // One menu instead of a row of icon buttons: it never
                    // collides with the pickers in a narrow pane.
                    ui.menu_button("☰", |ui| {
                        let can_split = env.pane_count < MAX_PANES;
                        if ui.add_enabled(can_split, egui::Button::new("Split into columns   Shift+→")).clicked() {
                            env.intents.push(PaneIntent::Split(id, SplitDirection::Horizontal));
                            ui.close();
                        }
                        if ui.add_enabled(can_split, egui::Button::new("Split into rows   Shift+↓")).clicked() {
                            env.intents.push(PaneIntent::Split(id, SplitDirection::Vertical));
                            ui.close();
                        }
                        if ui.button("Fullscreen   Space").clicked() {
                            env.intents.push(PaneIntent::Fullscreen(id));
                            ui.close();
                        }
                        ui.separator();
                        if ui.add_enabled(env.pane_count > 1, egui::Button::new("Close pane   Del")).clicked() {
                            env.intents.push(PaneIntent::Close(id));
                            ui.close();
                        }
                    })
                    .response
                    .on_hover_text("Split, fullscreen or close this pane");
                });
            });
            ui.separator();

            let body_h = ui.available_height().max(0.0);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), body_h),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(("pane-scroll", id))
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            let mut vs = env.view_states.get_or_default(id).clone();
                            let (history, shared, console, info, label, color) = match bound {
                                Some(i) => {
                                    let d = &env.devices.sessions[i];
                                    (&d.history, &d.shared, &d.console, d.info.as_ref(), env.views[i].label.as_str(), d.color)
                                }
                                None => (env.fallback.0, env.fallback.1, env.fallback.2, None, "", env.theme.text_highlight),
                            };
                            let mut ctx = ViewCtx {
                                history,
                                shared,
                                console,
                                replay: env.replay,
                                device: info,
                                device_label: label,
                                device_color: color,
                                devices: &env.views,
                                theme: env.theme,
                                view_state: &mut vs,
                                focused,
                                plot_budget: body_h,
                            };
                            render_view(view, ui, &mut ctx);
                            *env.view_states.get_or_default(id) = vs;
                        });
                },
            );
        });
}

/// The pane's device selector: "Follow active" or a specific device.
fn device_picker(
    env: &mut PaneEnv<'_>,
    ui: &mut egui::Ui,
    id: usize,
    binding: Option<&str>,
    bound: Option<usize>,
    stale: bool,
) {
    let text = match (binding, bound) {
        (Some(_), _) if stale => egui::RichText::new("● (gone)").color(egui::Color32::GRAY),
        (Some(_), Some(i)) => egui::RichText::new(format!("● {}", env.views[i].label)).color(env.views[i].color),
        (None, Some(i)) => egui::RichText::new(format!("↻ {}", env.views[i].label)).color(env.views[i].color),
        _ => egui::RichText::new("no device"),
    };
    egui::ComboBox::from_id_salt(("pane-device", id))
        .selected_text(text)
        .width(120.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(binding.is_none(), "↻ Follow active device")
                .clicked()
            {
                env.intents.push(PaneIntent::SetDevice(id, None));
            }
            ui.separator();
            for (i, v) in env.views.iter().enumerate() {
                let sel = binding == Some(env.ids[i].as_str());
                let label = egui::RichText::new(format!("● {}", v.label)).color(v.color);
                if ui.selectable_label(sel, label).clicked() {
                    env.intents.push(PaneIntent::SetDevice(id, Some(env.ids[i].clone())));
                }
            }
        })
        .response
        .on_hover_text("Which device this pane shows");
}
