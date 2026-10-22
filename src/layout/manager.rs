use super::node::{LayoutNode, SplitDirection, ViewType};
use serde::{Deserialize, Serialize};

pub const MAX_PANES: usize = 10;

/// One-click tiling arrangements offered by the layout picker. These are
/// guided shortcuts on top of the free-form split engine — every preset
/// just builds a concrete [`LayoutNode`] tree that the user can still
/// tweak afterwards with the per-pane split/close controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutPreset {
    Single,
    TwoColumns,
    TwoRows,
    ThreeColumns,
    Grid2x2,
}

impl LayoutPreset {
    /// Presets shown in the picker, in display order.
    pub const ALL: [LayoutPreset; 5] = [
        LayoutPreset::Single,
        LayoutPreset::TwoColumns,
        LayoutPreset::TwoRows,
        LayoutPreset::ThreeColumns,
        LayoutPreset::Grid2x2,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LayoutPreset::Single => "Single",
            LayoutPreset::TwoColumns => "Two columns",
            LayoutPreset::TwoRows => "Two rows",
            LayoutPreset::ThreeColumns => "Three columns",
            LayoutPreset::Grid2x2 => "2 × 2 grid",
        }
    }

    /// Small ASCII sketch of the arrangement for the picker button.
    pub fn glyph(self) -> &'static str {
        match self {
            LayoutPreset::Single => "▭",
            LayoutPreset::TwoColumns => "▏▏",
            LayoutPreset::TwoRows => "☰",
            LayoutPreset::ThreeColumns => "▏▏▏",
            LayoutPreset::Grid2x2 => "田",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TilingManager {
    pub root: LayoutNode,
    pub focused: usize,
    pub fullscreen: bool,
    pub is_default: bool,
    pub theme_variant: u8,
}

impl Default for TilingManager {
    fn default() -> Self {
        Self {
            root: LayoutNode::Pane {
                id: 1,
                view: ViewType::Dashboard,
            },
            focused: 1,
            fullscreen: false,
            is_default: false,
            theme_variant: 0,
        }
    }
}

impl TilingManager {
    pub fn pane_count(&self) -> usize {
        self.root.pane_count()
    }

    pub fn focused_view(&self) -> ViewType {
        self.root.find_view(self.focused).unwrap_or(ViewType::Empty)
    }

    pub fn set_view(&mut self, view: ViewType) {
        self.root.set_view(self.focused, view);
    }

    pub fn split(&mut self, direction: SplitDirection) {
        if self.pane_count() >= MAX_PANES {
            return;
        }
        let mut next_id = self.max_id();
        if self.root.split_pane(self.focused, direction, &mut next_id) {
            self.focused = self.root.reindex(self.focused);
        }
    }

    pub fn close_focused(&mut self) {
        let target = self.focused;
        if let Some(repl) = self.root.close_pane(target) {
            self.root = repl;
        }
        self.focused = self.root.reindex(target.saturating_sub(1).max(1));
        // re-resolve focus to nearest existing pane
        let mut panes = Vec::new();
        self.root.collect_panes(&mut panes);
        if !panes.contains(&self.focused) {
            self.focused = *panes.first().unwrap_or(&1);
        }
    }

    pub fn focus_next(&mut self) {
        let mut panes = Vec::new();
        self.root.collect_panes(&mut panes);
        if panes.is_empty() {
            return;
        }
        let cur = panes.iter().position(|p| *p == self.focused).unwrap_or(0);
        self.focused = panes[(cur + 1) % panes.len()];
    }

    pub fn focus_by_id(&mut self, id: usize) {
        let mut panes = Vec::new();
        self.root.collect_panes(&mut panes);
        if panes.contains(&id) {
            self.focused = id;
        }
    }

    pub fn toggle_fullscreen(&mut self) {
        self.fullscreen = !self.fullscreen;
    }

    /// Replace the whole layout with a [`LayoutPreset`] arrangement.
    ///
    /// The current panes' views are collected in DFS order and reassigned
    /// to the new panes in the same order (padding with [`ViewType::Empty`]
    /// once the preset has more panes than the old layout), so switching
    /// arrangements keeps showing what the user was looking at wherever it
    /// fits. Panes are numbered 1..=N and focus resets to the first pane.
    pub fn apply_preset(&mut self, preset: LayoutPreset) {
        let mut old_ids = Vec::new();
        self.root.collect_panes(&mut old_ids);
        let mut views = old_ids
            .iter()
            .filter_map(|id| self.root.find_view(*id))
            .collect::<Vec<_>>()
            .into_iter();

        let mut next_id = 0usize;
        let mut mk = || {
            next_id += 1;
            LayoutNode::Pane {
                id: next_id,
                view: views.next().unwrap_or(ViewType::Empty),
            }
        };

        let split = |direction, ratio, children| LayoutNode::Split {
            direction,
            ratio,
            children,
        };

        self.root = match preset {
            LayoutPreset::Single => mk(),
            LayoutPreset::TwoColumns => split(SplitDirection::Horizontal, 50, vec![mk(), mk()]),
            LayoutPreset::TwoRows => split(SplitDirection::Vertical, 50, vec![mk(), mk()]),
            // Left third + two equal thirds on the right (33 / 33.5 / 33.5).
            LayoutPreset::ThreeColumns => split(
                SplitDirection::Horizontal,
                33,
                vec![
                    mk(),
                    split(SplitDirection::Horizontal, 50, vec![mk(), mk()]),
                ],
            ),
            LayoutPreset::Grid2x2 => split(
                SplitDirection::Horizontal,
                50,
                vec![
                    split(SplitDirection::Vertical, 50, vec![mk(), mk()]),
                    split(SplitDirection::Vertical, 50, vec![mk(), mk()]),
                ],
            ),
        };

        self.focused = 1;
        self.fullscreen = false;
    }

    fn max_id(&self) -> usize {
        let mut v = Vec::new();
        self.root.collect_panes(&mut v);
        v.into_iter().max().unwrap_or(0)
    }
}
