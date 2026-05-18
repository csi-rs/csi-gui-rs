//! Modal overlay widgets used by `App`.
//!
//! Each overlay is a small `struct` with an `open: bool` flag and a
//! `show(&mut self, ctx) -> Result` method. They're independent of the
//! pane layout and float on top of the central panel when open.

pub mod alert;
pub mod config_overlay;
pub mod help;
pub mod main_menu;
pub mod preprocess_overlay;
pub mod template_picker;
pub mod text_input;
pub mod theme_picker;
pub mod view_selector;
