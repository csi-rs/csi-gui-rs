use super::node::{LayoutNode, SplitDirection, ViewType};
use serde::{Deserialize, Serialize};

pub const MAX_PANES: usize = 10;

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
        if self
            .root
            .split_pane(self.focused, direction, &mut next_id)
        {
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
        let cur = panes
            .iter()
            .position(|p| *p == self.focused)
            .unwrap_or(0);
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

    fn max_id(&self) -> usize {
        let mut v = Vec::new();
        self.root.collect_panes(&mut v);
        v.into_iter().max().unwrap_or(0)
    }
}
