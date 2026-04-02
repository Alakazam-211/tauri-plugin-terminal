use std::collections::HashMap;
use tauri::State;
use uuid::Uuid;

use crate::grid_types::GridUpdate;
use crate::TerminalState;


#[tauri::command]
pub fn terminal_create(
    state: State<'_, TerminalState>,
    app: tauri::AppHandle,
    cwd: String,
    command: Option<String>,
    args: Option<Vec<String>>,
    cols: Option<u16>,
    rows: Option<u16>,
    id: Option<String>,
    env: Option<HashMap<String, String>>,
) -> Result<serde_json::Value, String> {
    let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());

    log::debug!("[terminal] Creating terminal id={} cwd={} command={:?} size={}x{}", id, cwd, command, cols.unwrap_or(80), rows.unwrap_or(24));

    let mut manager = state
        .manager
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;

    match manager.create(id.clone(), cwd, command, args, cols, rows, env, app) {
        Ok(()) => {
            log::debug!("[terminal] Terminal {} created successfully", id);
            Ok(serde_json::json!({ "id": id }))
        }
        Err(e) => {
            log::debug!("[terminal] Terminal creation failed: {}", e);
            Err(e)
        }
    }
}

#[tauri::command]
pub fn terminal_write(
    state: State<'_, TerminalState>,
    id: String,
    data: String,
) -> Result<(), String> {
    state
        .manager
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?
        .write(&id, &data)
}

#[tauri::command]
pub fn terminal_resize(
    state: State<'_, TerminalState>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state
        .manager
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?
        .resize(&id, cols, rows)
}

#[tauri::command]
pub fn terminal_kill(
    state: State<'_, TerminalState>,
    id: String,
) -> Result<(), String> {
    state
        .manager
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?
        .kill(&id)
}

#[tauri::command]
pub fn terminal_exists(
    state: State<'_, TerminalState>,
    id: String,
) -> Result<bool, String> {
    Ok(state
        .manager
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?
        .exists(&id))
}

#[tauri::command]
pub fn terminal_get_grid(
    state: State<'_, TerminalState>,
    id: String,
) -> Result<GridUpdate, String> {
    state
        .manager
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?
        .get_grid(&id)
}

#[tauri::command]
pub fn terminal_scroll(
    state: State<'_, TerminalState>,
    id: String,
    delta: i32,
) -> Result<(), String> {
    state
        .manager
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?
        .scroll(&id, delta)
}

#[tauri::command]
pub fn terminal_set_focus(
    state: State<'_, TerminalState>,
    id: String,
    focused: bool,
) -> Result<(), String> {
    state
        .manager
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?
        .set_focus(&id, focused)
}

#[tauri::command]
pub fn terminal_get_selection_text(
    state: State<'_, TerminalState>,
    id: String,
    start_col: u16,
    start_row: u16,
    end_col: u16,
    end_row: u16,
) -> Result<String, String> {
    state
        .manager
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?
        .get_selection_text(&id, start_col, start_row, end_col, end_row)
}

#[cfg(unix)]
#[tauri::command]
pub fn terminal_kill_foreground(
    state: State<'_, TerminalState>,
    id: String,
) -> Result<(), String> {
    state
        .manager
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?
        .kill_foreground(&id)
}

#[cfg(unix)]
#[tauri::command]
pub fn terminal_get_foreground_command(
    state: State<'_, TerminalState>,
    id: String,
) -> Result<Option<String>, String> {
    state
        .manager
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?
        .get_foreground_command(&id)
}

#[tauri::command]
pub fn terminal_set_font_size(
    state: State<'_, TerminalState>,
    id: String,
    font_size: f32,
    dpr: f32,
) -> Result<serde_json::Value, String> {
    let (cw, ch) = state
        .manager
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?
        .set_font_size(&id, font_size, dpr)?;
    Ok(serde_json::json!({ "cell_width": cw, "cell_height": ch }))
}

#[tauri::command]
pub fn terminal_get_cell_metrics(
    state: State<'_, TerminalState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let (cw, ch, cols, rows) = state
        .manager
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?
        .get_cell_metrics(&id)?;
    Ok(serde_json::json!({ "cell_width": cw, "cell_height": ch, "cols": cols, "rows": rows }))
}

#[tauri::command]
pub fn terminal_active_count_for_path(
    state: State<'_, TerminalState>,
    path: String,
) -> Result<i32, String> {
    Ok(state
        .manager
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?
        .get_count_for_path(&path))
}
