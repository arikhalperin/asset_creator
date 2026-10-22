//! Game Asset Creator: a desktop image editor for building game assets.

pub mod app_state;
pub mod asset_library;
pub mod canvas;
pub mod cleanup;
pub mod document;
pub mod extract;
pub mod extract_ui;
pub mod filter_dialog;
pub mod filters;
pub mod frame_set;
pub mod io;
pub mod layers;
pub mod layers_panel;
pub mod library;
pub mod library_ui;
pub mod paint;
pub mod parts_ui;
pub mod pattern;
pub mod pattern_ui;
pub mod pdf;
pub mod project;
pub mod recolor;
pub mod review_ui;
pub mod selection;
pub mod tools;
pub mod transform;
pub mod ui;
pub mod undo;
pub mod video;
pub mod video_ui;

pub use app_state::{AppState, DocId};
pub use document::{Document, DocumentError, Layer, PixelRect};
pub use ui::{EditorApp, APP_TITLE};
pub use undo::{Command, CommandError, FillLayer, UndoStack};
