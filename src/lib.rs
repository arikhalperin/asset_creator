//! Game Asset Creator: a desktop image editor for building game assets.

pub mod app_state;
pub mod canvas;
pub mod document;
pub mod io;
pub mod ui;
pub mod undo;

pub use app_state::{AppState, DocId};
pub use document::{Document, DocumentError, Layer, PixelRect};
pub use ui::{EditorApp, APP_TITLE};
pub use undo::{Command, CommandError, FillLayer, UndoStack};
