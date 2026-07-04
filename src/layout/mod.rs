//! Tiling-window pane manager.
//!
//! The window is recursively split into rectangles, each of which is either
//! a [`LayoutNode::Pane`] (one [`ViewType`]) or a [`LayoutNode::Split`].
//! [`TilingManager`] owns the root and the focus pointer; persistence is
//! handled by the `serde` impls on the layout tree.

pub mod manager;
pub mod node;

pub use manager::{LayoutPreset, MAX_PANES, TilingManager};
pub use node::{LayoutNode, SplitDirection, ViewType};
