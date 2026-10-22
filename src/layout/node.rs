use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewType {
    Empty,
    Dashboard,
    /// Every connected device in one table.
    Devices,
    Polar,
    Isometric,
    Spectrogram,
    Phase,
    Magnitude,
    Evolution,
    RawScatter,
    Console,
    Replay,
}

// Hand-rolled (de)serialization. Writes match what serde's derive would
// produce for an externally-tagged enum, so saved layouts stay
// forward-compatible. The deserializer additionally accepts the legacy
// `Camera` tag and collapses it to `Empty` — the placeholder pane was
// removed but old persistence files / templates may still mention it.
impl Serialize for ViewType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            ViewType::Empty => "Empty",
            ViewType::Dashboard => "Dashboard",
            ViewType::Devices => "Devices",
            ViewType::Polar => "Polar",
            ViewType::Isometric => "Isometric",
            ViewType::Spectrogram => "Spectrogram",
            ViewType::Phase => "Phase",
            ViewType::Magnitude => "Magnitude",
            ViewType::Evolution => "Evolution",
            ViewType::RawScatter => "RawScatter",
            ViewType::Console => "Console",
            ViewType::Replay => "Replay",
        })
    }
}

impl<'de> Deserialize<'de> for ViewType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(match raw.as_str() {
            "Empty" => ViewType::Empty,
            "Dashboard" => ViewType::Dashboard,
            "Devices" => ViewType::Devices,
            "Polar" => ViewType::Polar,
            "Isometric" => ViewType::Isometric,
            "Spectrogram" => ViewType::Spectrogram,
            "Phase" => ViewType::Phase,
            "Magnitude" => ViewType::Magnitude,
            "Evolution" => ViewType::Evolution,
            "RawScatter" => ViewType::RawScatter,
            "Console" => ViewType::Console,
            "Replay" => ViewType::Replay,
            // Legacy placeholder, collapse to Empty so old layouts load.
            "Camera" => ViewType::Empty,
            other => {
                return Err(serde::de::Error::unknown_variant(
                    other,
                    &[
                        "Empty",
                        "Dashboard",
                        "Polar",
                        "Isometric",
                        "Spectrogram",
                        "Phase",
                        "Magnitude",
                        "Evolution",
                        "RawScatter",
                        "Console",
                        "Replay",
                    ],
                ));
            }
        })
    }
}

