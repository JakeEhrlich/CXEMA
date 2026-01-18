# CXEMA

A mostly faithful remake of [KOHCTPYKTOP: Engineer of the People](https://www.zachtronics.com/kohctpyktop-engineer-of-the-people/), the Flash game by Zachtronics about designing integrated circuits.

## About

CXEMA recreates the core circuit design experience of the original game while adding quality-of-life improvements, including:

- **Vim-like visual mode** with hjkl navigation and modal editing
- **Prefix commands** for filtered operations (e.g., `sd` to delete only silicon, `md` to delete only metal)
- **Fast navigation** with `w`/`b` (move by 4), `e` (find material), `gg`/`ge` (top/bottom), and half-screen jumps
- **Keyboard shortcuts** alongside mouse controls for all operations
- **Context-sensitive help** showing available commands

## Controls

### Mouse Modes (1-7, 9)
- `1` - Place N-type silicon (red)
- `2` - Place P-type silicon (yellow)
- `3` - Place metal
- `4` - Place/delete vias
- `5` - Delete metal
- `6` - Delete silicon
- `7` - Delete all
- `9` - Mouse selection mode (click and drag to select)

### Visual Mode (8)
- `hjkl` / arrow keys - Move cursor
- `w` / `b` - Move right/left by 4
- `e` - Move to next material
- `Ctrl+A` / `Ctrl+E` - Move to left/right edge
- `gg` / `ge` - Go to top/bottom row
- `gd` / `gu` - Half-screen down/up
- `gh` / `gb` - Half-screen right/left
- `v` - Start selection (keyboard)
- `-` - Draw N-silicon
- `+` - Draw P-silicon
- `=` - Draw metal
- `.` - Toggle via
- `d` - Delete all at cursor/selection
- `sd` - Delete only silicon
- `md` - Delete only metal
- `se` / `me` - Find next silicon/metal

### Snippets (Copy/Paste)
- `y` - Yank (copy) selection and save as snippet
- `p` - Paste snippet at cursor
- `r` - Rotate snippet 90° clockwise
- `Shift+←/→` - Switch tabs (navigate to Snippets tab)
- `Shift+↑/↓` - Select snippet in list

## Building

Requires Rust. Clone and run:

```
cargo run
```

### Command Line Options

```
cargo run [snippets_directory]
```

- `snippets_directory` - Path to store/load snippets (default: `.snippits`)

## Credits

Original game by [Zachtronics](https://www.zachtronics.com/).
