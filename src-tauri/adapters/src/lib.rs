//! Application adapters for detection, capture, preflight, and restore.

pub mod browser;
pub mod git;
pub mod runtime;
pub mod scan;
pub mod terminal;
pub mod traits;
pub mod vscode;

pub use browser::BrowserAdapter;
pub use git::GitAdapter;
pub use runtime::RuntimeAdapter;
pub use terminal::TerminalAdapter;
pub use traits::*;
pub use vscode::VSCodeAdapter;
