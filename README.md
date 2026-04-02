# tauri-plugin-terminal

Terminal emulation plugin for [Tauri v2](https://tauri.app), powered by [`alacritty_terminal`](https://github.com/alacritty/alacritty).

Provides full PTY management with DOM-based grid rendering. Terminals are created, resized, and destroyed through Tauri commands. Grid updates are emitted as events with compact line data that your frontend renders as styled DOM elements.

## Features

- Full PTY lifecycle management (create, resize, kill)
- Alacritty terminal emulation (VT100/xterm compatible)
- DOM-based rendering via compact grid events (text + style spans)
- Proper terminal grid resize (both PTY and grid, matching upstream Alacritty)
- Shell detection (zsh, bash, sh)
- Custom command execution with arguments
- Scroll support (scrollback history)
- Text selection
- Foreground process detection and signaling (Ctrl+C)
- Custom environment variables per terminal
- SIGPIPE handling for process safety

## Installation

### Rust

Add to your `src-tauri/Cargo.toml`:

```toml
[dependencies]
tauri-plugin-terminal = "0.1"
```

### JavaScript/TypeScript

```bash
npm install @alakazamlabs/tauri-plugin-terminal
```

## Usage

### Rust (src-tauri/lib.rs)

```rust
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_terminal::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

### Frontend

```typescript
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

// Create a terminal
const { id } = await invoke('plugin:terminal|terminal_create', {
  cwd: '/path/to/directory',
  command: 'bash',  // optional — defaults to user's shell
  args: [],         // optional
  cols: 80,         // optional
  rows: 24,         // optional
  id: 'my-term',    // optional — auto-generated UUID if omitted
  env: {            // optional — custom env vars
    MY_VAR: 'value',
  },
})

// Listen for grid updates (render these as DOM)
const unlisten = await listen(`terminal:grid:${id}`, (event) => {
  const grid = event.payload
  // grid.lines: CompactLine[] — each has { row, text, spans: StyleSpan[] }
  // grid.cols, grid.rows — terminal dimensions
  // grid.cursor_col, grid.cursor_row, grid.cursor_visible
  // grid.display_offset — scroll position
  renderGrid(grid)
})

// Send keyboard input
await invoke('plugin:terminal|terminal_write', { id, data: 'ls -la\r' })

// Resize (call this when your container changes size)
await invoke('plugin:terminal|terminal_resize', { id, cols: 120, rows: 40 })

// Scroll
await invoke('plugin:terminal|terminal_scroll', { id, delta: -5 }) // scroll up

// Kill
await invoke('plugin:terminal|terminal_kill', { id })
```

### Grid Update Format

Each `terminal:grid:{id}` event contains:

```typescript
interface GridUpdate {
  cols: number        // terminal width in columns
  rows: number        // terminal height in rows
  cursor_col: number  // cursor column position
  cursor_row: number  // cursor row position
  cursor_visible: boolean
  cursor_shape: string // "block" | "underline" | "beam"
  lines: CompactLine[]
  full: boolean       // true = full redraw, false = incremental
  mode: number        // terminal mode bits
  display_offset: number // scroll offset (0 = bottom)
}

interface CompactLine {
  row: number         // row index (0 = top)
  text: string        // plain text content
  spans: StyleSpan[]  // style info for non-default cells
}

interface StyleSpan {
  s: number           // start column (inclusive)
  e: number           // end column (inclusive)
  fg?: number         // foreground color (0xRRGGBB), omitted if default
  bg?: number         // background color (0xRRGGBB), omitted if default
  fl?: number         // flags: 1=bold, 2=italic, 4=underline, 8=strikethrough,
                      //        16=inverse, 32=dim, 64=hidden, 128=wide char
}
```

## React Component

A ready-to-use React component is available in the `webview-src/` directory. See `webview-src/TerminalView.tsx` for a complete DOM-based terminal renderer.

## How It Works

1. **PTY Creation**: `terminal_create` spawns a PTY with the user's shell (or custom command) using `alacritty_terminal`'s TTY layer.

2. **Grid Emission**: A background thread watches the terminal for output. When content changes, it snapshots the terminal grid into compact lines (text + style spans) and emits a `terminal:grid:{id}` Tauri event.

3. **Resize**: `terminal_resize` does two things (matching upstream Alacritty):
   - Sends `ioctl(TIOCSWINSZ)` to the PTY fd (kernel delivers SIGWINCH to the process)
   - Calls `term.resize()` to reflow the terminal grid content

4. **Input**: `terminal_write` sends raw bytes to the PTY. The frontend is responsible for converting keyboard events to the appropriate escape sequences.

## License

MIT - [Alakazam Labs](https://alakazamlabs.com)
