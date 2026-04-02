//! # tauri-plugin-terminal
//!
//! Terminal emulation plugin for Tauri v2, powered by `alacritty_terminal`.
//!
//! Provides full PTY management with DOM-based grid rendering via Tauri events.
//! Terminals are created, resized, and destroyed through Tauri commands. Grid
//! updates are emitted as `terminal:grid:{id}` events with compact line data
//! that the frontend renders as styled DOM elements.
//!
//! ## Quick Start
//!
//! ```rust
//! fn main() {
//!     tauri::Builder::default()
//!         .plugin(tauri_plugin_terminal::init())
//!         .run(tauri::generate_context!())
//!         .expect("error while running tauri application");
//! }
//! ```

mod backend;
mod commands;
pub mod grid_types;

pub use backend::TerminalManager;

use std::sync::Mutex;
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Wry,
};

/// Plugin state — holds the terminal manager.
pub struct TerminalState {
    pub manager: Mutex<TerminalManager>,
}

/// Initialize the terminal plugin.
///
/// Call this in your Tauri builder:
/// ```rust
/// tauri::Builder::default()
///     .plugin(tauri_plugin_terminal::init())
/// ```
pub fn init() -> TauriPlugin<Wry> {
    // Ignore SIGPIPE at process startup so writing to a dead PTY returns EPIPE
    // instead of killing the entire Tauri process.
    #[cfg(unix)]
    backend::ignore_sigpipe();

    Builder::<Wry, ()>::new("terminal")
        .setup(|app, _api| {
            app.manage(TerminalState {
                manager: Mutex::new(TerminalManager::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::terminal_create,
            commands::terminal_write,
            commands::terminal_resize,
            commands::terminal_kill,
            commands::terminal_exists,
            commands::terminal_get_grid,
            commands::terminal_scroll,
            commands::terminal_set_focus,
            commands::terminal_get_selection_text,
            commands::terminal_kill_foreground,
            commands::terminal_get_foreground_command,
            commands::terminal_set_font_size,
            commands::terminal_get_cell_metrics,
            commands::terminal_active_count_for_path,
        ])
        .build()
}