impl ViewType {
    pub const ALL_SELECTABLE: [ViewType; 11] = [
        ViewType::Dashboard,
        ViewType::Devices,
        ViewType::Magnitude,
        ViewType::Phase,
        ViewType::Polar,
        ViewType::Isometric,
        ViewType::Spectrogram,
        ViewType::Evolution,
        ViewType::RawScatter,
        ViewType::Console,
        ViewType::Replay,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ViewType::Empty => "Empty",
            ViewType::Dashboard => "Dashboard",
            ViewType::Devices => "Devices",
            ViewType::Polar => "Polar",
            ViewType::Isometric => "Isometric (CIR)",
            ViewType::Spectrogram => "Spectrogram",
            ViewType::Phase => "Phase",
            ViewType::Magnitude => "Magnitude",
            ViewType::Evolution => "Evolution",
            ViewType::RawScatter => "I/Q Scatter",
            ViewType::Console => "Console",
            ViewType::Replay => "Replay",
        }
    }

    pub fn is_temporal(self) -> bool {
        matches!(
            self,
            ViewType::Dashboard
                | ViewType::Polar
                | ViewType::Isometric
                | ViewType::Spectrogram
                | ViewType::Phase
                | ViewType::Magnitude
                | ViewType::RawScatter
        )
    }

    pub fn is_spatial(self) -> bool {
        false
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum LayoutNode {
    Pane {
        id: usize,
        view: ViewType,
    },
    Split {
        direction: SplitDirection,
        /// 10..=90 — clamp on every write
        ratio: u16,
        children: Vec<LayoutNode>,
    },
}

impl LayoutNode {
    pub fn pane(id: usize, view: ViewType) -> Self {
        LayoutNode::Pane { id, view }
    }

    /// Collect pane ids in DFS order (used for Tab cycling and reindex).
    pub fn collect_panes(&self, out: &mut Vec<usize>) {
        match self {
            LayoutNode::Pane { id, .. } => out.push(*id),
            LayoutNode::Split { children, .. } => {
                for c in children {
                    c.collect_panes(out);
                }
            }
        }
    }

    pub fn pane_count(&self) -> usize {
        let mut v = Vec::new();
        self.collect_panes(&mut v);
        v.len()
    }

    pub fn find_view(&self, target: usize) -> Option<ViewType> {
        match self {
            LayoutNode::Pane { id, view } if *id == target => Some(*view),
            LayoutNode::Pane { .. } => None,
            LayoutNode::Split { children, .. } => children.iter().find_map(|c| c.find_view(target)),
        }
    }

    pub fn set_view(&mut self, target: usize, new_view: ViewType) -> bool {
        match self {
            LayoutNode::Pane { id, view } if *id == target => {
                *view = new_view;
                true
            }
            LayoutNode::Pane { .. } => false,
            LayoutNode::Split { children, .. } => {
                children.iter_mut().any(|c| c.set_view(target, new_view))
            }
        }
    }

    pub fn split_pane(
        &mut self,
        target: usize,
        direction: SplitDirection,
        next_id: &mut usize,
    ) -> bool {
        match self {
            LayoutNode::Pane { id, view } if *id == target => {
                let left = LayoutNode::Pane {
                    id: *id,
                    view: *view,
                };
                *next_id += 1;
                let right = LayoutNode::Pane {
                    id: *next_id,
                    view: ViewType::Empty,
                };
                *self = LayoutNode::Split {
                    direction,
                    ratio: 50,
                    children: vec![left, right],
                };
                true
            }
            LayoutNode::Pane { .. } => false,
            LayoutNode::Split { children, .. } => children
                .iter_mut()
                .any(|c| c.split_pane(target, direction, next_id)),
        }
    }

    /// Remove the pane with `target` id. Returns `Some(replacement)` if this
    /// subtree should be replaced with a different node (because a split lost
    /// its second child), otherwise `None`.
    pub fn close_pane(&mut self, target: usize) -> Option<LayoutNode> {
        match self {
            LayoutNode::Pane { id, .. } if *id == target => Some(LayoutNode::Pane {
                id: *id,
                view: ViewType::Empty,
            }),
            LayoutNode::Pane { .. } => None,
            LayoutNode::Split { children, .. } => {
                let mut idx_to_remove = None;
                for (i, child) in children.iter_mut().enumerate() {
                    if let Some(repl) = child.close_pane(target) {
                        match child {
                            LayoutNode::Pane { id, .. } if *id == target => {
                                idx_to_remove = Some(i);
                            }
                            _ => {
                                *child = repl;
                            }
                        }
                        break;
                    }
                }
                if let Some(i) = idx_to_remove {
                    children.remove(i);
                    if children.len() == 1 {
                        return Some(children.remove(0));
                    }
                }
                None
            }
        }
    }

    /// Renumber panes 1..=N in DFS order. Returns new focus id for `old_focus`.
    pub fn reindex(&mut self, old_focus: usize) -> usize {
        let mut counter = 0usize;
        let mut new_focus = old_focus;
        self.reindex_inner(&mut counter, old_focus, &mut new_focus);
        new_focus
    }

    fn reindex_inner(&mut self, counter: &mut usize, old: usize, new_focus: &mut usize) {
        match self {
            LayoutNode::Pane { id, .. } => {
                *counter += 1;
                if *id == old {
                    *new_focus = *counter;
                }
                *id = *counter;
            }
            LayoutNode::Split { children, .. } => {
                for c in children {
                    c.reindex_inner(counter, old, new_focus);
                }
            }
        }
    }
}
