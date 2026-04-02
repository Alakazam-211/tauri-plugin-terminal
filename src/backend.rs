use std::borrow::Cow;
use std::collections::HashMap;
use std::os::unix::io::AsRawFd;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use alacritty_terminal::event::{Event as AlacEvent, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::tty;
use alacritty_terminal::Term;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor, Rgb};
use tauri::{AppHandle, Emitter};
use crate::grid_types::*;
// ── Terminal Event Listener ─────────────────────────────────────────────
/// Event listener that forwards alacritty terminal events to a channel
/// for the grid emission thread to process.
#[derive(Clone)]
struct TerminalListener {
    wakeup_tx: mpsc::Sender<()>,
    app_handle: AppHandle,
    id: String,
}
impl EventListener for TerminalListener {
    fn send_event(&self, event: AlacEvent) {
        match event {
            AlacEvent::Wakeup => {
                let _ = self.wakeup_tx.send(());
            }
            AlacEvent::Title(title) => {
                let _ = self.app_handle.emit(
                    &format!("terminal:title:{}", self.id),
                    &title,
                );
            }
            AlacEvent::Bell => {
                let _ = self.app_handle.emit(
                    &format!("terminal:bell:{}", self.id),
                    (),
                );
            }
            AlacEvent::ChildExit(status) => {
                let code = status.code().unwrap_or(-1);
                let _ = self.app_handle.emit(
                    &format!("terminal:exit:{}", self.id),
                    serde_json::json!({ "exitCode": code }),
                );
            }
            AlacEvent::Exit => {
                let _ = self.app_handle.emit(
                    &format!("terminal:exit:{}", self.id),
                    serde_json::json!({ "exitCode": 0 }),
                );
            }
            _ => {}
        }
    }
}
// ── Terminal Size Helper ────────────────────────────────────────────────
struct TermSize {
    cols: usize,
    rows: usize,
}
impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.rows + 5000 // scrollback
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}
// ── Shell escaping ─────────────────────────────────────────────────────
/// Shell-escape a single argument for use in a `-c` shell string.
/// Wraps in single quotes and escapes any embedded single quotes.
fn shell_escape_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    // If the arg contains no special chars, return as-is
    if arg.chars().all(|c| c.is_alphanumeric() || "-_./=:@,%+".contains(c)) {
        return arg.to_string();
    }
    // Wrap in single quotes, escaping any embedded single quotes: ' -> '\''
    format!("'{}'", arg.replace('\'', "'\\''"))
}
// ── Default terminal colors (VS Code Dark+) ─────────────────────────────
fn default_color_palette() -> [Option<Rgb>; 269] {
    let mut colors = [None; 269];
    // Named colors (0-15)
    colors[NamedColor::Black as usize] = Some(Rgb { r: 0x1e, g: 0x1e, b: 0x1e });
    colors[NamedColor::Red as usize] = Some(Rgb { r: 0xf4, g: 0x47, b: 0x47 });
    colors[NamedColor::Green as usize] = Some(Rgb { r: 0x6a, g: 0x99, b: 0x55 });
    colors[NamedColor::Yellow as usize] = Some(Rgb { r: 0xd7, g: 0xba, b: 0x7d });
    colors[NamedColor::Blue as usize] = Some(Rgb { r: 0x56, g: 0x9c, b: 0xd6 });
    colors[NamedColor::Magenta as usize] = Some(Rgb { r: 0xc5, g: 0x86, b: 0xc0 });
    colors[NamedColor::Cyan as usize] = Some(Rgb { r: 0x9c, g: 0xdc, b: 0xfe });
    colors[NamedColor::White as usize] = Some(Rgb { r: 0xd4, g: 0xd4, b: 0xd4 });
    colors[NamedColor::BrightBlack as usize] = Some(Rgb { r: 0x5a, g: 0x5a, b: 0x5a });
    colors[NamedColor::BrightRed as usize] = Some(Rgb { r: 0xf4, g: 0x47, b: 0x47 });
    colors[NamedColor::BrightGreen as usize] = Some(Rgb { r: 0x6a, g: 0x99, b: 0x55 });
    colors[NamedColor::BrightYellow as usize] = Some(Rgb { r: 0xd7, g: 0xba, b: 0x7d });
    colors[NamedColor::BrightBlue as usize] = Some(Rgb { r: 0x56, g: 0x9c, b: 0xd6 });
    colors[NamedColor::BrightMagenta as usize] = Some(Rgb { r: 0xc5, g: 0x86, b: 0xc0 });
    colors[NamedColor::BrightCyan as usize] = Some(Rgb { r: 0x9c, g: 0xdc, b: 0xfe });
    colors[NamedColor::BrightWhite as usize] = Some(Rgb { r: 0xff, g: 0xff, b: 0xff });
    // Foreground/Background
    colors[NamedColor::Foreground as usize] = Some(Rgb { r: 0xe0, g: 0xe0, b: 0xe0 });
    colors[NamedColor::Background as usize] = Some(Rgb { r: 0x0a, g: 0x0a, b: 0x0a });
    colors[NamedColor::Cursor as usize] = Some(Rgb { r: 0x52, g: 0x8b, b: 0xff });
    // Standard 256-color palette (indices 16-255)
    // Colors 16-231: 6x6x6 color cube
    for r in 0..6u8 {
        for g in 0..6u8 {
            for b in 0..6u8 {
                let idx = 16 + (r as usize * 36) + (g as usize * 6) + b as usize;
                let rv = if r == 0 { 0 } else { 55 + r * 40 };
                let gv = if g == 0 { 0 } else { 55 + g * 40 };
                let bv = if b == 0 { 0 } else { 55 + b * 40 };
                if idx < 269 {
                    colors[idx] = Some(Rgb { r: rv, g: gv, b: bv });
                }
            }
        }
    }
    // Colors 232-255: grayscale ramp
    for i in 0..24u8 {
        let v = 8 + i * 10;
        let idx = 232 + i as usize;
        if idx < 269 {
            colors[idx] = Some(Rgb { r: v, g: v, b: v });
        }
    }
    colors
}
/// Resolve an alacritty Color to an RGB value using the terminal color palette.
fn resolve_color(color: &AnsiColor, palette: &[Option<Rgb>; 269]) -> u32 {
    match color {
        AnsiColor::Spec(rgb) => ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32,
        AnsiColor::Named(name) => {
            let idx = *name as usize;
            if let Some(rgb) = palette.get(idx).and_then(|c| *c) {
                ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32
            } else {
                0xcccccc // fallback
            }
        }
        AnsiColor::Indexed(idx) => {
            if let Some(rgb) = palette.get(*idx as usize).and_then(|c| *c) {
                ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32
            } else {
                0xcccccc // fallback
            }
        }
    }
}
/// Convert cell flags to our compact u8 representation.
fn flags_to_u8(flags: CellFlags) -> u8 {
    let mut out: u8 = 0;
    if flags.contains(CellFlags::BOLD) {
        out |= ATTR_BOLD;
    }
    if flags.contains(CellFlags::ITALIC) {
        out |= ATTR_ITALIC;
    }
    if flags.intersects(CellFlags::UNDERLINE | CellFlags::DOUBLE_UNDERLINE | CellFlags::UNDERCURL) {
        out |= ATTR_UNDERLINE;
    }
    if flags.contains(CellFlags::STRIKEOUT) {
        out |= ATTR_STRIKETHROUGH;
    }
    if flags.contains(CellFlags::INVERSE) {
        out |= ATTR_INVERSE;
    }
    if flags.contains(CellFlags::DIM) {
        out |= ATTR_DIM;
    }
    if flags.contains(CellFlags::HIDDEN) {
        out |= ATTR_HIDDEN;
    }
    if flags.contains(CellFlags::WIDE_CHAR) {
        out |= ATTR_WIDE;
    }
    out
}
// ── Terminal Instance ───────────────────────────────────────────────────
struct AlacrittyTerminalInstance {
    term: Arc<FairMutex<Term<TerminalListener>>>,
    event_loop_sender: EventLoopSender,
    app_handle: AppHandle,
    cwd: String,
    pty_raw_fd: i32,
    child_pid: Option<u32>,
    palette: [Option<Rgb>; 269],
    /// Atomic flag for grid emission loop -- set by scroll() to force full re-snapshot.
    force_full_render: Arc<std::sync::atomic::AtomicBool>,
    /// Channel to send manual wakeups to the emission loop
    /// (e.g. after scroll_display which doesn't trigger PTY wakeups).
    wakeup_tx: Option<mpsc::Sender<()>>,
    /// Handle for the grid emission thread, joined on kill.
    grid_thread_handle: Option<thread::JoinHandle<()>>,
}
// ── Terminal Manager ────────────────────────────────────────────────────
pub struct TerminalManager {
    terminals: HashMap<String, AlacrittyTerminalInstance>,
}
impl TerminalManager {
    pub fn new() -> Self {
        Self {
            terminals: HashMap::new(),
        }
    }
    pub fn create(
        &mut self,
        id: String,
        cwd: String,
        command: Option<String>,
        args: Option<Vec<String>>,
        cols: Option<u16>,
        rows: Option<u16>,
        env: Option<HashMap<String, String>>,
        app_handle: AppHandle,
    ) -> Result<(), String> {
        if self.terminals.contains_key(&id) {
            log::debug!("[terminal] Terminal {} already exists, skipping creation", id);
            return Ok(());
        }
        let safe_cwd = resolve_cwd(&cwd);
        let shell = detect_shell();
        let c = cols.unwrap_or(80) as usize;
        let r = rows.unwrap_or(24) as usize;
        // Build terminal config
        let term_config = TermConfig {
            scrolling_history: 5000,
            ..TermConfig::default()
        };
        // Create event listener
        let (wakeup_tx, wakeup_rx) = mpsc::channel();
        // Clone wakeup_tx before it's moved into the listener.
        // Scroll uses this to inject wakeups into the same channel as PTY events.
        let scroll_wakeup_tx = wakeup_tx.clone();
        let listener = TerminalListener {
            wakeup_tx,
            app_handle: app_handle.clone(),
            id: id.clone(),
        };
        let term_size = TermSize { cols: c, rows: r };
        // Create Term
        let term = Term::new(term_config, &term_size, listener.clone());
        let term = Arc::new(FairMutex::new(term));
        // Build PTY options
        let mut pty_options = tty::Options {
            working_directory: Some(std::path::PathBuf::from(&safe_cwd)),
            drain_on_exit: true,
            ..Default::default()
        };
        // Set shell and optional command
        if let Some(ref user_command) = command {
            let mut shell_cmd = shell_escape_arg(user_command);
            if let Some(ref user_args) = args {
                for arg in user_args {
                    shell_cmd.push(' ');
                    shell_cmd.push_str(&shell_escape_arg(arg));
                }
            }
            let full_args = vec!["-ilc".to_string(), shell_cmd];
            pty_options.shell = Some(tty::Shell::new(shell, full_args));
        } else {
            pty_options.shell = Some(tty::Shell::new(shell, vec![]));
        }
        // Standard environment variables
        pty_options.env.insert("TERM".to_string(), "xterm-256color".to_string());
        pty_options.env.insert("COLORTERM".to_string(), "truecolor".to_string());
        pty_options.env.insert("PROMPT_EOL_MARK".to_string(), String::new());
        // Consumer-provided environment variables
        if let Some(custom_env) = env {
            for (key, value) in custom_env {
                pty_options.env.insert(key, value);
            }
        }
        let window_size = WindowSize {
            num_lines: r as u16,
            num_cols: c as u16,
            cell_width: 8,   // approximate, frontend will send real metrics
            cell_height: 16, // approximate
        };
        // Create PTY via alacritty's tty module
        let pty = tty::new(&pty_options, window_size, 0)
            .map_err(|e| format!("Failed to create PTY: {}", e))?;
        let raw_fd = pty.file().as_raw_fd();
        let child_pid = pty.child().id();
        // Create event loop
        let event_loop = EventLoop::new(
            Arc::clone(&term),
            listener,
            pty,
            true,  // drain_on_exit
            false, // ref_test
        )
        .map_err(|e| format!("Failed to create event loop: {}", e))?;
        let event_loop_sender = event_loop.channel();
        // Spawn the event loop thread (reads PTY, parses VT100, updates Term)
        event_loop.spawn();
        let palette = default_color_palette();
        let force_full_render = Arc::new(std::sync::atomic::AtomicBool::new(false));
        // Spawn grid (DOM text) emission thread
        let term_for_grid = Arc::clone(&term);
        let app_for_grid = app_handle.clone();
        let id_for_grid = id.clone();
        let grid_palette = palette;
        let force_full_for_grid = Arc::clone(&force_full_render);
        let grid_thread_handle = thread::spawn(move || {
            grid_emission_loop(
                &id_for_grid,
                &term_for_grid,
                &app_for_grid,
                wakeup_rx,
                grid_palette,
                force_full_for_grid,
            );
        });
        let instance = AlacrittyTerminalInstance {
            term,
            event_loop_sender,
            app_handle,
            cwd: safe_cwd,
            pty_raw_fd: raw_fd,
            child_pid: Some(child_pid),
            palette,
            force_full_render,
            wakeup_tx: Some(scroll_wakeup_tx),
            grid_thread_handle: Some(grid_thread_handle),
        };
        self.terminals.insert(id.clone(), instance);
        log::debug!("[terminal] Terminal {} created ({}x{}, pid={})", id, c, r, child_pid);
        Ok(())
    }
    pub fn write(&self, id: &str, data: &str) -> Result<(), String> {
        let instance = self
            .terminals
            .get(id)
            .ok_or_else(|| format!("Terminal {} not found", id))?;
        // Snap to bottom on user input -- like iTerm2 / Zed.
        // If the user scrolled up into history, typing should bring
        // the viewport back to the live terminal.
        {
            let mut term = instance.term.lock_unfair();
            let display_offset = term.grid().display_offset();
            if display_offset != 0 {
                term.scroll_display(alacritty_terminal::grid::Scroll::Bottom);
            }
        }
        let bytes = data.as_bytes().to_vec();
        let _ = instance
            .event_loop_sender
            .send(Msg::Input(Cow::Owned(bytes)));
        Ok(())
    }
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let instance = match self.terminals.get(id) {
            Some(i) => i,
            None => return Ok(()),
        };
        let window_size = WindowSize {
            num_lines: rows,
            num_cols: cols,
            cell_width: 8,
            cell_height: 16,
        };
        // 1. Resize the PTY fd (sends ioctl TIOCSWINSZ, kernel delivers SIGWINCH)
        let _ = instance
            .event_loop_sender
            .send(Msg::Resize(window_size));
        // 2. Resize the terminal grid (reflows content, adjusts cursor position).
        //    Both Alacritty and Zed do this from outside the event loop -- the Term
        //    is behind a FairMutex so there's no race condition.
        struct TermSize(usize, usize);
        impl Dimensions for TermSize {
            fn total_lines(&self) -> usize { self.0 }
            fn screen_lines(&self) -> usize { self.0 }
            fn columns(&self) -> usize { self.1 }
        }
        let mut term = instance.term.lock();
        term.resize(TermSize(rows as usize, cols as usize));
        Ok(())
    }
    pub fn kill(&mut self, id: &str) -> Result<(), String> {
        if let Some(mut instance) = self.terminals.remove(id) {
            // Send shutdown to event loop
            let _ = instance.event_loop_sender.send(Msg::Shutdown);
            // Drop the wakeup channel to unblock the grid emission thread
            instance.wakeup_tx.take();
            // Kill child process (Zed pattern: two-phase kill with proper reaping)
            if let Some(pid) = instance.child_pid {
                #[cfg(unix)]
                unsafe {
                    // Phase 1: SIGHUP to process group (graceful)
                    let pgid = libc::getpgid(pid as i32);
                    if pgid > 0 {
                        if libc::killpg(pgid, libc::SIGHUP) != 0 {
                            // Process group kill failed, try direct kill
                            libc::kill(pid as i32, libc::SIGHUP);
                        }
                    } else {
                        libc::kill(pid as i32, libc::SIGHUP);
                    }
                }
                thread::sleep(Duration::from_millis(100));
                #[cfg(unix)]
                unsafe {
                    // Phase 2: SIGKILL (forceful)
                    libc::kill(pid as i32, libc::SIGKILL);
                }
                // Reap the child process to prevent zombies.
                #[cfg(unix)]
                unsafe {
                    let mut status: i32 = 0;
                    let mut reaped = false;
                    for _ in 0..5 {
                        let result = libc::waitpid(pid as i32, &mut status, libc::WNOHANG);
                        if result > 0 || result == -1 {
                            reaped = true;
                            break;
                        }
                        thread::sleep(Duration::from_millis(20));
                    }
                    if !reaped {
                        libc::waitpid(pid as i32, &mut status, libc::WNOHANG);
                    }
                }
            }
            // Join the grid thread with a timeout to prevent thread leaks.
            if let Some(grid_handle) = instance.grid_thread_handle.take() {
                let (join_tx, join_rx) = std::sync::mpsc::channel();
                let joiner = thread::spawn(move || {
                    let _ = grid_handle.join();
                    let _ = join_tx.send(());
                });
                if join_rx.recv_timeout(Duration::from_millis(500)).is_err() {
                    drop(joiner);
                }
            }
            // instance is dropped here.
        }
        Ok(())
    }
    #[cfg(unix)]
    pub fn kill_foreground(&self, id: &str) -> Result<(), String> {
        let instance = self
            .terminals
            .get(id)
            .ok_or_else(|| format!("Terminal {} not found", id))?;
        let fg_pgid = unsafe { libc::tcgetpgrp(instance.pty_raw_fd) };
        if fg_pgid > 0 {
            unsafe {
                libc::killpg(fg_pgid, libc::SIGINT);
            }
            Ok(())
        } else {
            Err("Could not determine foreground process group".to_string())
        }
    }
    #[cfg(unix)]
    pub fn get_foreground_command(&self, id: &str) -> Result<Option<String>, String> {
        let instance = match self.terminals.get(id) {
            Some(i) => i,
            None => return Ok(None),
        };
        let fg_pgid = unsafe { libc::tcgetpgrp(instance.pty_raw_fd) };
        if fg_pgid <= 0 {
            return Ok(None);
        }
        // If foreground is the shell, return None
        if let Some(pid) = instance.child_pid {
            if fg_pgid == pid as i32 {
                return Ok(None);
            }
        }
        #[cfg(target_os = "macos")]
        {
            let mut buf = [0u8; 4096];
            let ret = unsafe {
                libc::proc_pidpath(
                    fg_pgid,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len() as u32,
                )
            };
            if ret > 0 {
                let path = String::from_utf8_lossy(&buf[..ret as usize]);
                let name = path.rsplit('/').next().unwrap_or("");
                if !name.is_empty() {
                    return Ok(Some(name.to_string()));
                }
            }
        }
        #[cfg(target_os = "linux")]
        {
            if let Ok(comm) = std::fs::read_to_string(format!("/proc/{}/comm", fg_pgid)) {
                let name = comm.trim();
                if !name.is_empty() {
                    return Ok(Some(name.to_string()));
                }
            }
        }
        Ok(None)
    }
    pub fn exists(&self, id: &str) -> bool {
        self.terminals.contains_key(id)
    }
    pub fn get_buffer(&self, _id: &str) -> Result<String, String> {
        // Scrollback is managed by the Term.
        // Reattach uses get_grid() instead. Return empty for backward compat.
        Ok(String::new())
    }
    /// Get a full grid snapshot for the terminal.
    pub fn get_grid(&self, id: &str) -> Result<GridUpdate, String> {
        let instance = self
            .terminals
            .get(id)
            .ok_or_else(|| format!("Terminal {} not found", id))?;
        let term = instance.term.lock_unfair();
        Ok(snapshot_grid(&term, &instance.palette, true))
    }
    /// Scroll the terminal and trigger a re-render via the emission loop.
    /// We set a flag so the emission loop does a FULL render (not damage-based),
    /// since scroll_display doesn't reliably produce damage.
    pub fn scroll(&self, id: &str, delta: i32) -> Result<(), String> {
        let instance = self
            .terminals
            .get(id)
            .ok_or_else(|| format!("Terminal {} not found", id))?;
        {
            let mut term = instance.term.lock_unfair();
            term.scroll_display(alacritty_terminal::grid::Scroll::Delta(delta));
        }
        // Set the force-full-render flag so emission loop does full snapshot
        instance.force_full_render.store(true, std::sync::atomic::Ordering::Relaxed);
        // Send wakeup through the same channel as PTY events
        if let Some(ref wakeup_tx) = instance.wakeup_tx {
            let _ = wakeup_tx.send(());
        }
        Ok(())
    }
    /// Set terminal focus state.
    pub fn set_focus(&self, id: &str, _focused: bool) -> Result<(), String> {
        let _instance = self
            .terminals
            .get(id)
            .ok_or_else(|| format!("Terminal {} not found", id))?;
        // Focus state can be used by consumers for cursor blink, etc.
        Ok(())
    }
    /// Get logical cell metrics for mouse coordinate mapping.
    /// Returns (cell_width, cell_height, grid_cols, grid_rows).
    pub fn get_cell_metrics(&self, id: &str) -> Result<(u32, u32, u16, u16), String> {
        let instance = self
            .terminals
            .get(id)
            .ok_or_else(|| format!("Terminal {} not found", id))?;
        let term = instance.term.lock_unfair();
        let grid = term.grid();
        // Return basic cell metrics (8x16 approximation -- consumers should
        // use their own font metrics for precise mapping).
        Ok((
            8,
            16,
            grid.columns() as u16,
            grid.screen_lines() as u16,
        ))
    }
    /// Set font size -- no-op in DOM-only mode, provided for API compatibility.
    /// Returns (cell_width, cell_height) approximation.
    pub fn set_font_size(&self, id: &str, _font_size: f32, _dpr: f32) -> Result<(u32, u32), String> {
        let _instance = self
            .terminals
            .get(id)
            .ok_or_else(|| format!("Terminal {} not found", id))?;
        // In DOM-only mode there is no glyph cache or bitmap to resize.
        // Return approximate cell metrics.
        Ok((8, 16))
    }
    /// Get text content from a selection range.
    pub fn get_selection_text(
        &self,
        id: &str,
        start_col: u16,
        start_row: u16,
        end_col: u16,
        end_row: u16,
    ) -> Result<String, String> {
        let instance = self
            .terminals
            .get(id)
            .ok_or_else(|| format!("Terminal {} not found", id))?;
        let term = instance.term.lock_unfair();
        let grid = term.grid();
        let cols = grid.columns();
        let screen_lines = grid.screen_lines();
        let mut text = String::new();
        // Early return for empty grid (prevents out-of-bounds access)
        if cols == 0 || screen_lines == 0 {
            return Ok(String::new());
        }
        let (sr, sc, er, ec) = if start_row < end_row || (start_row == end_row && start_col <= end_col) {
            (start_row, start_col, end_row, end_col)
        } else {
            (end_row, end_col, start_row, start_col)
        };
        for row_idx in sr..=er {
            if row_idx as usize >= screen_lines {
                break;
            }
            let line = Line(row_idx as i32);
            let row = &grid[line];
            let col_start = if row_idx == sr { (sc as usize).min(cols - 1) } else { 0 };
            let col_end = if row_idx == er {
                (ec as usize).min(cols - 1)
            } else {
                cols - 1
            };
            for col in col_start..=col_end {
                let cell = &row[Column(col)];
                if !cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                    text.push(cell.c);
                    if let Some(zws) = cell.zerowidth() {
                        for &zw in zws {
                            text.push(zw);
                        }
                    }
                }
            }
            // Add newline between rows (but not after last row)
            if row_idx < er {
                // Trim trailing spaces from line before adding newline
                let trimmed_len = text.trim_end().len();
                text.truncate(trimmed_len);
                text.push('\n');
            }
        }
        // Trim trailing whitespace from final result
        let trimmed = text.trim_end().to_string();
        Ok(trimmed)
    }
    pub fn get_count_for_path(&self, path: &str) -> i32 {
        self.terminals
            .values()
            .filter(|inst| inst.cwd.starts_with(path))
            .count() as i32
    }
    pub fn get_active_count(&self) -> i32 {
        self.terminals.len() as i32
    }
    pub fn kill_all(&mut self) {
        let ids: Vec<String> = self.terminals.keys().cloned().collect();
        for id in ids {
            let _ = self.kill(&id);
        }
    }
}
impl Drop for TerminalManager {
    fn drop(&mut self) {
        self.kill_all();
    }
}
impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}
// ── Shared Utilities ────────────────────────────────────────────────────
/// Ignore SIGPIPE at process startup so writing to a dead PTY returns EPIPE
/// instead of killing the entire Tauri process.
///
/// This is intentionally global: child processes spawned via Command::new()
/// inherit their own signal mask, so git/external tools are unaffected.
#[cfg(unix)]
pub fn ignore_sigpipe() {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}
/// Expand tilde in a path and ensure the directory exists.
pub fn resolve_cwd(cwd: &str) -> String {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/"));
    let resolved = if cwd == "~" {
        home.to_string_lossy().to_string()
    } else if cwd.starts_with("~/") {
        cwd.replacen("~", &home.to_string_lossy(), 1)
    } else {
        cwd.to_string()
    };
    if std::path::Path::new(&resolved).exists() {
        resolved
    } else {
        log::debug!("[terminal] WARNING: CWD '{}' does not exist, falling back to home", resolved);
        home.to_string_lossy().to_string()
    }
}
/// Detect the user's default shell.
pub fn detect_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| std::path::Path::new(s).exists())
        .unwrap_or_else(|| {
            for sh in &["/bin/zsh", "/bin/bash", "/bin/sh"] {
                if std::path::Path::new(sh).exists() {
                    return sh.to_string();
                }
            }
            "/bin/sh".to_string()
        })
}
// ── Grid Snapshot ───────────────────────────────────────────────────────
/// Default foreground/background colors (must match terminal theme)
const DEFAULT_FG: u32 = 0xe0e0e0;
const DEFAULT_BG: u32 = 0x0a0a0a;
/// Convert a row of alacritty cells to a compact line (text + sparse style spans).
/// Span indices use "text position" (char index into the text string, not grid column),
/// so they stay aligned even when wide char spacers are skipped.
fn row_to_compact_line(
    row_idx: usize,
    row: &alacritty_terminal::grid::Row<alacritty_terminal::term::cell::Cell>,
    cols: usize,
    palette: &[Option<Rgb>; 269],
) -> CompactLine {
    let mut text = String::with_capacity(cols);
    let mut spans: Vec<StyleSpan> = Vec::new();
    // Current span tracking -- positions are TEXT indices (not grid columns)
    let mut span_start: Option<u16> = None;
    let mut span_fg: u32 = DEFAULT_FG;
    let mut span_bg: u32 = DEFAULT_BG;
    let mut span_flags: u8 = 0;
    let mut text_pos: u16 = 0;
    // Track the rightmost styled position to avoid trimming styled trailing spaces
    let mut rightmost_styled_text_pos: Option<u16> = None;
    for col_idx in 0..cols {
        let cell = &row[Column(col_idx)];
        if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
            continue;
        }
        let fg = resolve_color(&cell.fg, palette);
        let bg = resolve_color(&cell.bg, palette);
        let flags = flags_to_u8(cell.flags);
        let current_text_pos = text_pos;
        text.push(cell.c);
        text_pos += 1;
        if let Some(zws) = cell.zerowidth() {
            for &zw in zws {
                text.push(zw);
                // Don't increment text_pos -- zero-width chars don't take a cell
            }
        }
        let is_default = fg == DEFAULT_FG && bg == DEFAULT_BG && flags == 0;
        if !is_default {
            rightmost_styled_text_pos = Some(current_text_pos);
            // Extend or start a span
            if let Some(start) = span_start {
                if fg == span_fg && bg == span_bg && flags == span_flags {
                    // Continue current span
                } else {
                    // Flush previous span, start new one
                    spans.push(StyleSpan {
                        s: start,
                        e: current_text_pos.saturating_sub(1),
                        fg: if span_fg != DEFAULT_FG { Some(span_fg) } else { None },
                        bg: if span_bg != DEFAULT_BG { Some(span_bg) } else { None },
                        fl: if span_flags != 0 { Some(span_flags) } else { None },
                    });
                    span_start = Some(current_text_pos);
                    span_fg = fg;
                    span_bg = bg;
                    span_flags = flags;
                }
            } else {
                span_start = Some(current_text_pos);
                span_fg = fg;
                span_bg = bg;
                span_flags = flags;
            }
        } else if let Some(start) = span_start {
            // Flush span -- we hit a default cell
            spans.push(StyleSpan {
                s: start,
                e: current_text_pos.saturating_sub(1),
                fg: if span_fg != DEFAULT_FG { Some(span_fg) } else { None },
                bg: if span_bg != DEFAULT_BG { Some(span_bg) } else { None },
                fl: if span_flags != 0 { Some(span_flags) } else { None },
            });
            span_start = None;
        }
    }
    // Flush final span if any
    if let Some(start) = span_start {
        spans.push(StyleSpan {
            s: start,
            e: text_pos.saturating_sub(1),
            fg: if span_fg != DEFAULT_FG { Some(span_fg) } else { None },
            bg: if span_bg != DEFAULT_BG { Some(span_bg) } else { None },
            fl: if span_flags != 0 { Some(span_flags) } else { None },
        });
    }
    // Trim trailing spaces, but only up to the rightmost styled position.
    // This preserves styled spaces (like cursor backgrounds) that would
    // otherwise be lost.
    let trimmed = if let Some(styled_pos) = rightmost_styled_text_pos {
        let min_chars = styled_pos as usize + 1;
        let t = text.trim_end();
        let trimmed_chars = t.chars().count();
        if trimmed_chars < min_chars {
            // Keep at least up to the rightmost styled char
            let chars: Vec<char> = text.chars().collect();
            chars[..min_chars.min(chars.len())].iter().collect()
        } else {
            t.to_string()
        }
    } else {
        text.trim_end().to_string()
    };
    CompactLine {
        row: row_idx as u16,
        text: trimmed,
        spans,
    }
}
/// Take a full snapshot of the terminal grid using compact line format.
fn snapshot_grid(
    term: &Term<TerminalListener>,
    palette: &[Option<Rgb>; 269],
    full: bool,
) -> GridUpdate {
    let start = std::time::Instant::now();
    let content = term.renderable_content();
    let grid = term.grid();
    let cols = grid.columns();
    let rows = grid.screen_lines();
    let display_offset = content.display_offset;
    let cursor_point = content.cursor.point;
    // Read cursor shape from alacritty (Block, Beam, Underline, Hidden, HollowBlock)
    let (cursor_visible, cursor_shape) = match content.cursor.shape {
        alacritty_terminal::vte::ansi::CursorShape::Block => (true, "block"),
        alacritty_terminal::vte::ansi::CursorShape::Underline => (true, "underline"),
        alacritty_terminal::vte::ansi::CursorShape::Beam => (true, "bar"),
        _ => (true, "block"),
    };
    // Hide cursor when scrolled up or when SHOW_CURSOR is off
    let cursor_visible = cursor_visible
        && display_offset == 0
        && term.mode().contains(alacritty_terminal::term::TermMode::SHOW_CURSOR);
    let mut lines = Vec::with_capacity(rows);
    for row_idx in 0..rows {
        let line = Line(row_idx as i32 - display_offset as i32);
        let row = &grid[line];
        let compact = row_to_compact_line(row_idx, row, cols, palette);
        // Skip entirely empty lines to reduce payload
        if !compact.text.is_empty() || !compact.spans.is_empty() || !full {
            lines.push(compact);
        }
    }
    let elapsed = start.elapsed();
    let text_bytes: u32 = lines.iter().map(|l| l.text.len() as u32).sum();
    let span_count: u16 = lines.iter().map(|l| l.spans.len() as u16).sum();
    GridUpdate {
        cols: cols as u16,
        rows: rows as u16,
        cursor_col: cursor_point.column.0 as u16,
        cursor_row: cursor_point.line.0 as u16,
        cursor_visible,
        cursor_shape: cursor_shape.to_string(),
        lines,
        full,
        mode: term.mode().bits(),
        display_offset,
        selection: None,
    }
}
/// Take an incremental snapshot using damage tracking with compact line format.
fn snapshot_damaged(
    term: &mut Term<TerminalListener>,
    palette: &[Option<Rgb>; 269],
) -> GridUpdate {
    let start = std::time::Instant::now();
    // Get cursor info before damage() borrows term
    let content = term.renderable_content();
    let cursor_point = content.cursor.point;
    let display_offset = content.display_offset;
    let (cursor_shape_str, cursor_shape_visible) = match content.cursor.shape {
        alacritty_terminal::vte::ansi::CursorShape::Block => ("block", true),
        alacritty_terminal::vte::ansi::CursorShape::Underline => ("underline", true),
        alacritty_terminal::vte::ansi::CursorShape::Beam => ("bar", true),
        _ => ("block", true),
    };
    let cursor_visible = cursor_shape_visible
        && display_offset == 0
        && term.mode().contains(alacritty_terminal::term::TermMode::SHOW_CURSOR);
    drop(content);
    let grid = term.grid();
    let cols = grid.columns();
    let rows = grid.screen_lines();
    let damage = term.damage();
    let is_full = matches!(damage, alacritty_terminal::term::TermDamage::Full);
    let damaged_lines: Vec<usize> = match damage {
        alacritty_terminal::term::TermDamage::Full => {
            (0..rows).collect()
        }
        alacritty_terminal::term::TermDamage::Partial(iter) => {
            iter.filter(|d| d.is_damaged())
                .map(|d| d.line)
                .collect()
        }
    };
    let grid = term.grid(); // re-borrow after damage()
    let mut lines = Vec::with_capacity(damaged_lines.len());
    for &row_idx in &damaged_lines {
        if row_idx >= rows {
            continue;
        }
        let line = Line(row_idx as i32 - display_offset as i32);
        let row = &grid[line];
        lines.push(row_to_compact_line(row_idx, row, cols, palette));
    }
    term.reset_damage();
    let elapsed = start.elapsed();
    let text_bytes: u32 = lines.iter().map(|l| l.text.len() as u32).sum();
    let span_count: u16 = lines.iter().map(|l| l.spans.len() as u16).sum();
    let line_count = lines.len() as u16;
    GridUpdate {
        cols: cols as u16,
        rows: rows as u16,
        cursor_col: cursor_point.column.0 as u16,
        cursor_row: cursor_point.line.0 as u16,
        cursor_visible,
        cursor_shape: cursor_shape_str.to_string(),
        lines,
        full: is_full,
        mode: term.mode().bits(),
        display_offset,
        selection: None,
    }
}
// ── Grid (DOM text) Emission Loop ───────────────────────────────────────
/// Background thread that snapshots the terminal grid as styled text runs
/// and emits GridUpdate events to the frontend for DOM rendering.
fn grid_emission_loop(
    id: &str,
    term: &Arc<FairMutex<Term<TerminalListener>>>,
    app_handle: &AppHandle,
    wakeup_rx: mpsc::Receiver<()>,
    palette: [Option<Rgb>; 269],
    force_full_render: Arc<std::sync::atomic::AtomicBool>,
) {
    let event_name = format!("terminal:grid:{}", id);
    let min_frame_interval = Duration::from_millis(16);
    let mut last_emit = std::time::Instant::now() - min_frame_interval;
    loop {
        // Block on wakeup channel -- both PTY events and scroll use this
        match wakeup_rx.recv() {
            Ok(()) => {}
            Err(_) => break,
        }
        // Rate limit: ensure at least 16ms between emissions
        let since_last = last_emit.elapsed();
        if since_last < min_frame_interval {
            let wait = min_frame_interval - since_last;
            let deadline = std::time::Instant::now() + wait;
            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() { break; }
                match wakeup_rx.recv_timeout(remaining) {
                    Ok(()) => continue,
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        }
        // Adaptive batch window: 4ms normally, extended during high-throughput
        let mut wakeup_count = 0u32;
        let batch_deadline = std::time::Instant::now() + Duration::from_millis(4);
        loop {
            let remaining = batch_deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() { break; }
            match wakeup_rx.recv_timeout(remaining) {
                Ok(()) => { wakeup_count += 1; continue; }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        // During high-throughput bursts, wait longer to accumulate changes
        if wakeup_count > 10 {
            let burst_deadline = std::time::Instant::now() + Duration::from_millis(30);
            loop {
                let remaining = burst_deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() { break; }
                match wakeup_rx.recv_timeout(remaining) {
                    Ok(()) => continue,
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        }
        // Snapshot the grid
        let force_full = force_full_render.swap(false, std::sync::atomic::Ordering::Relaxed);
        let update = {
            let mut term = term.lock_unfair();
            if force_full {
                // Consume and discard damage, take full snapshot
                let _ = term.damage();
                term.reset_damage();
                snapshot_grid(&term, &palette, true)
            } else {
                snapshot_damaged(&mut term, &palette)
            }
        };
        // Only emit if there are lines to send
        if !update.lines.is_empty() {
            let _ = app_handle.emit(&event_name, &update);
        }
        last_emit = std::time::Instant::now();
    }
}
