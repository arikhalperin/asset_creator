//! Game Asset Creator: a desktop image editor for building game assets.

pub mod app_state;
pub mod document;
pub mod io;
pub mod ui;

pub use app_state::{AppState, DocId};
pub use document::{Document, DocumentError, Layer};
pub use ui::{EditorApp, APP_TITLE};
