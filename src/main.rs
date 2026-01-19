use minifb::{Key, Window, WindowOptions};
use rodio::{Decoder, OutputStream, Sink, Source};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

// Grid dimensions (matches original game)
// 44 wide: 1 blank + 3 pin + 36 playable + 3 pin + 1 blank
// 27 high: 2 blank + (6 pins × 4 stride) + 2 blank
const GRID_WIDTH: usize = 44;
const GRID_HEIGHT: usize = 27;
const CELL_SIZE: usize = 16;

// Pin layout: 3x3 pins, left at x=1, right at x=40
const PIN_SIZE: usize = 3;

// UI Panel on right side
const PANEL_WIDTH: usize = 80;
const BUTTON_HEIGHT: usize = 44;
const BUTTON_MARGIN: usize = 4;

// Bottom area for help text, tabs, and content panel
const HELP_AREA_HEIGHT: usize = 20;
const TAB_HEIGHT: usize = 24;
const TAB_CONTENT_HEIGHT: usize = 230;  // Content area below tabs (increased for specs)
const BOTTOM_AREA_HEIGHT: usize = HELP_AREA_HEIGHT + TAB_HEIGHT + TAB_CONTENT_HEIGHT;

// Window size includes grid + panel + bottom area
const GRID_PIXEL_WIDTH: usize = GRID_WIDTH * CELL_SIZE;
const GRID_PIXEL_HEIGHT: usize = GRID_HEIGHT * CELL_SIZE;
const WINDOW_WIDTH: usize = GRID_PIXEL_WIDTH + PANEL_WIDTH;
const WINDOW_HEIGHT: usize = GRID_PIXEL_HEIGHT + BOTTOM_AREA_HEIGHT;

// Colors (0xRRGGBB format)
const COLOR_BACKGROUND: u32 = 0x4a4a4a;
const COLOR_GRID_LINE: u32 = 0x3a3a3a;
const COLOR_CELL_DARK: u32 = 0x5a5a5a;
const COLOR_CELL_MID: u32 = 0x6a6a6a;
const COLOR_CELL_LIGHT: u32 = 0x7a7a7a;

const COLOR_N_TYPE: u32 = 0x882222; // Red/brown - N-type silicon
const COLOR_P_TYPE: u32 = 0xdddd44; // Yellow - P-type silicon
const COLOR_METAL: u32 = 0xeeeeee;  // Light gray - Metal layer
const COLOR_OUTLINE: u32 = 0x000000; // Black outline
const COLOR_VIA: u32 = 0x111111;    // Dark circle for vias

// Darker gate colors (for the interrupting silicon in a gate)
const COLOR_N_GATE: u32 = 0x661818; // Darker red for N-type gate
const COLOR_P_GATE: u32 = 0xaaaa33; // Darker yellow for P-type gate

// Metal transparency (0.0 = fully transparent, 1.0 = fully opaque)
const METAL_ALPHA: f32 = 0.5;

// Pin label color
const COLOR_PIN_TEXT: u32 = 0x404040;    // Dark gray text

// Editor colors
const COLOR_CURSOR: u32 = 0x00ff00;       // Green cursor
const COLOR_SELECTION: u32 = 0x0066ff;    // Blue selection
const COLOR_PATH_PREVIEW: u32 = 0xffffff; // White path preview
const COLOR_SOURCE_POINT: u32 = 0xff00ff; // Magenta source point

// Panel colors
const COLOR_PANEL_BG: u32 = 0x606060;     // Panel background
const COLOR_BUTTON_BG: u32 = 0x808080;    // Button background
const COLOR_BUTTON_LIGHT: u32 = 0xa0a0a0; // Button highlight edge
const COLOR_BUTTON_DARK: u32 = 0x505050;  // Button shadow edge
const COLOR_BUTTON_ACTIVE: u32 = 0x993333; // Active button border (red)
const COLOR_BUTTON_TEXT: u32 = 0x000000;  // Button text
const COLOR_DELETE_X: u32 = 0xcc3333;     // Red X for delete buttons

// Bottom area colors
const COLOR_HELP_BG: u32 = 0x505050;      // Help area background
const COLOR_HELP_TEXT: u32 = 0xcccccc;    // Help text color
const COLOR_TAB_BG: u32 = 0x707070;       // Inactive tab background
const COLOR_TAB_ACTIVE_BG: u32 = 0x909090; // Active tab background
const COLOR_TAB_TEXT: u32 = 0x000000;     // Tab text color

// Key repeat timing (in milliseconds)
const KEY_REPEAT_DELAY_MS: u64 = 400;  // Initial delay before repeat starts
const KEY_REPEAT_RATE_MS: u64 = 50;    // Rate of repeat once started

// ============================================================================
// Data Structures
// ============================================================================

/// Edit modes for construction
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditMode {
    NSilicon,      // 1 - Place N-type silicon
    PSilicon,      // 2 - Place P-type silicon
    Metal,         // 3 - Place metal wire
    Via,           // 4 - Place/delete via
    DeleteMetal,   // 5 - Delete metal only
    DeleteSilicon, // 6 - Delete silicon and via
    DeleteAll,     // 7 - Delete everything
    Visual,        // 8 - Vim-like visual mode
    MouseSelect,   // 9 - Mouse-based selection mode
}

/// Visual mode sub-states
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VisualState {
    Normal,        // Just cursor, no selection
    Selecting,     // 'v' pressed, selecting area
    PlacingN,      // '-' pressed - placing N silicon
    PlacingP,      // '+' pressed - placing P silicon
    PlacingMetal,  // '=' pressed - placing metal
}

/// Pending prefix modifier for visual mode commands
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingModifier {
    None,
    Silicon,  // 's' - filter to silicon
    Metal,    // 'm' - filter to metal
    Goto,     // 'g' - goto prefix
}

/// Bottom tabs
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Specifications,
    Verification,
    DesignSnippets,
    Designs,
    Help,
    Menu,
}

/// Music track selection
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MusicTrack {
    None,
    AnalogSequence,
    GroovyBeat,
    RetroLoop,
}

impl MusicTrack {
    fn filename(&self) -> Option<&'static str> {
        match self {
            MusicTrack::None => None,
            MusicTrack::AnalogSequence => Some("music/analog_sequence.wav"),
            MusicTrack::GroovyBeat => Some("music/groovy_beat.wav"),
            MusicTrack::RetroLoop => Some("music/retro_loop.wav"),
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            MusicTrack::None => "No Music",
            MusicTrack::AnalogSequence => "Analog Sequence",
            MusicTrack::GroovyBeat => "Groovy Beat",
            MusicTrack::RetroLoop => "Retro Loop",
        }
    }

    fn next(&self) -> MusicTrack {
        match self {
            MusicTrack::None => MusicTrack::AnalogSequence,
            MusicTrack::AnalogSequence => MusicTrack::GroovyBeat,
            MusicTrack::GroovyBeat => MusicTrack::RetroLoop,
            MusicTrack::RetroLoop => MusicTrack::RetroLoop, // Stay at end
        }
    }

    fn prev(&self) -> MusicTrack {
        match self {
            MusicTrack::None => MusicTrack::None, // Stay at beginning
            MusicTrack::AnalogSequence => MusicTrack::None,
            MusicTrack::GroovyBeat => MusicTrack::AnalogSequence,
            MusicTrack::RetroLoop => MusicTrack::GroovyBeat,
        }
    }
}

/// Dialog state for text input
#[derive(Clone, Debug, PartialEq, Eq)]
enum DialogState {
    None,
    SaveSnippet { name: String },  // Entering name for a new snippet
    SaveDesign { name: String },   // Entering name for a new design
}

/// A saved design snippet - a rectangular region of circuit
#[derive(Clone)]
struct Snippet {
    name: String,
    width: usize,
    height: usize,
    // Stored as relative coordinates from top-left
    nodes: Vec<Vec<Node>>,
    // Horizontal edges (width-1 x height)
    h_edges: Vec<Vec<Edge>>,
    // Vertical edges (width x height-1)
    v_edges: Vec<Vec<Edge>>,
}

/// Editor state for construction
struct EditorState {
    mode: EditMode,
    visual_state: VisualState,
    pending_modifier: PendingModifier,
    active_tab: Tab,
    dialog: DialogState,

    // Cursor position (grid coordinates)
    cursor_x: usize,
    cursor_y: usize,

    // Selection anchor (for visual mode)
    selection_anchor: Option<(usize, usize)>,

    // Mouse selection end point (for finalized mouse selections in mode 9)
    mouse_selection_end: Option<(usize, usize)>,

    // Path construction state (for mouse-based modes)
    path_start: Option<(usize, usize)>,
    current_path: Vec<(usize, usize)>,

    // Mouse position (grid coordinates)
    mouse_grid_x: Option<usize>,
    mouse_grid_y: Option<usize>,

    // Snippets
    snippets: Vec<Snippet>,
    selected_snippet: usize,
    snippets_dir: PathBuf,

    // Designs (full circuit saves)
    designs: Vec<Snippet>,  // Reuse Snippet struct for full designs
    selected_design: usize,
    designs_dir: PathBuf,

    // Levels (verification challenges)
    levels: Vec<Level>,
    selected_level: usize,
    level_scroll_offset: usize,
    levels_dir: PathBuf,

    // Save data (completion tracking, achievements)
    save_data: SaveData,
    save_path: PathBuf,

    // Yank buffer for paste operations
    yank_buffer: Option<Snippet>,

    // Music selection
    current_music: MusicTrack,
    music_changed: bool, // Flag to signal main loop to update music playback
}

impl EditorState {
    fn new(snippets_dir: PathBuf, designs_dir: PathBuf, levels_dir: PathBuf, save_path: PathBuf) -> Self {
        let save_data = SaveData::load(&save_path);
        Self {
            mode: EditMode::Visual,  // Start in visual mode
            visual_state: VisualState::Normal,
            pending_modifier: PendingModifier::None,
            active_tab: Tab::Help,   // Start on Help tab
            dialog: DialogState::None,
            cursor_x: GRID_WIDTH / 2,
            cursor_y: GRID_HEIGHT / 2,
            selection_anchor: None,
            mouse_selection_end: None,
            path_start: None,
            current_path: Vec::new(),
            mouse_grid_x: None,
            mouse_grid_y: None,
            snippets: Vec::new(),
            selected_snippet: 0,
            snippets_dir,
            designs: Vec::new(),
            selected_design: 0,
            designs_dir,
            levels: Vec::new(),
            selected_level: 0,
            level_scroll_offset: 0,
            levels_dir,
            save_data,
            save_path,
            yank_buffer: None,
            current_music: MusicTrack::None,
            music_changed: false,
        }
    }

    fn clear_modifier(&mut self) {
        self.pending_modifier = PendingModifier::None;
    }

    fn get_selection(&self) -> Option<(usize, usize, usize, usize)> {
        if let Some((ax, ay)) = self.selection_anchor {
            // For finalized mouse selection, use stored end point
            // Otherwise use current cursor position
            let (bx, by) = self.mouse_selection_end.unwrap_or((self.cursor_x, self.cursor_y));
            let min_x = ax.min(bx);
            let max_x = ax.max(bx);
            let min_y = ay.min(by);
            let max_y = ay.max(by);
            Some((min_x, min_y, max_x, max_y))
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
struct Pin {
    label: String,
    x: usize,        // Grid x position
    y: usize,        // Grid y position
}

impl Pin {
    fn new(label: &str, x: usize, y: usize) -> Self {
        Self {
            label: label.to_string(),
            x,
            y,
        }
    }

    /// Check if a grid coordinate is within this pin's 3x3 area
    fn contains(&self, x: usize, y: usize) -> bool {
        x >= self.x && x < self.x + PIN_SIZE && y >= self.y && y < self.y + PIN_SIZE
    }
}

/// Check if a grid coordinate is within any pin's 3x3 area
fn is_pin_cell(x: usize, y: usize, pins: &[Pin]) -> bool {
    pins.iter().any(|pin| pin.contains(x, y))
}

/// Get corner fill flags for a pin cell based on diagonal neighbors
/// Returns [top_left, top_right, bottom_left, bottom_right]
/// A corner should be filled if the diagonal neighbor is also a pin cell
fn get_pin_corner_fills(x: usize, y: usize, pins: &[Pin]) -> [bool; 4] {
    [
        // Top-left: check (x-1, y-1)
        x > 0 && y > 0 && is_pin_cell(x - 1, y - 1, pins),
        // Top-right: check (x+1, y-1)
        y > 0 && is_pin_cell(x + 1, y - 1, pins),
        // Bottom-left: check (x-1, y+1)
        x > 0 && is_pin_cell(x - 1, y + 1, pins),
        // Bottom-right: check (x+1, y+1)
        is_pin_cell(x + 1, y + 1, pins),
    ]
}

/// A waveform is a sequence of 0/1 values over discrete time steps
#[derive(Clone, Debug, Deserialize)]
struct Waveform {
    pin_index: usize,      // Which pin (0-11)
    is_input: bool,        // true = input to circuit, false = expected output
    values: String,        // Signal values as string of '0' and '1' chars
    #[serde(default)]
    test: String,          // Test mask: '?' = check, 'x' = don't care (empty = check all)
    #[serde(default = "default_true")]
    display: bool,         // Whether to show in verification tab (default: true)
}

fn default_true() -> bool { true }

/// A level defines a verification challenge
#[derive(Clone, Debug, Deserialize)]
struct Level {
    name: String,
    #[serde(default)]
    order: usize,                 // For sorting levels in menu
    pins: Vec<String>,            // Labels for all 12 pins
    waveforms: Vec<Waveform>,     // Input and expected output waveforms
    #[serde(default)]
    specification: Vec<String>,   // Lines of text explaining the level
    #[serde(default = "default_accuracy_threshold")]
    accuracy_threshold: f32,      // Required accuracy to pass (0.97-0.98 typically)
}

fn default_accuracy_threshold() -> f32 {
    0.98
}

impl Level {
    /// Load a level from a JSON file
    fn load(path: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }
}

/// Save data for tracking progress
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct SaveData {
    completed_levels: Vec<String>,  // Names of completed levels
    #[serde(default)]
    achievements: Vec<String>,      // For future use
}

impl SaveData {
    fn load(path: &Path) -> Self {
        if let Ok(content) = std::fs::read_to_string(path) {
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    fn save(&self, path: &Path) {
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, content);
        }
    }

    fn is_level_complete(&self, level_name: &str) -> bool {
        self.completed_levels.contains(&level_name.to_string())
    }

    fn mark_level_complete(&mut self, level_name: &str) {
        if !self.is_level_complete(level_name) {
            self.completed_levels.push(level_name.to_string());
        }
    }
}

/// Load all levels from a directory
fn load_all_levels(dir: &Path) -> Vec<Level> {
    let mut levels = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Some(level) = Level::load(&path) {
                    levels.push(level);
                }
            }
        }
    }
    // Sort by order (from filename prefix)
    levels.sort_by_key(|l| l.order);
    levels
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SiliconKind {
    N,
    P,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Silicon {
    #[default]
    None,
    N,
    P,
    Gate { channel: SiliconKind }, // channel is N or P, gate is the opposite
}

#[derive(Clone, Copy, Debug, Default)]
struct Node {
    silicon: Silicon,
    metal: bool,
    via: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Edge {
    n_silicon: bool, // N-type silicon crosses this edge
    p_silicon: bool, // P-type silicon crosses this edge
    metal: bool,     // Metal crosses this edge
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layer {
    NSilicon,
    PSilicon,
    Metal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// The circuit grid with nodes and edges
struct Circuit {
    nodes: [[Node; GRID_WIDTH]; GRID_HEIGHT],
    // Horizontal edges: between (x,y) and (x+1,y)
    h_edges: [[Edge; GRID_WIDTH - 1]; GRID_HEIGHT],
    // Vertical edges: between (x,y) and (x,y+1)
    v_edges: [[Edge; GRID_WIDTH]; GRID_HEIGHT - 1],
}

impl Circuit {
    fn new() -> Self {
        Self {
            nodes: [[Node::default(); GRID_WIDTH]; GRID_HEIGHT],
            h_edges: [[Edge::default(); GRID_WIDTH - 1]; GRID_HEIGHT],
            v_edges: [[Edge::default(); GRID_WIDTH]; GRID_HEIGHT - 1],
        }
    }

    fn get_node(&self, x: usize, y: usize) -> Option<&Node> {
        self.nodes.get(y).and_then(|row| row.get(x))
    }

    fn get_node_mut(&mut self, x: usize, y: usize) -> Option<&mut Node> {
        self.nodes.get_mut(y).and_then(|row| row.get_mut(x))
    }

    /// Get the edge between two adjacent cells (order doesn't matter)
    fn get_edge(&self, x1: usize, y1: usize, x2: usize, y2: usize) -> Option<&Edge> {
        // Normalize so we always access in consistent order
        let (x1, y1, x2, y2) = if (y1, x1) > (y2, x2) {
            (x2, y2, x1, y1)
        } else {
            (x1, y1, x2, y2)
        };

        if y1 == y2 && x2 == x1 + 1 {
            // Horizontal edge
            self.h_edges.get(y1).and_then(|row| row.get(x1))
        } else if x1 == x2 && y2 == y1 + 1 {
            // Vertical edge
            self.v_edges.get(y1).and_then(|row| row.get(x1))
        } else {
            None // Not adjacent
        }
    }

    fn get_edge_mut(&mut self, x1: usize, y1: usize, x2: usize, y2: usize) -> Option<&mut Edge> {
        let (x1, y1, x2, y2) = if (y1, x1) > (y2, x2) {
            (x2, y2, x1, y1)
        } else {
            (x1, y1, x2, y2)
        };

        if y1 == y2 && x2 == x1 + 1 {
            self.h_edges.get_mut(y1).and_then(|row| row.get_mut(x1))
        } else if x1 == x2 && y2 == y1 + 1 {
            self.v_edges.get_mut(y1).and_then(|row| row.get_mut(x1))
        } else {
            None
        }
    }

    /// Set connection on a specific layer between two adjacent cells
    fn set_edge(&mut self, x1: usize, y1: usize, x2: usize, y2: usize, layer: Layer, connected: bool) {
        if let Some(edge) = self.get_edge_mut(x1, y1, x2, y2) {
            match layer {
                Layer::NSilicon => edge.n_silicon = connected,
                Layer::PSilicon => edge.p_silicon = connected,
                Layer::Metal => edge.metal = connected,
            }
        }
    }

    /// Check if a layer is connected in a given direction from (x, y)
    fn is_connected(&self, x: usize, y: usize, dir: Direction, layer: Layer) -> bool {
        let (nx, ny) = match dir {
            Direction::Up => {
                if y == 0 { return false; }
                (x, y - 1)
            }
            Direction::Down => (x, y + 1),
            Direction::Left => {
                if x == 0 { return false; }
                (x - 1, y)
            }
            Direction::Right => (x + 1, y),
        };

        self.get_edge(x, y, nx, ny)
            .map(|e| match layer {
                Layer::NSilicon => e.n_silicon,
                Layer::PSilicon => e.p_silicon,
                Layer::Metal => e.metal,
            })
            .unwrap_or(false)
    }
}

// ============================================================================
// Circuit Simulation
// ============================================================================

/// Simulation state for the circuit
struct SimState {
    metal_high: [[bool; GRID_WIDTH]; GRID_HEIGHT],
    n_silicon_high: [[bool; GRID_WIDTH]; GRID_HEIGHT],
    p_silicon_high: [[bool; GRID_WIDTH]; GRID_HEIGHT],
    gate_open: [[bool; GRID_WIDTH]; GRID_HEIGHT],  // From previous tick
    tick: usize,
    output_history: Vec<[bool; 12]>,  // History of output pin values for each tick
    // Last simulation result
    last_accuracy: Option<f32>,
    last_passed: Option<bool>,
}

impl SimState {
    fn new() -> Self {
        Self {
            metal_high: [[false; GRID_WIDTH]; GRID_HEIGHT],
            n_silicon_high: [[false; GRID_WIDTH]; GRID_HEIGHT],
            p_silicon_high: [[false; GRID_WIDTH]; GRID_HEIGHT],
            gate_open: [[false; GRID_WIDTH]; GRID_HEIGHT],
            tick: 0,
            output_history: Vec::new(),
            last_accuracy: None,
            last_passed: None,
        }
    }

    /// Initialize gate states based on circuit topology
    /// P-channel gates start open (they conduct when gate signal is LOW)
    /// N-channel gates start closed (they conduct when gate signal is HIGH)
    fn init_gates(&mut self, circuit: &Circuit) {
        for y in 0..GRID_HEIGHT {
            for x in 0..GRID_WIDTH {
                if let Some(node) = circuit.get_node(x, y) {
                    if let Silicon::Gate { channel } = node.silicon {
                        // P-channel gates are open when gate signal is LOW (initial state)
                        // N-channel gates are closed when gate signal is LOW (initial state)
                        self.gate_open[y][x] = matches!(channel, SiliconKind::P);
                    }
                }
            }
        }
    }

    /// Run one simulation tick
    /// pin_values: for each pin index (0-11), whether it's driven high
    fn step(&mut self, circuit: &Circuit, pins: &[Pin], pin_values: &[bool; 12]) {
        // Step 1: Propagate signals using gate states from previous tick
        self.propagate(circuit, pins, pin_values);

        // Step 2: Update gate states for next tick based on current signals
        self.update_gates(circuit);

        // Step 3: Record output pin values for history
        let mut output_values = [false; 12];
        for (i, pin) in pins.iter().enumerate() {
            if i < 12 {
                output_values[i] = self.metal_high[pin.y][pin.x];
            }
        }
        self.output_history.push(output_values);

        self.tick += 1;
    }

    /// Propagate signals through the circuit
    fn propagate(&mut self, circuit: &Circuit, pins: &[Pin], pin_values: &[bool; 12]) {
        // Reset all signals to low
        self.metal_high = [[false; GRID_WIDTH]; GRID_HEIGHT];
        self.n_silicon_high = [[false; GRID_WIDTH]; GRID_HEIGHT];
        self.p_silicon_high = [[false; GRID_WIDTH]; GRID_HEIGHT];

        // Queue for BFS propagation
        let mut metal_queue: VecDeque<(usize, usize)> = VecDeque::new();
        let mut n_silicon_queue: VecDeque<(usize, usize)> = VecDeque::new();
        let mut p_silicon_queue: VecDeque<(usize, usize)> = VecDeque::new();

        // Drive pins according to waveform values
        for (i, pin) in pins.iter().enumerate() {
            if i < 12 && pin_values[i] {
                // Pin is high - drive all metal cells in the 3x3 pin area
                for dy in 0..PIN_SIZE {
                    for dx in 0..PIN_SIZE {
                        let x = pin.x + dx;
                        let y = pin.y + dy;
                        if x < GRID_WIDTH && y < GRID_HEIGHT {
                            if !self.metal_high[y][x] {
                                self.metal_high[y][x] = true;
                                metal_queue.push_back((x, y));
                            }
                        }
                    }
                }
            }
        }

        // Iterate until no changes (fixed-point)
        // This is needed because: metal -> via -> silicon -> via -> metal -> ...
        loop {
            let mut changed = false;

            // Propagate metal signals
            while let Some((x, y)) = metal_queue.pop_front() {
                // Check all 4 neighbors for metal connections
                let neighbors = [
                    (x.wrapping_sub(1), y, true),  // left, horizontal edge
                    (x + 1, y, true),              // right, horizontal edge
                    (x, y.wrapping_sub(1), false), // up, vertical edge
                    (x, y + 1, false),             // down, vertical edge
                ];

                for (nx, ny, is_horizontal) in neighbors {
                    if nx >= GRID_WIDTH || ny >= GRID_HEIGHT {
                        continue;
                    }

                    // Check if there's a metal edge connection
                    let has_edge = if is_horizontal {
                        let edge_x = x.min(nx);
                        circuit.h_edges.get(y).and_then(|row| row.get(edge_x)).map(|e| e.metal).unwrap_or(false)
                    } else {
                        let edge_y = y.min(ny);
                        circuit.v_edges.get(edge_y).and_then(|row| row.get(x)).map(|e| e.metal).unwrap_or(false)
                    };

                    if has_edge && !self.metal_high[ny][nx] {
                        self.metal_high[ny][nx] = true;
                        metal_queue.push_back((nx, ny));
                        changed = true;
                    }
                }

                // If there's a via here, propagate to silicon
                if let Some(node) = circuit.get_node(x, y) {
                    if node.via {
                        match node.silicon {
                            Silicon::N => {
                                if !self.n_silicon_high[y][x] {
                                    self.n_silicon_high[y][x] = true;
                                    n_silicon_queue.push_back((x, y));
                                    changed = true;
                                }
                            }
                            Silicon::P => {
                                if !self.p_silicon_high[y][x] {
                                    self.p_silicon_high[y][x] = true;
                                    p_silicon_queue.push_back((x, y));
                                    changed = true;
                                }
                            }
                            Silicon::Gate { channel } => {
                                // Via on a gate - signal goes to the channel type
                                match channel {
                                    SiliconKind::N => {
                                        if !self.n_silicon_high[y][x] {
                                            self.n_silicon_high[y][x] = true;
                                            n_silicon_queue.push_back((x, y));
                                            changed = true;
                                        }
                                    }
                                    SiliconKind::P => {
                                        if !self.p_silicon_high[y][x] {
                                            self.p_silicon_high[y][x] = true;
                                            p_silicon_queue.push_back((x, y));
                                            changed = true;
                                        }
                                    }
                                }
                            }
                            Silicon::None => {}
                        }
                    }
                }
            }

            // Propagate N-silicon signals
            while let Some((x, y)) = n_silicon_queue.pop_front() {
                if self.propagate_silicon(circuit, x, y, SiliconKind::N, &mut n_silicon_queue, &mut metal_queue) {
                    changed = true;
                }
            }

            // Propagate P-silicon signals
            while let Some((x, y)) = p_silicon_queue.pop_front() {
                if self.propagate_silicon(circuit, x, y, SiliconKind::P, &mut p_silicon_queue, &mut metal_queue) {
                    changed = true;
                }
            }

            if !changed {
                break;
            }
        }
    }

    /// Propagate silicon signal from a node
    /// Returns true if any changes were made
    fn propagate_silicon(&mut self, circuit: &Circuit, x: usize, y: usize, kind: SiliconKind, queue: &mut VecDeque<(usize, usize)>, metal_queue: &mut VecDeque<(usize, usize)>) -> bool {
        let mut changed = false;

        // If this silicon node has a via and metal isn't already high, propagate up to metal
        if let Some(node) = circuit.get_node(x, y) {
            if node.via && !self.metal_high[y][x] {
                self.metal_high[y][x] = true;
                metal_queue.push_back((x, y));
                changed = true;
            }
        }

        let neighbors = [
            (x.wrapping_sub(1), y, true),  // left
            (x + 1, y, true),              // right
            (x, y.wrapping_sub(1), false), // up
            (x, y + 1, false),             // down
        ];

        for (nx, ny, is_horizontal) in neighbors {
            if nx >= GRID_WIDTH || ny >= GRID_HEIGHT {
                continue;
            }

            // Check if there's a silicon edge connection of this type
            let has_edge = if is_horizontal {
                let edge_x = x.min(nx);
                circuit.h_edges.get(y).and_then(|row| row.get(edge_x)).map(|e| {
                    match kind {
                        SiliconKind::N => e.n_silicon,
                        SiliconKind::P => e.p_silicon,
                    }
                }).unwrap_or(false)
            } else {
                let edge_y = y.min(ny);
                circuit.v_edges.get(edge_y).and_then(|row| row.get(x)).map(|e| {
                    match kind {
                        SiliconKind::N => e.n_silicon,
                        SiliconKind::P => e.p_silicon,
                    }
                }).unwrap_or(false)
            };

            if !has_edge {
                continue;
            }

            // Check if neighbor is already high
            let neighbor_high = match kind {
                SiliconKind::N => self.n_silicon_high[ny][nx],
                SiliconKind::P => self.p_silicon_high[ny][nx],
            };
            if neighbor_high {
                continue;
            }

            // Check if neighbor is a gate that blocks us
            if let Some(neighbor_node) = circuit.get_node(nx, ny) {
                if let Silicon::Gate { channel } = neighbor_node.silicon {
                    if channel == kind {
                        // This is a gate of our silicon type - check if it's open
                        if !self.gate_open[ny][nx] {
                            continue; // Gate is closed, can't propagate through
                        }
                    }
                    // If channel != kind, this gate doesn't block us (we're the gate control)
                }
            }

            // Also check if current node is a gate that blocks outgoing signal
            if let Some(current_node) = circuit.get_node(x, y) {
                if let Silicon::Gate { channel } = current_node.silicon {
                    if channel == kind {
                        // Current node is a gate of our type - check if open
                        if !self.gate_open[y][x] {
                            continue; // Can't propagate out of closed gate
                        }
                    }
                }
            }

            // Propagate signal
            match kind {
                SiliconKind::N => self.n_silicon_high[ny][nx] = true,
                SiliconKind::P => self.p_silicon_high[ny][nx] = true,
            }
            queue.push_back((nx, ny));
            changed = true;
        }

        changed
    }

    /// Update gate open/close states based on current signals
    fn update_gates(&mut self, circuit: &Circuit) {
        for y in 0..GRID_HEIGHT {
            for x in 0..GRID_WIDTH {
                if let Some(node) = circuit.get_node(x, y) {
                    if let Silicon::Gate { channel } = node.silicon {
                        // Gate signal comes from the opposite silicon type
                        let gate_signal = match channel {
                            SiliconKind::N => self.p_silicon_high[y][x], // N-channel, gate is P
                            SiliconKind::P => self.n_silicon_high[y][x], // P-channel, gate is N
                        };

                        // N-channel opens when gate is HIGH
                        // P-channel opens when gate is LOW
                        self.gate_open[y][x] = match channel {
                            SiliconKind::N => gate_signal,
                            SiliconKind::P => !gate_signal,
                        };
                    }
                }
            }
        }
    }

    /// Get the signal state at a pin location (reads metal signal)
    fn get_pin_signal(&self, pin: &Pin) -> bool {
        // Check if any cell in the pin's 3x3 area has high metal
        for dy in 0..PIN_SIZE {
            for dx in 0..PIN_SIZE {
                let x = pin.x + dx;
                let y = pin.y + dy;
                if x < GRID_WIDTH && y < GRID_HEIGHT && self.metal_high[y][x] {
                    return true;
                }
            }
        }
        false
    }
}

// ============================================================================
// BFS Pathfinding
// ============================================================================

use std::collections::VecDeque;

/// Find shortest path between two grid points using BFS
/// Avoids cells that already have the specified layer (for routing around obstacles)
fn find_path(start: (usize, usize), end: (usize, usize), circuit: &Circuit, layer: Layer) -> Vec<(usize, usize)> {
    if start == end {
        return vec![start];
    }

    let mut visited = [[false; GRID_WIDTH]; GRID_HEIGHT];
    let mut parent: [[Option<(usize, usize)>; GRID_WIDTH]; GRID_HEIGHT] =
        [[None; GRID_WIDTH]; GRID_HEIGHT];
    let mut queue = VecDeque::new();

    queue.push_back(start);
    visited[start.1][start.0] = true;

    while let Some((x, y)) = queue.pop_front() {
        if (x, y) == end {
            // Reconstruct path
            let mut path = Vec::new();
            let mut current = Some(end);
            while let Some(pos) = current {
                path.push(pos);
                current = parent[pos.1][pos.0];
            }
            path.reverse();
            return path;
        }

        // Check all 4 neighbors
        let neighbors = [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ];

        for (nx, ny) in neighbors {
            if nx < GRID_WIDTH && ny < GRID_HEIGHT && !visited[ny][nx] {
                // Check if this cell is in non-playable area (pins, borders)
                // Allow start/end points even if non-playable (they might be pins)
                let in_nonplayable = !is_playable(nx, ny) && (nx, ny) != start && (nx, ny) != end;

                // Check if this cell is blocked by existing material
                let blocked = if let Some(node) = circuit.get_node(nx, ny) {
                    match layer {
                        Layer::NSilicon => node.silicon != Silicon::None,
                        Layer::PSilicon => node.silicon != Silicon::None,
                        Layer::Metal => node.metal,
                    }
                } else {
                    true
                };

                // Allow the end point even if it's "blocked"
                if !in_nonplayable && (!blocked || (nx, ny) == end) {
                    visited[ny][nx] = true;
                    parent[ny][nx] = Some((x, y));
                    queue.push_back((nx, ny));
                }
            }
        }
    }

    Vec::new() // No path found
}

// ============================================================================
// Edit Operations
// ============================================================================

/// Check if a grid position is in the playable area (not pins or edges)
fn is_playable(x: usize, _y: usize) -> bool {
    // Playable area is columns 4-39, any row
    x >= 4 && x <= 39
}

/// Check if a cell has a straight silicon wire (connected only horizontally or only vertically)
/// Returns Some((is_horizontal, silicon_kind)) if it's a straight wire, None otherwise
fn get_straight_silicon(circuit: &Circuit, x: usize, y: usize) -> Option<(bool, SiliconKind)> {
    let node = circuit.get_node(x, y)?;

    let kind = match node.silicon {
        Silicon::N => SiliconKind::N,
        Silicon::P => SiliconKind::P,
        _ => return None,
    };

    let layer = match kind {
        SiliconKind::N => Layer::NSilicon,
        SiliconKind::P => Layer::PSilicon,
    };

    let conn_left = circuit.is_connected(x, y, Direction::Left, layer);
    let conn_right = circuit.is_connected(x, y, Direction::Right, layer);
    let conn_up = circuit.is_connected(x, y, Direction::Up, layer);
    let conn_down = circuit.is_connected(x, y, Direction::Down, layer);

    // Must have connections on BOTH ends of exactly one axis
    // (a "through" wire, not an endpoint)
    let horizontal_through = conn_left && conn_right;
    let vertical_through = conn_up && conn_down;

    if horizontal_through && !conn_up && !conn_down {
        Some((true, kind))  // Horizontal wire (no vertical connections)
    } else if vertical_through && !conn_left && !conn_right {
        Some((false, kind)) // Vertical wire (no horizontal connections)
    } else {
        None
    }
}

/// Try to place silicon at a cell, creating a gate if crossing opposite-type silicon
fn place_silicon_at(circuit: &mut Circuit, x: usize, y: usize, silicon_type: SiliconKind, is_vertical_movement: bool) {
    if !is_playable(x, y) {
        return;
    }

    // Check current node state
    if let Some(node) = circuit.get_node(x, y) {
        // If there's already a gate, check if we should preserve it
        if let Silicon::Gate { channel } = node.silicon {
            // Gate type is the opposite of channel
            let gate_type = match channel {
                SiliconKind::N => SiliconKind::P,
                SiliconKind::P => SiliconKind::N,
            };
            // If placing the same type as the gate, just connect (don't overwrite)
            if silicon_type == gate_type {
                return;
            }
            // If placing the same type as the channel, also don't overwrite
            if silicon_type == channel {
                return;
            }
        }
    }

    // Check if there's existing opposite-type silicon that could form a gate
    if let Some((is_horizontal_wire, existing_kind)) = get_straight_silicon(circuit, x, y) {
        if existing_kind != silicon_type {
            // We have opposite type silicon - check if we can form a gate
            // Gate forms when our movement is perpendicular to the existing wire
            // horizontal wire + vertical movement = perpendicular
            // vertical wire + horizontal movement = perpendicular
            let perpendicular = is_horizontal_wire == is_vertical_movement;

            if perpendicular {
                // Create a gate! The existing silicon becomes the channel
                if let Some(node) = circuit.get_node_mut(x, y) {
                    node.silicon = Silicon::Gate { channel: existing_kind };
                }
                return;
            }
        }
    }

    // No gate - just place the silicon (overwrite if necessary)
    if let Some(node) = circuit.get_node_mut(x, y) {
        match silicon_type {
            SiliconKind::N => node.silicon = Silicon::N,
            SiliconKind::P => node.silicon = Silicon::P,
        }
    }
}

/// Place silicon along a path
fn place_silicon_path(circuit: &mut Circuit, path: &[(usize, usize)], silicon_type: SiliconKind) {
    let layer = match silicon_type {
        SiliconKind::N => Layer::NSilicon,
        SiliconKind::P => Layer::PSilicon,
    };

    for i in 0..path.len() {
        let (x, y) = path[i];

        // Place silicon on node (only if playable)
        if is_playable(x, y) {
            // Determine if this step is vertical movement
            let is_vertical = if i > 0 {
                let (_, py) = path[i - 1];
                py != y  // Vertical if y changed
            } else if path.len() > 1 {
                let (_, ny) = path[1];
                ny != y
            } else {
                false
            };

            // Place silicon (with gate detection)
            place_silicon_at(circuit, x, y, silicon_type, is_vertical);
        }

        // Connect to previous node in path (always, even for non-playable)
        if i > 0 {
            let (px, py) = path[i - 1];
            circuit.set_edge(px, py, x, y, layer, true);
        }
    }
}

/// Place metal along a path
fn place_metal_path(circuit: &mut Circuit, path: &[(usize, usize)]) {
    for i in 0..path.len() {
        let (x, y) = path[i];

        // Place metal on node (only if playable - pins already have metal)
        if is_playable(x, y) {
            if let Some(node) = circuit.get_node_mut(x, y) {
                node.metal = true;
            }
        }

        // Connect to previous node in path (always, even for pins)
        if i > 0 {
            let (px, py) = path[i - 1];
            circuit.set_edge(px, py, x, y, Layer::Metal, true);
        }
    }
}

/// Place a via at a position (only if there's silicon)
fn place_via(circuit: &mut Circuit, x: usize, y: usize) -> bool {
    if !is_playable(x, y) {
        return false;
    }

    if let Some(node) = circuit.get_node_mut(x, y) {
        if node.silicon != Silicon::None {
            node.via = true;
            node.metal = true;
            return true;
        }
    }
    false
}

/// Delete via at a position
fn delete_via(circuit: &mut Circuit, x: usize, y: usize) {
    if let Some(node) = circuit.get_node_mut(x, y) {
        node.via = false;
    }
}

/// Delete metal at a position and its connections
fn delete_metal(circuit: &mut Circuit, x: usize, y: usize) {
    if let Some(node) = circuit.get_node_mut(x, y) {
        // Don't delete metal if there's a via
        if !node.via {
            node.metal = false;
        }
    }

    // Remove metal edges
    if x > 0 {
        circuit.set_edge(x - 1, y, x, y, Layer::Metal, false);
    }
    if x < GRID_WIDTH - 1 {
        circuit.set_edge(x, y, x + 1, y, Layer::Metal, false);
    }
    if y > 0 {
        circuit.set_edge(x, y - 1, x, y, Layer::Metal, false);
    }
    if y < GRID_HEIGHT - 1 {
        circuit.set_edge(x, y, x, y + 1, Layer::Metal, false);
    }
}

/// Delete silicon and via at a position
fn delete_silicon(circuit: &mut Circuit, x: usize, y: usize) {
    if let Some(node) = circuit.get_node_mut(x, y) {
        node.silicon = Silicon::None;
        node.via = false;
    }

    // Remove silicon edges
    if x > 0 {
        circuit.set_edge(x - 1, y, x, y, Layer::NSilicon, false);
        circuit.set_edge(x - 1, y, x, y, Layer::PSilicon, false);
    }
    if x < GRID_WIDTH - 1 {
        circuit.set_edge(x, y, x + 1, y, Layer::NSilicon, false);
        circuit.set_edge(x, y, x + 1, y, Layer::PSilicon, false);
    }
    if y > 0 {
        circuit.set_edge(x, y - 1, x, y, Layer::NSilicon, false);
        circuit.set_edge(x, y - 1, x, y, Layer::PSilicon, false);
    }
    if y < GRID_HEIGHT - 1 {
        circuit.set_edge(x, y, x, y + 1, Layer::NSilicon, false);
        circuit.set_edge(x, y, x, y + 1, Layer::PSilicon, false);
    }

    // Check neighboring gates for validity
    validate_neighboring_gates(circuit, x, y);
}

/// Check if a gate at position (x, y) is still valid, and if not, convert it to its channel type
fn validate_gate(circuit: &mut Circuit, x: usize, y: usize) {
    let channel = match circuit.get_node(x, y) {
        Some(node) => match node.silicon {
            Silicon::Gate { channel } => channel,
            _ => return, // Not a gate, nothing to validate
        },
        None => return,
    };

    // Check if the channel still has through connections (both ends)
    let layer = match channel {
        SiliconKind::N => Layer::NSilicon,
        SiliconKind::P => Layer::PSilicon,
    };

    let conn_left = circuit.is_connected(x, y, Direction::Left, layer);
    let conn_right = circuit.is_connected(x, y, Direction::Right, layer);
    let conn_up = circuit.is_connected(x, y, Direction::Up, layer);
    let conn_down = circuit.is_connected(x, y, Direction::Down, layer);

    let horizontal_through = conn_left && conn_right;
    let vertical_through = conn_up && conn_down;

    // Gate is valid if channel has through connection in exactly one axis
    let is_valid = (horizontal_through && !conn_up && !conn_down)
        || (vertical_through && !conn_left && !conn_right);

    if !is_valid {
        // Convert gate back to channel type
        if let Some(node) = circuit.get_node_mut(x, y) {
            node.silicon = match channel {
                SiliconKind::N => Silicon::N,
                SiliconKind::P => Silicon::P,
            };
        }
    }
}

/// Check all neighbors of a position for invalid gates
fn validate_neighboring_gates(circuit: &mut Circuit, x: usize, y: usize) {
    if x > 0 {
        validate_gate(circuit, x - 1, y);
    }
    if x < GRID_WIDTH - 1 {
        validate_gate(circuit, x + 1, y);
    }
    if y > 0 {
        validate_gate(circuit, x, y - 1);
    }
    if y < GRID_HEIGHT - 1 {
        validate_gate(circuit, x, y + 1);
    }
}

/// Delete everything at a position
fn delete_all(circuit: &mut Circuit, x: usize, y: usize) {
    delete_metal(circuit, x, y);
    delete_silicon(circuit, x, y);
    // Also remove metal on via
    if let Some(node) = circuit.get_node_mut(x, y) {
        node.metal = false;
    }
}

/// Delete everything in a rectangular region
fn delete_region(circuit: &mut Circuit, x1: usize, y1: usize, x2: usize, y2: usize) {
    for y in y1..=y2 {
        for x in x1..=x2 {
            if is_playable(x, y) {
                delete_all(circuit, x, y);
            }
        }
    }
}

/// Yank (copy) a rectangular region into a snippet
fn yank_region(circuit: &Circuit, x1: usize, y1: usize, x2: usize, y2: usize, name: String) -> Snippet {
    let width = x2 - x1 + 1;
    let height = y2 - y1 + 1;

    // Copy nodes
    let mut nodes = Vec::with_capacity(height);
    for y in y1..=y2 {
        let mut row = Vec::with_capacity(width);
        for x in x1..=x2 {
            if let Some(node) = circuit.get_node(x, y) {
                row.push(*node);
            } else {
                row.push(Node::default());
            }
        }
        nodes.push(row);
    }

    // Copy horizontal edges (connections between adjacent cells in same row)
    let mut h_edges = Vec::with_capacity(height);
    for y in y1..=y2 {
        let mut row = Vec::with_capacity(width.saturating_sub(1));
        for x in x1..x2 {
            let edge = circuit.get_edge(x, y, x + 1, y).copied().unwrap_or_default();
            row.push(edge);
        }
        h_edges.push(row);
    }

    // Copy vertical edges (connections between adjacent cells in same column)
    let mut v_edges = Vec::with_capacity(height.saturating_sub(1));
    for y in y1..y2 {
        let mut row = Vec::with_capacity(width);
        for x in x1..=x2 {
            let edge = circuit.get_edge(x, y, x, y + 1).copied().unwrap_or_default();
            row.push(edge);
        }
        v_edges.push(row);
    }

    Snippet {
        name,
        width,
        height,
        nodes,
        h_edges,
        v_edges,
    }
}

/// Yank the entire circuit as a design
fn yank_entire_circuit(circuit: &Circuit, name: String) -> Snippet {
    yank_region(circuit, 0, 0, GRID_WIDTH - 1, GRID_HEIGHT - 1, name)
}

/// Load a design into the circuit, replacing current content
fn load_design_to_circuit(circuit: &mut Circuit, design: &Snippet, pins: &[Pin]) {
    // Clear the entire circuit first
    for y in 0..GRID_HEIGHT {
        for x in 0..GRID_WIDTH {
            if let Some(node) = circuit.get_node_mut(x, y) {
                node.silicon = Silicon::None;
                node.via = false;
                node.metal = false;
            }
            // Clear edges
            if x + 1 < GRID_WIDTH {
                if let Some(edge) = circuit.get_edge_mut(x, y, x + 1, y) {
                    *edge = Edge::default();
                }
            }
            if y + 1 < GRID_HEIGHT {
                if let Some(edge) = circuit.get_edge_mut(x, y, x, y + 1) {
                    *edge = Edge::default();
                }
            }
        }
    }

    // Paste the design at (0,0)
    paste_snippet(circuit, design, 0, 0);

    // Re-setup pins (they should have metal)
    setup_pins(circuit, pins);
}

/// Rotate a snippet 90 degrees clockwise
fn rotate_snippet(snippet: &Snippet) -> Snippet {
    let old_w = snippet.width;
    let old_h = snippet.height;
    let new_w = old_h;
    let new_h = old_w;

    // Rotate nodes: new[x][y] = old[old_h - 1 - y][x]
    let mut new_nodes = vec![vec![Node::default(); new_w]; new_h];
    for old_y in 0..old_h {
        for old_x in 0..old_w {
            let new_x = old_h - 1 - old_y;
            let new_y = old_x;
            if new_y < new_h && new_x < new_w {
                new_nodes[new_y][new_x] = snippet.nodes[old_y][old_x];
            }
        }
    }

    // Rotate horizontal edges -> become vertical edges
    // old h_edges: (old_w - 1) x old_h -> new v_edges: new_w x (new_h - 1)
    let mut new_v_edges = vec![vec![Edge::default(); new_w]; new_h.saturating_sub(1)];
    for old_y in 0..old_h {
        for old_x in 0..old_w.saturating_sub(1) {
            let new_x = old_h - 1 - old_y;
            let new_y = old_x; // This becomes the top of the vertical edge
            if new_y < new_h.saturating_sub(1) && new_x < new_w {
                new_v_edges[new_y][new_x] = snippet.h_edges.get(old_y).and_then(|r| r.get(old_x)).copied().unwrap_or_default();
            }
        }
    }

    // Rotate vertical edges -> become horizontal edges
    // old v_edges: old_w x (old_h - 1) -> new h_edges: (new_w - 1) x new_h
    let mut new_h_edges = vec![vec![Edge::default(); new_w.saturating_sub(1)]; new_h];
    for old_y in 0..old_h.saturating_sub(1) {
        for old_x in 0..old_w {
            // Vertical edge from (old_x, old_y) to (old_x, old_y+1)
            // After rotation: becomes horizontal edge from (new_x, new_y) to (new_x+1, new_y)
            // where new_x = old_h - 1 - old_y - 1 = old_h - 2 - old_y (since edge connects old_y to old_y+1)
            let new_x = old_h - 2 - old_y;
            let new_y = old_x;
            if new_y < new_h && new_x < new_w.saturating_sub(1) {
                new_h_edges[new_y][new_x] = snippet.v_edges.get(old_y).and_then(|r| r.get(old_x)).copied().unwrap_or_default();
            }
        }
    }

    Snippet {
        name: snippet.name.clone(),
        width: new_w,
        height: new_h,
        nodes: new_nodes,
        h_edges: new_h_edges,
        v_edges: new_v_edges,
    }
}

fn paste_snippet(circuit: &mut Circuit, snippet: &Snippet, dest_x: usize, dest_y: usize) {
    // Paste nodes
    for (sy, row) in snippet.nodes.iter().enumerate() {
        for (sx, node) in row.iter().enumerate() {
            let x = dest_x + sx;
            let y = dest_y + sy;
            if x < GRID_WIDTH && y < GRID_HEIGHT && is_playable(x, y) {
                if let Some(dest_node) = circuit.get_node_mut(x, y) {
                    *dest_node = *node;
                }
            }
        }
    }

    // Paste horizontal edges
    for (sy, row) in snippet.h_edges.iter().enumerate() {
        for (sx, edge) in row.iter().enumerate() {
            let x = dest_x + sx;
            let y = dest_y + sy;
            if x + 1 < GRID_WIDTH && y < GRID_HEIGHT {
                if let Some(dest_edge) = circuit.get_edge_mut(x, y, x + 1, y) {
                    *dest_edge = *edge;
                }
            }
        }
    }

    // Paste vertical edges
    for (sy, row) in snippet.v_edges.iter().enumerate() {
        for (sx, edge) in row.iter().enumerate() {
            let x = dest_x + sx;
            let y = dest_y + sy;
            if x < GRID_WIDTH && y + 1 < GRID_HEIGHT {
                if let Some(dest_edge) = circuit.get_edge_mut(x, y, x, y + 1) {
                    *dest_edge = *edge;
                }
            }
        }
    }
}

// ============================================================================
// Snippet Persistence
// ============================================================================

/// Save a snippet to a file in the snippets directory
fn save_snippet_to_file(snippet: &Snippet, dir: &PathBuf) -> std::io::Result<()> {
    // Create directory if it doesn't exist
    fs::create_dir_all(dir)?;

    // Sanitize filename (replace invalid chars with _)
    let safe_name: String = snippet.name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let filename = format!("{}.snip", safe_name);
    let path = dir.join(&filename);

    let mut file = fs::File::create(&path)?;

    // Write header
    writeln!(file, "CXEMA_SNIPPET")?;
    writeln!(file, "name:{}", snippet.name)?;
    writeln!(file, "size:{}x{}", snippet.width, snippet.height)?;

    // Write nodes
    writeln!(file, "nodes:")?;
    for row in &snippet.nodes {
        let line: String = row.iter().map(|node| {
            let si = match node.silicon {
                Silicon::None => '.',
                Silicon::N => 'n',
                Silicon::P => 'p',
                Silicon::Gate { channel: SiliconKind::N } => 'N',
                Silicon::Gate { channel: SiliconKind::P } => 'G',
            };
            let vi = if node.via { 'v' } else { '.' };
            let me = if node.metal { 'm' } else { '.' };
            format!("{}{}{}", si, vi, me)
        }).collect::<Vec<_>>().join(",");
        writeln!(file, "{}", line)?;
    }

    // Write horizontal edges
    writeln!(file, "h_edges:")?;
    for row in &snippet.h_edges {
        let line: String = row.iter().map(|edge| {
            let n = if edge.n_silicon { 'n' } else { '.' };
            let p = if edge.p_silicon { 'p' } else { '.' };
            let m = if edge.metal { 'm' } else { '.' };
            format!("{}{}{}", n, p, m)
        }).collect::<Vec<_>>().join(",");
        writeln!(file, "{}", line)?;
    }

    // Write vertical edges
    writeln!(file, "v_edges:")?;
    for row in &snippet.v_edges {
        let line: String = row.iter().map(|edge| {
            let n = if edge.n_silicon { 'n' } else { '.' };
            let p = if edge.p_silicon { 'p' } else { '.' };
            let m = if edge.metal { 'm' } else { '.' };
            format!("{}{}{}", n, p, m)
        }).collect::<Vec<_>>().join(",");
        writeln!(file, "{}", line)?;
    }

    Ok(())
}

/// Load a snippet from a file
fn load_snippet_from_file(path: &PathBuf) -> std::io::Result<Snippet> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    // Check header
    let header = lines.next().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Empty file"))??;
    if header != "CXEMA_SNIPPET" {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid snippet file"));
    }

    // Parse name
    let name_line = lines.next().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Missing name"))??;
    let name = name_line.strip_prefix("name:").unwrap_or("unnamed").to_string();

    // Parse size
    let size_line = lines.next().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Missing size"))??;
    let size_str = size_line.strip_prefix("size:").unwrap_or("1x1");
    let mut size_parts = size_str.split('x');
    let width: usize = size_parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let height: usize = size_parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);

    // Skip "nodes:" line
    lines.next();

    // Parse nodes
    let mut nodes = Vec::new();
    for _ in 0..height {
        if let Some(Ok(line)) = lines.next() {
            let row: Vec<Node> = line.split(',').map(|cell| {
                let chars: Vec<char> = cell.chars().collect();
                let silicon = match chars.get(0) {
                    Some('n') => Silicon::N,
                    Some('p') => Silicon::P,
                    Some('N') => Silicon::Gate { channel: SiliconKind::N },
                    Some('G') => Silicon::Gate { channel: SiliconKind::P },
                    _ => Silicon::None,
                };
                let via = chars.get(1) == Some(&'v');
                let metal = chars.get(2) == Some(&'m');
                Node { silicon, via, metal }
            }).collect();
            nodes.push(row);
        }
    }

    // Skip "h_edges:" line
    lines.next();

    // Parse horizontal edges
    let mut h_edges = Vec::new();
    for _ in 0..height {
        if let Some(Ok(line)) = lines.next() {
            if line.is_empty() {
                h_edges.push(Vec::new());
            } else {
                let row: Vec<Edge> = line.split(',').map(|cell| {
                    let chars: Vec<char> = cell.chars().collect();
                    Edge {
                        n_silicon: chars.get(0) == Some(&'n'),
                        p_silicon: chars.get(1) == Some(&'p'),
                        metal: chars.get(2) == Some(&'m'),
                    }
                }).collect();
                h_edges.push(row);
            }
        }
    }

    // Skip "v_edges:" line
    lines.next();

    // Parse vertical edges
    let mut v_edges = Vec::new();
    for _ in 0..height.saturating_sub(1) {
        if let Some(Ok(line)) = lines.next() {
            if line.is_empty() {
                v_edges.push(Vec::new());
            } else {
                let row: Vec<Edge> = line.split(',').map(|cell| {
                    let chars: Vec<char> = cell.chars().collect();
                    Edge {
                        n_silicon: chars.get(0) == Some(&'n'),
                        p_silicon: chars.get(1) == Some(&'p'),
                        metal: chars.get(2) == Some(&'m'),
                    }
                }).collect();
                v_edges.push(row);
            }
        }
    }

    Ok(Snippet {
        name,
        width,
        height,
        nodes,
        h_edges,
        v_edges,
    })
}

/// Load all snippets from the snippets directory
fn load_all_snippets(dir: &PathBuf) -> Vec<Snippet> {
    let mut snippets = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "snip").unwrap_or(false) {
                if let Ok(snippet) = load_snippet_from_file(&path) {
                    snippets.push(snippet);
                }
            }
        }
    }

    // Sort by name
    snippets.sort_by(|a, b| a.name.cmp(&b.name));
    snippets
}

/// Delete a snippet file from the snippets directory
fn delete_snippet_file(snippet: &Snippet, dir: &PathBuf) -> std::io::Result<()> {
    // Generate the same filename as save_snippet_to_file
    let safe_name: String = snippet.name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let filename = format!("{}.snip", safe_name);
    let path = dir.join(&filename);

    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let snippets_dir = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        PathBuf::from(".snippits")
    };
    let designs_dir = if args.len() > 2 {
        PathBuf::from(&args[2])
    } else {
        PathBuf::from(".designs")
    };
    let levels_dir = if args.len() > 3 {
        PathBuf::from(&args[3])
    } else {
        PathBuf::from("levels")
    };
    let save_path = PathBuf::from(".save.json");

    let mut buffer: Vec<u32> = vec![0; WINDOW_WIDTH * WINDOW_HEIGHT];
    let mut circuit = Circuit::new();
    let mut editor = EditorState::new(snippets_dir.clone(), designs_dir.clone(), levels_dir.clone(), save_path);

    // Load existing snippets, designs, and levels from disk
    editor.snippets = load_all_snippets(&snippets_dir);
    editor.designs = load_all_snippets(&designs_dir);
    editor.levels = load_all_levels(&levels_dir);

    // Load font for pin labels
    let font = load_font("terminus/ter-u14b.bdf");

    // Create pins and place their metal
    let pins = create_pins();
    setup_pins(&mut circuit, &pins);

    // Add some test patterns to visualize
    setup_test_pattern(&mut circuit);

    // Audio setup for music playback
    let (_stream, stream_handle) = OutputStream::try_default()
        .expect("Failed to create audio output stream");
    let sink = Sink::try_new(&stream_handle).expect("Failed to create audio sink");

    // Simulation state
    let mut sim = SimState::new();
    let mut sim_last_tick = Instant::now();
    let sim_tick_duration = Duration::from_millis(150); // 9/60 seconds = 150ms
    let mut sim_running = false;
    let mut sim_waveform_index = 0;  // Current position in waveform

    let mut window = Window::new(
        "CXEMA",
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
        WindowOptions {
            resize: false,
            ..WindowOptions::default()
        },
    )
    .expect("Failed to create window");

    window.set_target_fps(60);

    // Track key states to detect key press (not just held)
    let mut prev_keys: Vec<Key> = Vec::new();
    // Track mouse button states for debouncing
    let mut prev_left_down = false;
    let mut prev_right_down = false;

    // Key repeat state: tracks when each movement key was first pressed and last repeated
    let mut key_press_time: HashMap<Key, Instant> = HashMap::new();
    let mut key_last_repeat: HashMap<Key, Instant> = HashMap::new();
    let movement_keys = [Key::H, Key::J, Key::K, Key::L, Key::Left, Key::Right, Key::Up, Key::Down];

    while window.is_open() {
        let now = Instant::now();

        // Get currently pressed keys
        let current_keys: Vec<Key> = window.get_keys();

        // Detect newly pressed keys
        let mut new_keys: Vec<Key> = current_keys
            .iter()
            .filter(|k| !prev_keys.contains(k))
            .copied()
            .collect();

        // Track press times for movement keys
        for &key in &movement_keys {
            if current_keys.contains(&key) {
                if !prev_keys.contains(&key) {
                    // Key just pressed - record time
                    key_press_time.insert(key, now);
                    key_last_repeat.remove(&key);
                }
            } else {
                // Key released - clear tracking
                key_press_time.remove(&key);
                key_last_repeat.remove(&key);
            }
        }

        // Generate repeat events for held movement keys
        for &key in &movement_keys {
            if let Some(&press_time) = key_press_time.get(&key) {
                let held_duration = now.duration_since(press_time);
                if held_duration >= Duration::from_millis(KEY_REPEAT_DELAY_MS) {
                    // Past initial delay - check if we should repeat
                    let should_repeat = match key_last_repeat.get(&key) {
                        None => true, // First repeat after delay
                        Some(&last) => now.duration_since(last) >= Duration::from_millis(KEY_REPEAT_RATE_MS),
                    };
                    if should_repeat && !new_keys.contains(&key) {
                        new_keys.push(key);
                        key_last_repeat.insert(key, now);
                    }
                }
            }
        }

        // Handle Escape to cancel path in any mode
        if new_keys.contains(&Key::Escape) {
            editor.path_start = None;
            editor.current_path.clear();
            if editor.mode == EditMode::Visual {
                editor.visual_state = VisualState::Normal;
                editor.selection_anchor = None;
            }
        }

        // Handle input
        handle_input(&mut circuit, &mut editor, &mut sim, &new_keys, &window, &pins);

        // Check if music selection changed via keyboard
        if editor.music_changed {
            play_music(editor.current_music, &sink);
            editor.music_changed = false;
        }

        // Get current mouse button states
        let left_down = window.get_mouse_down(minifb::MouseButton::Left);
        let right_down = window.get_mouse_down(minifb::MouseButton::Right);

        // Detect mouse button press (transition from not pressed to pressed)
        let left_clicked = left_down && !prev_left_down;
        let right_clicked = right_down && !prev_right_down;

        // Update mouse position and handle clicks
        if let Some((mx, my)) = window.get_mouse_pos(minifb::MouseMode::Clamp) {
            let mx = mx as usize;
            let my = my as usize;

            // Check for panel button clicks first
            if left_clicked && mx >= GRID_PIXEL_WIDTH {
                if let Some(new_mode) = get_clicked_button(mx, my) {
                    editor.mode = new_mode;
                    editor.path_start = None;
                    editor.current_path.clear();
                    if new_mode == EditMode::Visual {
                        editor.visual_state = VisualState::Normal;
                        editor.selection_anchor = None;
                    }
                }
            }

            // Check for tab clicks
            if left_clicked && my >= GRID_PIXEL_HEIGHT {
                if let Some(tab) = get_clicked_tab(mx, my) {
                    editor.active_tab = tab;
                }
            }

            // Check for music button clicks in Menu tab
            if left_clicked && editor.active_tab == Tab::Menu {
                if let Some(track) = get_clicked_music_button(mx, my) {
                    if track != editor.current_music {
                        editor.current_music = track;
                        play_music(track, &sink);
                    }
                }
            }

            // Update grid position for non-panel area
            let grid_x = mx / CELL_SIZE;
            let grid_y = my / CELL_SIZE;
            if grid_x < GRID_WIDTH && grid_y < GRID_HEIGHT {
                editor.mouse_grid_x = Some(grid_x);
                editor.mouse_grid_y = Some(grid_y);

                // In MouseSelect mode, cursor follows mouse (for easy paste positioning)
                if editor.mode == EditMode::MouseSelect {
                    editor.cursor_x = grid_x;
                    editor.cursor_y = grid_y;
                }

                // Update path preview for mouse-based modes
                if let Some(start) = editor.path_start {
                    let layer = match editor.mode {
                        EditMode::NSilicon => Layer::NSilicon,
                        EditMode::PSilicon => Layer::PSilicon,
                        EditMode::Metal => Layer::Metal,
                        _ => Layer::Metal,
                    };
                    editor.current_path = find_path(start, (grid_x, grid_y), &circuit, layer);
                }
            } else {
                editor.mouse_grid_x = None;
                editor.mouse_grid_y = None;
            }
        }

        // Handle mouse clicks with debouncing (only for grid area)
        handle_mouse(&mut circuit, &mut editor, left_clicked, right_clicked, left_down, right_down);

        // Toggle simulation with Space when on Verification tab
        if new_keys.contains(&Key::Space) && editor.active_tab == Tab::Verification {
            sim_running = !sim_running;
            if sim_running {
                // Reset simulation state when starting
                sim = SimState::new();
                sim.init_gates(&circuit);
                sim_waveform_index = 0;
                sim_last_tick = now;
            }
        }

        // Run simulation ticks
        if sim_running {
            if now.duration_since(sim_last_tick) >= sim_tick_duration {
                sim_last_tick = now;

                // Get pin values from current level's waveforms
                let mut pin_values = [false; 12];
                if let Some(level) = editor.levels.get(editor.selected_level) {
                    for waveform in &level.waveforms {
                        if waveform.is_input && waveform.pin_index < 12 {
                            // Get value at current waveform index
                            let value = waveform.values.chars().nth(sim_waveform_index)
                                .map(|c| c == '1')
                                .unwrap_or(false);
                            pin_values[waveform.pin_index] = value;
                        }
                    }

                    // Run simulation step
                    sim.step(&circuit, &pins, &pin_values);

                    // Advance waveform index
                    let max_len = level.waveforms.iter()
                        .map(|w| w.values.len())
                        .max()
                        .unwrap_or(0);
                    sim_waveform_index += 1;
                    if sim_waveform_index >= max_len {
                        sim_running = false; // Stop at end

                        // Calculate accuracy and check for completion
                        // Test mask: 'x' = don't care, '?' or empty = check
                        let mut total_bits = 0;
                        let mut correct_bits = 0;
                        for waveform in level.waveforms.iter().filter(|w| !w.is_input) {
                            let pin_idx = waveform.pin_index;
                            let test_chars: Vec<char> = waveform.test.chars().collect();
                            for (t, expected_ch) in waveform.values.chars().enumerate() {
                                if t < sim.output_history.len() {
                                    // Check test mask - skip if 'x', check if '?' or no mask
                                    let should_check = if t < test_chars.len() {
                                        test_chars[t] != 'x'
                                    } else {
                                        true  // No mask = check all
                                    };
                                    if !should_check {
                                        continue;
                                    }
                                    total_bits += 1;
                                    let expected = expected_ch == '1';
                                    let actual = sim.output_history[t][pin_idx];
                                    if expected == actual {
                                        correct_bits += 1;
                                    }
                                }
                            }
                        }

                        if total_bits > 0 {
                            let accuracy = correct_bits as f32 / total_bits as f32;
                            let passed = accuracy >= level.accuracy_threshold;

                            // Store result for display
                            sim.last_accuracy = Some(accuracy);
                            sim.last_passed = Some(passed);

                            if passed && !editor.save_data.is_level_complete(&level.name) {
                                editor.save_data.mark_level_complete(&level.name);
                                editor.save_data.save(&editor.save_path);
                            }
                        }
                    }
                }
            }
        }

        // Render
        render(&circuit, &editor, &pins, &font, &sim, sim_running, sim_waveform_index, &mut buffer);

        window
            .update_with_buffer(&buffer, WINDOW_WIDTH, WINDOW_HEIGHT)
            .unwrap();

        prev_keys = current_keys;
        prev_left_down = left_down;
        prev_right_down = right_down;
    }
}

/// Convert a key to a character for text input
fn key_to_char(key: Key, shift: bool) -> Option<char> {
    match key {
        Key::A => Some(if shift { 'A' } else { 'a' }),
        Key::B => Some(if shift { 'B' } else { 'b' }),
        Key::C => Some(if shift { 'C' } else { 'c' }),
        Key::D => Some(if shift { 'D' } else { 'd' }),
        Key::E => Some(if shift { 'E' } else { 'e' }),
        Key::F => Some(if shift { 'F' } else { 'f' }),
        Key::G => Some(if shift { 'G' } else { 'g' }),
        Key::H => Some(if shift { 'H' } else { 'h' }),
        Key::I => Some(if shift { 'I' } else { 'i' }),
        Key::J => Some(if shift { 'J' } else { 'j' }),
        Key::K => Some(if shift { 'K' } else { 'k' }),
        Key::L => Some(if shift { 'L' } else { 'l' }),
        Key::M => Some(if shift { 'M' } else { 'm' }),
        Key::N => Some(if shift { 'N' } else { 'n' }),
        Key::O => Some(if shift { 'O' } else { 'o' }),
        Key::P => Some(if shift { 'P' } else { 'p' }),
        Key::Q => Some(if shift { 'Q' } else { 'q' }),
        Key::R => Some(if shift { 'R' } else { 'r' }),
        Key::S => Some(if shift { 'S' } else { 's' }),
        Key::T => Some(if shift { 'T' } else { 't' }),
        Key::U => Some(if shift { 'U' } else { 'u' }),
        Key::V => Some(if shift { 'V' } else { 'v' }),
        Key::W => Some(if shift { 'W' } else { 'w' }),
        Key::X => Some(if shift { 'X' } else { 'x' }),
        Key::Y => Some(if shift { 'Y' } else { 'y' }),
        Key::Z => Some(if shift { 'Z' } else { 'z' }),
        Key::Key0 => Some(if shift { ')' } else { '0' }),
        Key::Key1 => Some(if shift { '!' } else { '1' }),
        Key::Key2 => Some(if shift { '@' } else { '2' }),
        Key::Key3 => Some(if shift { '#' } else { '3' }),
        Key::Key4 => Some(if shift { '$' } else { '4' }),
        Key::Key5 => Some(if shift { '%' } else { '5' }),
        Key::Key6 => Some(if shift { '^' } else { '6' }),
        Key::Key7 => Some(if shift { '&' } else { '7' }),
        Key::Key8 => Some(if shift { '*' } else { '8' }),
        Key::Key9 => Some(if shift { '(' } else { '9' }),
        Key::Space => Some(' '),
        Key::Minus => Some(if shift { '_' } else { '-' }),
        Key::Equal => Some(if shift { '+' } else { '=' }),
        Key::Period => Some(if shift { '>' } else { '.' }),
        Key::Comma => Some(if shift { '<' } else { ',' }),
        Key::Slash => Some(if shift { '?' } else { '/' }),
        _ => None,
    }
}

/// Get the previous tab in order
fn prev_tab(tab: Tab) -> Tab {
    match tab {
        Tab::Specifications => Tab::Menu,
        Tab::Verification => Tab::Specifications,
        Tab::DesignSnippets => Tab::Verification,
        Tab::Designs => Tab::DesignSnippets,
        Tab::Help => Tab::Designs,
        Tab::Menu => Tab::Help,
    }
}

/// Get the next tab in order
fn next_tab(tab: Tab) -> Tab {
    match tab {
        Tab::Specifications => Tab::Verification,
        Tab::Verification => Tab::DesignSnippets,
        Tab::DesignSnippets => Tab::Designs,
        Tab::Designs => Tab::Help,
        Tab::Help => Tab::Menu,
        Tab::Menu => Tab::Specifications,
    }
}

/// Handle keyboard input
fn handle_input(circuit: &mut Circuit, editor: &mut EditorState, sim: &mut SimState, new_keys: &[Key], window: &Window, pins: &[Pin]) {
    // Check if modifier keys are held
    let shift_held = window.is_key_down(Key::LeftShift) || window.is_key_down(Key::RightShift);
    let ctrl_held = window.is_key_down(Key::LeftCtrl) || window.is_key_down(Key::RightCtrl);

    // Handle dialog input first (captures all keys when dialog is open)
    if let DialogState::SaveSnippet { ref mut name } = editor.dialog {
        for key in new_keys {
            match key {
                Key::Enter => {
                    // Save the snippet with the entered name
                    if let Some(ref mut snippet) = editor.yank_buffer {
                        snippet.name = if name.is_empty() { "snippet".to_string() } else { name.clone() };
                        // Save to disk
                        if let Err(e) = save_snippet_to_file(snippet, &editor.snippets_dir) {
                            eprintln!("Failed to save snippet: {}", e);
                        }
                        editor.snippets.push(snippet.clone());
                        editor.selected_snippet = editor.snippets.len() - 1;
                    }
                    editor.dialog = DialogState::None;
                    return;
                }
                Key::Escape => {
                    editor.dialog = DialogState::None;
                    return;
                }
                Key::Backspace => {
                    name.pop();
                }
                _ => {
                    // Try to get character for the key
                    if let Some(c) = key_to_char(*key, shift_held) {
                        name.push(c);
                    }
                }
            }
        }
        return; // Don't process other input while dialog is open
    }

    // Handle SaveDesign dialog
    if let DialogState::SaveDesign { ref mut name } = editor.dialog {
        for key in new_keys {
            match key {
                Key::Enter => {
                    // Save the design with the entered name
                    let design_name = if name.is_empty() { "design".to_string() } else { name.clone() };
                    let design = yank_entire_circuit(circuit, design_name);
                    // Save to disk
                    if let Err(e) = save_snippet_to_file(&design, &editor.designs_dir) {
                        eprintln!("Failed to save design: {}", e);
                    }
                    editor.designs.push(design);
                    editor.selected_design = editor.designs.len() - 1;
                    editor.dialog = DialogState::None;
                    return;
                }
                Key::Escape => {
                    editor.dialog = DialogState::None;
                    return;
                }
                Key::Backspace => {
                    name.pop();
                }
                _ => {
                    // Try to get character for the key
                    if let Some(c) = key_to_char(*key, shift_held) {
                        name.push(c);
                    }
                }
            }
        }
        return; // Don't process other input while dialog is open
    }

    for key in new_keys {
        // Shift + arrow keys for tab switching
        if shift_held {
            match key {
                Key::Left | Key::H => {
                    editor.active_tab = prev_tab(editor.active_tab);
                    continue;
                }
                Key::Right | Key::L => {
                    editor.active_tab = next_tab(editor.active_tab);
                    continue;
                }
                // Shift + up/down for within-tab navigation (snippet/design/level/music selection)
                Key::Up | Key::K => {
                    if editor.active_tab == Tab::DesignSnippets && !editor.snippets.is_empty() {
                        editor.selected_snippet = editor.selected_snippet.saturating_sub(1);
                    } else if editor.active_tab == Tab::Designs && !editor.designs.is_empty() {
                        editor.selected_design = editor.selected_design.saturating_sub(1);
                    } else if editor.active_tab == Tab::Verification && !editor.levels.is_empty() {
                        let prev_level = editor.selected_level;
                        editor.selected_level = editor.selected_level.saturating_sub(1);
                        // Adjust scroll offset to keep selected level visible
                        if editor.selected_level < editor.level_scroll_offset {
                            editor.level_scroll_offset = editor.selected_level;
                        }
                        // Reset simulation state when level changes
                        if editor.selected_level != prev_level {
                            sim.output_history.clear();
                            sim.last_accuracy = None;
                            sim.last_passed = None;
                        }
                    } else if editor.active_tab == Tab::Menu {
                        let new_track = editor.current_music.prev();
                        if new_track != editor.current_music {
                            editor.current_music = new_track;
                            editor.music_changed = true;
                        }
                    }
                    continue;
                }
                Key::Down | Key::J => {
                    if editor.active_tab == Tab::DesignSnippets && !editor.snippets.is_empty() {
                        editor.selected_snippet = (editor.selected_snippet + 1).min(editor.snippets.len() - 1);
                    } else if editor.active_tab == Tab::Designs && !editor.designs.is_empty() {
                        editor.selected_design = (editor.selected_design + 1).min(editor.designs.len() - 1);
                    } else if editor.active_tab == Tab::Verification && !editor.levels.is_empty() {
                        let prev_level = editor.selected_level;
                        editor.selected_level = (editor.selected_level + 1).min(editor.levels.len() - 1);
                        // Adjust scroll offset to keep selected level visible (max 10 visible)
                        if editor.selected_level >= editor.level_scroll_offset + 10 {
                            editor.level_scroll_offset = editor.selected_level - 9;
                        }
                        // Reset simulation state when level changes
                        if editor.selected_level != prev_level {
                            sim.output_history.clear();
                            sim.last_accuracy = None;
                            sim.last_passed = None;
                        }
                    } else if editor.active_tab == Tab::Menu {
                        let new_track = editor.current_music.next();
                        if new_track != editor.current_music {
                            editor.current_music = new_track;
                            editor.music_changed = true;
                        }
                    }
                    continue;
                }
                // Shift + D to delete selected snippet/design
                Key::D => {
                    if editor.active_tab == Tab::DesignSnippets && !editor.snippets.is_empty() {
                        let idx = editor.selected_snippet;
                        if idx < editor.snippets.len() {
                            let snippet = &editor.snippets[idx];
                            if let Err(e) = delete_snippet_file(snippet, &editor.snippets_dir) {
                                eprintln!("Failed to delete snippet file: {}", e);
                            }
                            editor.snippets.remove(idx);
                            if editor.selected_snippet >= editor.snippets.len() && editor.selected_snippet > 0 {
                                editor.selected_snippet -= 1;
                            }
                        }
                    } else if editor.active_tab == Tab::Designs && !editor.designs.is_empty() {
                        let idx = editor.selected_design;
                        if idx < editor.designs.len() {
                            let design = &editor.designs[idx];
                            if let Err(e) = delete_snippet_file(design, &editor.designs_dir) {
                                eprintln!("Failed to delete design file: {}", e);
                            }
                            editor.designs.remove(idx);
                            if editor.selected_design >= editor.designs.len() && editor.selected_design > 0 {
                                editor.selected_design -= 1;
                            }
                        }
                    }
                    continue;
                }
                // Shift + R to load selected design (replace current circuit)
                Key::R => {
                    if editor.active_tab == Tab::Designs && !editor.designs.is_empty() {
                        let idx = editor.selected_design;
                        if let Some(design) = editor.designs.get(idx).cloned() {
                            load_design_to_circuit(circuit, &design, pins);
                        }
                    }
                    continue;
                }
                // Shift + W to save current design
                Key::W => {
                    if editor.active_tab == Tab::Designs {
                        editor.dialog = DialogState::SaveDesign { name: String::new() };
                    }
                    continue;
                }
                _ => {}
            }
        }

        match key {
            // Mode switching (1-8)
            Key::Key1 => {
                editor.mode = EditMode::NSilicon;
                editor.path_start = None;
                editor.current_path.clear();
            }
            Key::Key2 => {
                editor.mode = EditMode::PSilicon;
                editor.path_start = None;
                editor.current_path.clear();
            }
            Key::Key3 => {
                editor.mode = EditMode::Metal;
                editor.path_start = None;
                editor.current_path.clear();
            }
            Key::Key4 => {
                editor.mode = EditMode::Via;
                editor.path_start = None;
                editor.current_path.clear();
            }
            Key::Key5 => {
                editor.mode = EditMode::DeleteMetal;
                editor.path_start = None;
                editor.current_path.clear();
            }
            Key::Key6 => {
                editor.mode = EditMode::DeleteSilicon;
                editor.path_start = None;
                editor.current_path.clear();
            }
            Key::Key7 => {
                editor.mode = EditMode::DeleteAll;
                editor.path_start = None;
                editor.current_path.clear();
            }
            Key::Key8 => {
                editor.mode = EditMode::Visual;
                editor.visual_state = VisualState::Normal;
                editor.path_start = None;
                editor.current_path.clear();
                editor.selection_anchor = None;
            }
            Key::Key9 => {
                editor.mode = EditMode::MouseSelect;
                editor.path_start = None;
                editor.current_path.clear();
                editor.selection_anchor = None;
                editor.mouse_selection_end = None;
            }

            // Visual mode keys (only active in Visual mode)
            _ if editor.mode == EditMode::Visual => {
                handle_visual_mode_key(circuit, editor, *key, shift_held, ctrl_held);
            }

            // MouseSelect mode keys
            _ if editor.mode == EditMode::MouseSelect => {
                handle_mouse_select_key(circuit, editor, *key);
            }

            _ => {}
        }
    }
}

// Playable area boundaries
const PLAYABLE_LEFT: usize = 4;
const PLAYABLE_RIGHT: usize = 39;
const PLAYABLE_WIDTH: usize = PLAYABLE_RIGHT - PLAYABLE_LEFT + 1; // 36

/// Handle visual mode keyboard input
fn handle_visual_mode_key(circuit: &mut Circuit, editor: &mut EditorState, key: Key, shift_held: bool, ctrl_held: bool) {
    let prev_x = editor.cursor_x;
    let prev_y = editor.cursor_y;
    let modifier = editor.pending_modifier;

    // Handle goto prefix commands (g + something)
    if modifier == PendingModifier::Goto {
        match key {
            Key::G => {
                // gg - go to top row
                editor.cursor_y = 0;
                editor.clear_modifier();
            }
            Key::E => {
                // ge - go to last row
                editor.cursor_y = GRID_HEIGHT - 1;
                editor.clear_modifier();
            }
            Key::D => {
                // gd - go down by half
                editor.cursor_y = (editor.cursor_y + GRID_HEIGHT / 2).min(GRID_HEIGHT - 1);
                editor.clear_modifier();
            }
            Key::U => {
                // gu - go up by half
                editor.cursor_y = editor.cursor_y.saturating_sub(GRID_HEIGHT / 2);
                editor.clear_modifier();
            }
            Key::H => {
                // gh - go right by half width
                editor.cursor_x = (editor.cursor_x + PLAYABLE_WIDTH / 2).min(GRID_WIDTH - 1);
                editor.clear_modifier();
            }
            Key::B => {
                // gb - go back (left) by half width
                editor.cursor_x = editor.cursor_x.saturating_sub(PLAYABLE_WIDTH / 2);
                editor.clear_modifier();
            }
            Key::Escape => {
                editor.clear_modifier();
            }
            _ => {
                // Invalid g command, clear modifier
                editor.clear_modifier();
            }
        }
    }
    // Handle silicon/metal modifier commands
    else if modifier == PendingModifier::Silicon || modifier == PendingModifier::Metal {
        match key {
            Key::D => {
                // sd or md - delete with filter
                if let Some((x1, y1, x2, y2)) = editor.get_selection() {
                    for y in y1..=y2 {
                        for x in x1..=x2 {
                            if is_playable(x, y) {
                                match modifier {
                                    PendingModifier::Silicon => delete_silicon(circuit, x, y),
                                    PendingModifier::Metal => delete_metal(circuit, x, y),
                                    _ => {}
                                }
                            }
                        }
                    }
                    editor.visual_state = VisualState::Normal;
                    editor.selection_anchor = None;
                } else if is_playable(editor.cursor_x, editor.cursor_y) {
                    match modifier {
                        PendingModifier::Silicon => delete_silicon(circuit, editor.cursor_x, editor.cursor_y),
                        PendingModifier::Metal => delete_metal(circuit, editor.cursor_x, editor.cursor_y),
                        _ => {}
                    }
                }
                editor.clear_modifier();
            }
            Key::E => {
                // se or me - move right until silicon/metal found
                let target_x = find_material_right(circuit, editor.cursor_x, editor.cursor_y, modifier);
                editor.cursor_x = target_x;
                editor.clear_modifier();
            }
            Key::Escape => {
                editor.clear_modifier();
            }
            _ => {
                editor.clear_modifier();
            }
        }
    }
    // Normal commands (no modifier)
    else {
        match key {
            // Prefix modifiers
            Key::S => {
                editor.pending_modifier = PendingModifier::Silicon;
                return; // Don't clear modifier or do material placement
            }
            Key::M => {
                editor.pending_modifier = PendingModifier::Metal;
                return;
            }
            Key::G => {
                editor.pending_modifier = PendingModifier::Goto;
                return;
            }

            // Movement: hjkl or arrow keys
            Key::H | Key::Left => {
                if ctrl_held {
                    // Ctrl+H not used, but keep for consistency
                }
                if editor.cursor_x > 0 {
                    editor.cursor_x -= 1;
                }
            }
            Key::J | Key::Down => {
                if editor.cursor_y < GRID_HEIGHT - 1 {
                    editor.cursor_y += 1;
                }
            }
            Key::K | Key::Up => {
                if editor.cursor_y > 0 {
                    editor.cursor_y -= 1;
                }
            }
            Key::L | Key::Right => {
                if editor.cursor_x < GRID_WIDTH - 1 {
                    editor.cursor_x += 1;
                }
            }

            // 'w' - move right by 4
            Key::W => {
                editor.cursor_x = (editor.cursor_x + 4).min(GRID_WIDTH - 1);
            }

            // 'b' - move left by 4 (back)
            Key::B => {
                editor.cursor_x = editor.cursor_x.saturating_sub(4);
            }

            // 'e' - move right until any material found
            Key::E => {
                let target_x = find_material_right(circuit, editor.cursor_x, editor.cursor_y, PendingModifier::None);
                editor.cursor_x = target_x;
            }

            // Ctrl+A - move to left edge of playable area
            Key::A if ctrl_held => {
                editor.cursor_x = PLAYABLE_LEFT;
            }

            // Ctrl+E - move to right edge of playable area
            // Note: Key::E with ctrl is handled above in the 'e' match, so we check ctrl there

            // 'v' - toggle visual selection mode
            Key::V => {
                if editor.visual_state == VisualState::Selecting {
                    editor.visual_state = VisualState::Normal;
                    editor.selection_anchor = None;
                } else {
                    editor.visual_state = VisualState::Selecting;
                    editor.selection_anchor = Some((editor.cursor_x, editor.cursor_y));
                }
            }

            // 'd' - delete selection or cursor position (all)
            Key::D => {
                if let Some((x1, y1, x2, y2)) = editor.get_selection() {
                    delete_region(circuit, x1, y1, x2, y2);
                    editor.visual_state = VisualState::Normal;
                    editor.selection_anchor = None;
                } else if is_playable(editor.cursor_x, editor.cursor_y) {
                    delete_all(circuit, editor.cursor_x, editor.cursor_y);
                }
            }

            // '=' for metal, '+' (Shift + =) for P-silicon
            Key::Equal => {
                if shift_held {
                    // '+' - P-silicon placing mode
                    editor.visual_state = VisualState::PlacingP;
                } else {
                    // '=' - metal placing mode
                    editor.visual_state = VisualState::PlacingMetal;
                }
            }

            // '-' - enter N-silicon placing mode
            Key::Minus => {
                editor.visual_state = VisualState::PlacingN;
            }

            // '.' - toggle via at cursor
            Key::Period => {
                if is_playable(editor.cursor_x, editor.cursor_y) {
                    if let Some(node) = circuit.get_node(editor.cursor_x, editor.cursor_y) {
                        if node.via {
                            delete_via(circuit, editor.cursor_x, editor.cursor_y);
                        } else {
                            place_via(circuit, editor.cursor_x, editor.cursor_y);
                        }
                    }
                }
            }

            // 'y' - yank (copy) selection to snippet (opens dialog if has selection)
            Key::Y => {
                if let Some((x1, y1, x2, y2)) = editor.get_selection() {
                    // Yank to buffer first
                    editor.yank_buffer = Some(yank_region(circuit, x1, y1, x2, y2, String::new()));
                    // Open dialog to name the snippet
                    editor.dialog = DialogState::SaveSnippet { name: String::new() };
                    editor.visual_state = VisualState::Normal;
                    editor.selection_anchor = None;
                }
            }

            // 'p' - paste from yank buffer or selected snippet
            Key::P => {
                // First try yank buffer, then selected snippet
                let snippet_to_paste = editor.yank_buffer.clone()
                    .or_else(|| editor.snippets.get(editor.selected_snippet).cloned());

                if let Some(snippet) = snippet_to_paste {
                    paste_snippet(circuit, &snippet, editor.cursor_x, editor.cursor_y);
                }
            }

            // 'r' - rotate the current snippet (yank buffer or selected)
            Key::R => {
                // Rotate yank buffer if present
                if let Some(ref snippet) = editor.yank_buffer {
                    editor.yank_buffer = Some(rotate_snippet(snippet));
                } else if !editor.snippets.is_empty() {
                    // Rotate the selected snippet in place
                    let idx = editor.selected_snippet;
                    if idx < editor.snippets.len() {
                        editor.snippets[idx] = rotate_snippet(&editor.snippets[idx]);
                    }
                }
            }

            // Escape - exit any sub-mode, return to normal visual mode
            Key::Escape => {
                editor.visual_state = VisualState::Normal;
                editor.selection_anchor = None;
            }

            _ => {}
        }
    }

    // Handle Ctrl+E separately (since E is also used for other things)
    if ctrl_held && key == Key::E {
        editor.cursor_x = PLAYABLE_RIGHT;
    }

    // If in placing mode and cursor moved, place material
    if (editor.cursor_x != prev_x || editor.cursor_y != prev_y) && is_playable(editor.cursor_x, editor.cursor_y) {
        match editor.visual_state {
            VisualState::PlacingN => {
                place_silicon_path(circuit, &[(prev_x, prev_y), (editor.cursor_x, editor.cursor_y)], SiliconKind::N);
            }
            VisualState::PlacingP => {
                place_silicon_path(circuit, &[(prev_x, prev_y), (editor.cursor_x, editor.cursor_y)], SiliconKind::P);
            }
            VisualState::PlacingMetal => {
                place_metal_path(circuit, &[(prev_x, prev_y), (editor.cursor_x, editor.cursor_y)]);
            }
            _ => {}
        }
    }
}

/// Handle keys in MouseSelect mode (y, p, r, d, s, m, Escape)
fn handle_mouse_select_key(circuit: &mut Circuit, editor: &mut EditorState, key: Key) {
    // Check if selection is finalized (both clicks done)
    let selection_finalized = editor.selection_anchor.is_some() && editor.mouse_selection_end.is_some();

    // Handle pending modifier first
    match editor.pending_modifier {
        PendingModifier::Silicon => {
            match key {
                Key::D => {
                    // Delete only silicon in selection
                    if selection_finalized {
                        if let Some((x1, y1, x2, y2)) = editor.get_selection() {
                            for y in y1..=y2 {
                                for x in x1..=x2 {
                                    delete_silicon(circuit, x, y);
                                }
                            }
                            editor.selection_anchor = None;
                            editor.mouse_selection_end = None;
                        }
                    }
                    editor.clear_modifier();
                    return;
                }
                Key::Escape => {
                    editor.clear_modifier();
                    return;
                }
                _ => {
                    editor.clear_modifier();
                }
            }
        }
        PendingModifier::Metal => {
            match key {
                Key::D => {
                    // Delete only metal in selection
                    if selection_finalized {
                        if let Some((x1, y1, x2, y2)) = editor.get_selection() {
                            for y in y1..=y2 {
                                for x in x1..=x2 {
                                    delete_metal(circuit, x, y);
                                }
                            }
                            editor.selection_anchor = None;
                            editor.mouse_selection_end = None;
                        }
                    }
                    editor.clear_modifier();
                    return;
                }
                Key::Escape => {
                    editor.clear_modifier();
                    return;
                }
                _ => {
                    editor.clear_modifier();
                }
            }
        }
        _ => {}
    }

    match key {
        // 's' - silicon modifier prefix
        Key::S => {
            editor.pending_modifier = PendingModifier::Silicon;
        }

        // 'm' - metal modifier prefix
        Key::M => {
            editor.pending_modifier = PendingModifier::Metal;
        }

        // 'y' - yank selection (only when finalized)
        Key::Y => {
            if selection_finalized {
                if let Some((x1, y1, x2, y2)) = editor.get_selection() {
                    editor.yank_buffer = Some(yank_region(circuit, x1, y1, x2, y2, String::new()));
                    editor.dialog = DialogState::SaveSnippet { name: String::new() };
                }
            }
        }

        // 'p' - paste (works anytime)
        Key::P => {
            let snippet_to_paste = editor.yank_buffer.clone()
                .or_else(|| editor.snippets.get(editor.selected_snippet).cloned());

            if let Some(snippet) = snippet_to_paste {
                paste_snippet(circuit, &snippet, editor.cursor_x, editor.cursor_y);
            }
        }

        // 'r' - rotate snippet
        Key::R => {
            if let Some(ref snippet) = editor.yank_buffer {
                editor.yank_buffer = Some(rotate_snippet(snippet));
            } else if !editor.snippets.is_empty() {
                let idx = editor.selected_snippet;
                if idx < editor.snippets.len() {
                    editor.snippets[idx] = rotate_snippet(&editor.snippets[idx]);
                }
            }
        }

        // 'd' - delete selection (only when finalized)
        Key::D => {
            if selection_finalized {
                if let Some((x1, y1, x2, y2)) = editor.get_selection() {
                    for y in y1..=y2 {
                        for x in x1..=x2 {
                            delete_all(circuit, x, y);
                        }
                    }
                    editor.selection_anchor = None;
                    editor.mouse_selection_end = None;
                }
            }
        }

        // Escape - clear selection
        Key::Escape => {
            editor.selection_anchor = None;
            editor.mouse_selection_end = None;
        }

        _ => {}
    }
}

/// Find the next cell to the right that contains material
/// modifier: None = any material, Silicon = silicon only, Metal = metal only
fn find_material_right(circuit: &Circuit, start_x: usize, y: usize, modifier: PendingModifier) -> usize {
    for x in (start_x + 1)..GRID_WIDTH {
        if let Some(node) = circuit.get_node(x, y) {
            let has_target = match modifier {
                PendingModifier::None => node.silicon != Silicon::None || node.metal,
                PendingModifier::Silicon => node.silicon != Silicon::None,
                PendingModifier::Metal => node.metal,
                PendingModifier::Goto => false, // shouldn't happen
            };
            if has_target {
                return x;
            }
        }
    }
    // If nothing found, go to end
    GRID_WIDTH - 1
}

/// Handle mouse input
/// left_clicked/right_clicked = debounced single click (transition)
/// left_down/right_down = held state (for continuous actions)
fn handle_mouse(
    circuit: &mut Circuit,
    editor: &mut EditorState,
    left_clicked: bool,
    right_clicked: bool,
    left_down: bool,
    _right_down: bool,
) {
    let (grid_x, grid_y) = match (editor.mouse_grid_x, editor.mouse_grid_y) {
        (Some(x), Some(y)) => (x, y),
        _ => return,
    };

    match editor.mode {
        EditMode::NSilicon | EditMode::PSilicon | EditMode::Metal => {
            // Use debounced clicks for path construction
            if left_clicked {
                if editor.path_start.is_none() {
                    // First click - set start point
                    editor.path_start = Some((grid_x, grid_y));
                    editor.current_path = vec![(grid_x, grid_y)];
                } else if !editor.current_path.is_empty() {
                    // Second click - place the path
                    match editor.mode {
                        EditMode::NSilicon => {
                            place_silicon_path(circuit, &editor.current_path, SiliconKind::N);
                        }
                        EditMode::PSilicon => {
                            place_silicon_path(circuit, &editor.current_path, SiliconKind::P);
                        }
                        EditMode::Metal => {
                            place_metal_path(circuit, &editor.current_path);
                        }
                        _ => {}
                    }
                    editor.path_start = None;
                    editor.current_path.clear();
                }
            }
            if right_clicked {
                // Cancel path
                editor.path_start = None;
                editor.current_path.clear();
            }
        }

        EditMode::Via => {
            // Use debounced clicks for via placement
            if left_clicked {
                place_via(circuit, grid_x, grid_y);
            }
            if right_clicked {
                delete_via(circuit, grid_x, grid_y);
            }
        }

        EditMode::DeleteMetal => {
            // Use held state for continuous deletion
            if left_down || right_clicked {
                delete_metal(circuit, grid_x, grid_y);
            }
        }

        EditMode::DeleteSilicon => {
            // Use held state for continuous deletion
            if left_down || right_clicked {
                delete_silicon(circuit, grid_x, grid_y);
            }
        }

        EditMode::DeleteAll => {
            // Use held state for continuous deletion
            if left_down || right_clicked {
                delete_all(circuit, grid_x, grid_y);
            }
        }

        EditMode::Visual => {
            // In visual mode, mouse clicks move cursor
            if left_clicked {
                editor.cursor_x = grid_x;
                editor.cursor_y = grid_y;
            }
        }

        EditMode::MouseSelect => {
            // Two-click selection mode
            if left_clicked {
                if editor.selection_anchor.is_none() {
                    // First click: set anchor
                    editor.selection_anchor = Some((grid_x, grid_y));
                    editor.mouse_selection_end = None;
                } else if editor.mouse_selection_end.is_none() {
                    // Second click: finalize selection
                    editor.mouse_selection_end = Some((grid_x, grid_y));
                } else {
                    // Already have finalized selection, start new one
                    editor.selection_anchor = Some((grid_x, grid_y));
                    editor.mouse_selection_end = None;
                }
            }
            // Right click pastes (same as 'p')
            if right_clicked {
                let snippet_to_paste = editor.yank_buffer.clone()
                    .or_else(|| editor.snippets.get(editor.selected_snippet).cloned());

                if let Some(snippet) = snippet_to_paste {
                    paste_snippet(circuit, &snippet, editor.cursor_x, editor.cursor_y);
                }
            }
        }
    }
}

/// Play the specified music track on the given sink (looping)
fn play_music(track: MusicTrack, sink: &Sink) {
    // Stop any currently playing music
    sink.stop();

    if let Some(filename) = track.filename() {
        if let Ok(file) = File::open(filename) {
            let reader = BufReader::new(file);
            if let Ok(source) = Decoder::new(reader) {
                sink.append(source.repeat_infinite());
                sink.play();
            }
        }
    }
}

/// Load a BDF font file and return a map of character to bitmap data
fn load_font(path: &str) -> HashMap<char, Vec<Vec<bool>>> {
    use std::fs;

    let mut glyphs: HashMap<char, Vec<Vec<bool>>> = HashMap::new();

    let content = match fs::read(path) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("Warning: Could not load font from {}", path);
            return glyphs;
        }
    };

    let font = match bdf_parser::BdfFont::parse(&content) {
        Ok(f) => f,
        Err(_) => {
            eprintln!("Warning: Could not parse font from {}", path);
            return glyphs;
        }
    };

    for glyph in font.glyphs.iter() {
        if let Some(ch) = glyph.encoding {
            let width = glyph.bounding_box.size.x as usize;
            let height = glyph.bounding_box.size.y as usize;
            let bitmap: Vec<Vec<bool>> = (0..height)
                .map(|y| {
                    (0..width)
                        .map(|x| glyph.pixel(x, y))
                        .collect()
                })
                .collect();
            glyphs.insert(ch, bitmap);
        }
    }

    glyphs
}

/// Create the pin configuration for the game
/// Pins are 3x3, positioned at y = 2, 6, 10, 14, 18, 22 (stride of 4)
/// Left pins at x=1, right pins at x=40
fn create_pins() -> Vec<Pin> {
    let pin_ys = [2, 6, 10, 14, 18, 22];
    let left_labels = ["+", "A0", "A1", "A2", "A3", "+"];
    let right_labels = ["+", "Y0", "Y1", "Y2", "Y3", "+"];

    let mut pins = Vec::new();

    // Left side pins (x=1)
    for (i, &y) in pin_ys.iter().enumerate() {
        pins.push(Pin::new(left_labels[i], 1, y));
    }

    // Right side pins (x=40)
    for (i, &y) in pin_ys.iter().enumerate() {
        pins.push(Pin::new(right_labels[i], 40, y));
    }

    pins
}

/// Place metal at pin locations (3x3) and set up their internal connections
fn setup_pins(circuit: &mut Circuit, pins: &[Pin]) {
    for pin in pins {
        // Fill 3x3 area with metal
        for dy in 0..PIN_SIZE {
            for dx in 0..PIN_SIZE {
                let x = pin.x + dx;
                let y = pin.y + dy;
                if let Some(node) = circuit.get_node_mut(x, y) {
                    node.metal = true;
                }

                // Connect horizontally within the pin
                if dx < PIN_SIZE - 1 {
                    circuit.set_edge(x, y, x + 1, y, Layer::Metal, true);
                }
                // Connect vertically within the pin
                if dy < PIN_SIZE - 1 {
                    circuit.set_edge(x, y, x, y + 1, Layer::Metal, true);
                }
            }
        }
    }
}

// ============================================================================
// Test Pattern
// ============================================================================

fn setup_test_pattern(circuit: &mut Circuit) {
    // Playable area is columns 4-39, rows 0-26
    // Test patterns placed in playable area

    // Isolated N-type dot (yellow)
    if let Some(node) = circuit.get_node_mut(6, 5) {
        node.silicon = Silicon::N;
    }

    // Isolated P-type dot (red)
    if let Some(node) = circuit.get_node_mut(8, 5) {
        node.silicon = Silicon::P;
    }

    // Isolated metal dot
    if let Some(node) = circuit.get_node_mut(10, 5) {
        node.metal = true;
    }

    // Horizontal N-type wire
    for x in 6..=10 {
        if let Some(node) = circuit.get_node_mut(x, 8) {
            node.silicon = Silicon::N;
        }
        if x < 10 {
            circuit.set_edge(x, 8, x + 1, 8, Layer::NSilicon, true);
        }
    }

    // Vertical P-type wire
    for y in 5..=9 {
        if let Some(node) = circuit.get_node_mut(14, y) {
            node.silicon = Silicon::P;
        }
        if y < 9 {
            circuit.set_edge(14, y, 14, y + 1, Layer::PSilicon, true);
        }
    }

    // N-type cross centered at (18, 7)
    let cx = 18;
    let cy = 7;
    for &(x, y) in &[(cx, cy), (cx - 1, cy), (cx + 1, cy), (cx, cy - 1), (cx, cy + 1)] {
        if let Some(node) = circuit.get_node_mut(x, y) {
            node.silicon = Silicon::N;
        }
    }
    circuit.set_edge(cx, cy, cx - 1, cy, Layer::NSilicon, true);
    circuit.set_edge(cx, cy, cx + 1, cy, Layer::NSilicon, true);
    circuit.set_edge(cx, cy, cx, cy - 1, Layer::NSilicon, true);
    circuit.set_edge(cx, cy, cx, cy + 1, Layer::NSilicon, true);

    // Gate example: P-type horizontal channel with N-type vertical gate
    let gx = 24;
    let gy = 7;

    // Gate cell itself
    if let Some(node) = circuit.get_node_mut(gx, gy) {
        node.silicon = Silicon::Gate { channel: SiliconKind::P };
    }

    // P-type channel extends left and right
    if let Some(node) = circuit.get_node_mut(gx - 1, gy) {
        node.silicon = Silicon::P;
    }
    if let Some(node) = circuit.get_node_mut(gx + 1, gy) {
        node.silicon = Silicon::P;
    }
    circuit.set_edge(gx, gy, gx - 1, gy, Layer::PSilicon, true);
    circuit.set_edge(gx, gy, gx + 1, gy, Layer::PSilicon, true);

    // N-type gate extends up and down
    if let Some(node) = circuit.get_node_mut(gx, gy - 1) {
        node.silicon = Silicon::N;
    }
    if let Some(node) = circuit.get_node_mut(gx, gy + 1) {
        node.silicon = Silicon::N;
    }
    circuit.set_edge(gx, gy, gx, gy - 1, Layer::NSilicon, true);
    circuit.set_edge(gx, gy, gx, gy + 1, Layer::NSilicon, true);

    // Via example: N-type horizontal with via connecting to vertical metal
    for x in 6..11 {
        if let Some(node) = circuit.get_node_mut(x, 16) {
            node.silicon = Silicon::N;
            if x == 8 {
                node.via = true;
                node.metal = true;
            }
        }
        if x < 10 {
            circuit.set_edge(x, 16, x + 1, 16, Layer::NSilicon, true);
        }
    }
    // Vertical metal through the via
    for y in 14..=18 {
        if let Some(node) = circuit.get_node_mut(8, y) {
            node.metal = true;
        }
        if y < 18 {
            circuit.set_edge(8, y, 8, y + 1, Layer::Metal, true);
        }
    }

    // Metal over silicon example (showing transparency)
    for x in 20..25 {
        if let Some(node) = circuit.get_node_mut(x, 16) {
            node.silicon = Silicon::P;
            node.metal = true;
        }
        if x < 24 {
            circuit.set_edge(x, 16, x + 1, 16, Layer::PSilicon, true);
            circuit.set_edge(x, 16, x + 1, 16, Layer::Metal, true);
        }
    }

    // Pin connection: metal from left pin A1 (y=6) into playable area
    // Left pin is at x=1,2,3 so connect from x=3 to playable area starting at x=4
    for x in 4..8 {
        if let Some(node) = circuit.get_node_mut(x, 7) {
            node.metal = true;
        }
        if x < 7 {
            circuit.set_edge(x, 7, x + 1, 7, Layer::Metal, true);
        }
    }
    // Connect to the pin (pin middle row is y=7 for pin at y=6)
    circuit.set_edge(3, 7, 4, 7, Layer::Metal, true);

    // Pin connection: metal to right pin Y1 (y=6)
    // Right pin is at x=40,41,42 so connect from playable area ending at x=39
    for x in 36..40 {
        if let Some(node) = circuit.get_node_mut(x, 7) {
            node.metal = true;
        }
        if x < 39 {
            circuit.set_edge(x, 7, x + 1, 7, Layer::Metal, true);
        }
    }
    // Connect to the pin
    circuit.set_edge(39, 7, 40, 7, Layer::Metal, true);
}

// ============================================================================
// Rendering
// ============================================================================

fn render(circuit: &Circuit, editor: &EditorState, pins: &[Pin], font: &HashMap<char, Vec<Vec<bool>>>, sim: &SimState, sim_running: bool, sim_waveform_index: usize, buffer: &mut [u32]) {
    // Fill background
    for pixel in buffer.iter_mut() {
        *pixel = COLOR_BACKGROUND;
    }

    // Draw all cells (pins are just cells with metal)
    for grid_y in 0..GRID_HEIGHT {
        for grid_x in 0..GRID_WIDTH {
            render_cell(circuit, grid_x, grid_y, pins, buffer);
        }
    }

    // Draw signal visualization when simulation is running
    if sim_running {
        for grid_y in 0..GRID_HEIGHT {
            for grid_x in 0..GRID_WIDTH {
                // Highlight cells with high signals
                let metal_high = sim.metal_high[grid_y][grid_x];
                let n_high = sim.n_silicon_high[grid_y][grid_x];
                let p_high = sim.p_silicon_high[grid_y][grid_x];

                if metal_high || n_high || p_high {
                    // Draw a bright overlay on high signals
                    let color = if metal_high { 0xffff00 } else if n_high { 0xff8080 } else { 0xffff80 };
                    draw_cell_overlay(grid_x, grid_y, color, 0.4, buffer);
                }
            }
        }
    }

    // Draw pin labels centered in their 3x3 area
    // Use level pin names if a level is selected, otherwise use default pin labels
    let level_pins = editor.levels.get(editor.selected_level).map(|l| &l.pins);
    for (i, pin) in pins.iter().enumerate() {
        // Center of 3x3 pin area
        let center_x = pin.x * CELL_SIZE + (PIN_SIZE * CELL_SIZE) / 2;
        let center_y = pin.y * CELL_SIZE + (PIN_SIZE * CELL_SIZE) / 2;
        let label = level_pins
            .and_then(|p| p.get(i))
            .map(|s| s.as_str())
            .unwrap_or(&pin.label);
        draw_text(label, center_x, center_y, font, COLOR_PIN_TEXT, buffer);
    }

    // Draw path preview (for mouse-based construction modes)
    if !editor.current_path.is_empty() {
        for &(x, y) in &editor.current_path {
            draw_cell_overlay(x, y, COLOR_PATH_PREVIEW, 0.3, buffer);
        }
        // Highlight source point
        if let Some((sx, sy)) = editor.path_start {
            draw_cell_overlay(sx, sy, COLOR_SOURCE_POINT, 0.5, buffer);
        }
    }

    // Draw selection highlight (for visual mode)
    if let Some((x1, y1, x2, y2)) = editor.get_selection() {
        for y in y1..=y2 {
            for x in x1..=x2 {
                draw_cell_overlay(x, y, COLOR_SELECTION, 0.3, buffer);
            }
        }
    }

    // Draw ghost preview of snippet when Snippets tab is active and in visual/mouse-select mode
    if (editor.mode == EditMode::Visual || editor.mode == EditMode::MouseSelect) && editor.active_tab == Tab::DesignSnippets {
        // Try yank buffer first, then selected snippet
        let snippet_to_preview = editor.yank_buffer.as_ref()
            .or_else(|| editor.snippets.get(editor.selected_snippet));

        if let Some(snippet) = snippet_to_preview {
            render_snippet_ghost(snippet, editor.cursor_x, editor.cursor_y, buffer);
        }
    }

    // Draw cursor (for visual and mouse-select modes)
    if editor.mode == EditMode::Visual || editor.mode == EditMode::MouseSelect {
        draw_cursor(editor.cursor_x, editor.cursor_y, buffer);
    }

    // Draw mode indicator at top-left (shows current mode, submode, and pending modifier)
    let mode_text = match editor.mode {
        EditMode::NSilicon => "1:N-Si",
        EditMode::PSilicon => "2:P-Si",
        EditMode::Metal => "3:Metal",
        EditMode::Via => "4:Via",
        EditMode::DeleteMetal => "5:DelM",
        EditMode::DeleteSilicon => "6:DelS",
        EditMode::DeleteAll => "7:DelA",
        EditMode::Visual => match editor.pending_modifier {
            PendingModifier::Silicon => "8:s-",
            PendingModifier::Metal => "8:m-",
            PendingModifier::Goto => "8:g-",
            PendingModifier::None => match editor.visual_state {
                VisualState::Normal => "8:Vis",
                VisualState::Selecting => "8:V-Sel",
                VisualState::PlacingN => "8:V-N",
                VisualState::PlacingP => "8:V-P",
                VisualState::PlacingMetal => "8:V-M",
            },
        },
        EditMode::MouseSelect => match editor.pending_modifier {
            PendingModifier::Silicon => "9:s-",
            PendingModifier::Metal => "9:m-",
            PendingModifier::Goto => "9:g-",
            PendingModifier::None => {
                if editor.mouse_selection_end.is_some() {
                    "9:Ready"  // Selection finalized, ready for y/d/p
                } else if editor.selection_anchor.is_some() {
                    "9:Sel.."  // Waiting for second click
                } else {
                    "9:MSel"   // No selection yet
                }
            }
        }
    };
    draw_text(mode_text, 40, 10, font, 0xffffff, buffer);

    // Draw the UI panel on the right
    render_panel(editor, font, buffer);

    // Draw the bottom area (help text and tabs)
    render_bottom_area(editor, font, sim, sim_running, sim_waveform_index, buffer);

    // Draw dialog overlay if open
    if let DialogState::SaveSnippet { ref name } = editor.dialog {
        render_dialog("SAVE DESIGN SNIPPET", "Enter name for snippet:", name, font, buffer);
    }
    if let DialogState::SaveDesign { ref name } = editor.dialog {
        render_dialog("SAVE DESIGN", "Enter name for design:", name, font, buffer);
    }
}

/// Render a modal dialog
fn render_dialog(title: &str, prompt: &str, input: &str, font: &HashMap<char, Vec<Vec<bool>>>, buffer: &mut [u32]) {
    let dialog_w = 300;
    let dialog_h = 120;
    let dialog_x = (WINDOW_WIDTH - dialog_w) / 2;
    let dialog_y = (GRID_PIXEL_HEIGHT - dialog_h) / 2;

    // Darken background
    for y in 0..WINDOW_HEIGHT {
        for x in 0..WINDOW_WIDTH {
            buffer[y * WINDOW_WIDTH + x] = alpha_blend(0x000000, buffer[y * WINDOW_WIDTH + x], 0.3);
        }
    }

    // Draw dialog background
    for y in dialog_y..dialog_y + dialog_h {
        for x in dialog_x..dialog_x + dialog_w {
            if x < WINDOW_WIDTH && y < WINDOW_HEIGHT {
                buffer[y * WINDOW_WIDTH + x] = COLOR_BUTTON_BG;
            }
        }
    }

    // Draw dialog border
    for x in dialog_x..dialog_x + dialog_w {
        if dialog_y < WINDOW_HEIGHT {
            buffer[dialog_y * WINDOW_WIDTH + x] = COLOR_BUTTON_LIGHT;
        }
        if dialog_y + dialog_h - 1 < WINDOW_HEIGHT {
            buffer[(dialog_y + dialog_h - 1) * WINDOW_WIDTH + x] = COLOR_BUTTON_DARK;
        }
    }
    for y in dialog_y..dialog_y + dialog_h {
        if y < WINDOW_HEIGHT {
            buffer[y * WINDOW_WIDTH + dialog_x] = COLOR_BUTTON_LIGHT;
            if dialog_x + dialog_w - 1 < WINDOW_WIDTH {
                buffer[y * WINDOW_WIDTH + dialog_x + dialog_w - 1] = COLOR_BUTTON_DARK;
            }
        }
    }

    // Draw title bar
    for y in dialog_y..dialog_y + 24 {
        for x in dialog_x..dialog_x + dialog_w {
            if x < WINDOW_WIDTH && y < WINDOW_HEIGHT {
                buffer[y * WINDOW_WIDTH + x] = COLOR_PANEL_BG;
            }
        }
    }
    draw_text(title, dialog_x + dialog_w / 2, dialog_y + 12, font, COLOR_HELP_TEXT, buffer);

    // Draw prompt
    draw_text(prompt, dialog_x + dialog_w / 2, dialog_y + 45, font, COLOR_BUTTON_TEXT, buffer);

    // Draw input field background
    let input_x = dialog_x + 20;
    let input_y = dialog_y + 60;
    let input_w = dialog_w - 40;
    let input_h = 24;
    for y in input_y..input_y + input_h {
        for x in input_x..input_x + input_w {
            if x < WINDOW_WIDTH && y < WINDOW_HEIGHT {
                buffer[y * WINDOW_WIDTH + x] = 0xffffff; // White input background
            }
        }
    }

    // Draw input text (left-aligned)
    let display_text = if input.is_empty() { "snippet" } else { input };
    draw_text_left(display_text, input_x + 5, input_y + input_h / 2, font, if input.is_empty() { 0x808080 } else { COLOR_BUTTON_TEXT }, buffer);

    // Draw cursor - calculate actual text width from font
    let mut text_width = 0;
    for ch in input.chars() {
        if let Some(glyph) = font.get(&ch) {
            text_width += glyph.get(0).map(|r| r.len()).unwrap_or(0) + 1;
        }
    }
    let cursor_x = input_x + 5 + text_width;
    if cursor_x < input_x + input_w - 5 {
        for y in input_y + 4..input_y + input_h - 4 {
            if y < WINDOW_HEIGHT && cursor_x < WINDOW_WIDTH {
                buffer[y * WINDOW_WIDTH + cursor_x] = COLOR_BUTTON_TEXT;
            }
        }
    }

    // Draw hint
    draw_text("Enter: save | Esc: cancel", dialog_x + dialog_w / 2, dialog_y + dialog_h - 15, font, 0x606060, buffer);
}

/// Get context-sensitive help text based on current mode/state
fn get_help_text(editor: &EditorState) -> &'static str {
    match editor.mode {
        EditMode::Visual => match editor.pending_modifier {
            PendingModifier::Silicon => "d:del silicon | e:find silicon | Esc:cancel",
            PendingModifier::Metal => "d:del metal | e:find metal | Esc:cancel",
            PendingModifier::Goto => "g:top | e:bottom | d:half-down | u:half-up | h:half-right | b:half-left",
            PendingModifier::None => match editor.visual_state {
                VisualState::Normal => "hjkl:move w/b:fast e:find | v:select | -+=.d y p r",
                VisualState::Selecting => "hjkl:extend | y:yank | d/sd/md:delete | Esc:cancel",
                VisualState::PlacingN => "hjkl:draw N-silicon | Esc:stop",
                VisualState::PlacingP => "hjkl:draw P-silicon | Esc:stop",
                VisualState::PlacingMetal => "hjkl:draw metal | Esc:stop",
            },
        },
        EditMode::MouseSelect => match editor.pending_modifier {
            PendingModifier::Silicon => "d:del silicon | Esc:cancel",
            PendingModifier::Metal => "d:del metal | Esc:cancel",
            _ => {
                if editor.mouse_selection_end.is_some() {
                    "y:yank | d/sd/md:delete | p/RClick:paste | r:rotate | Click:new | Esc:clear"
                } else if editor.selection_anchor.is_some() {
                    "Click to set 2nd corner | Esc:cancel"
                } else {
                    "Click to set 1st corner | p/RClick:paste | r:rotate"
                }
            }
        }
        EditMode::NSilicon => "Click start, click end to place N-silicon | Esc/RClick:cancel",
        EditMode::PSilicon => "Click start, click end to place P-silicon | Esc/RClick:cancel",
        EditMode::Metal => "Click start, click end to place metal | Esc/RClick:cancel",
        EditMode::Via => "LClick:place via | RClick:delete via",
        EditMode::DeleteMetal => "Click/drag to delete metal",
        EditMode::DeleteSilicon => "Click/drag to delete silicon",
        EditMode::DeleteAll => "Click/drag to delete everything",
    }
}

/// Render the bottom area with help text, tabs, and content
fn render_bottom_area(editor: &EditorState, font: &HashMap<char, Vec<Vec<bool>>>, sim: &SimState, sim_running: bool, sim_waveform_index: usize, buffer: &mut [u32]) {
    let help_y = GRID_PIXEL_HEIGHT;
    let tab_y = help_y + HELP_AREA_HEIGHT;
    let content_y = tab_y + TAB_HEIGHT;

    // Draw help area background
    for y in help_y..help_y + HELP_AREA_HEIGHT {
        for x in 0..WINDOW_WIDTH {
            buffer[y * WINDOW_WIDTH + x] = COLOR_HELP_BG;
        }
    }

    // Draw help text
    let help_text = get_help_text(editor);
    draw_text(help_text, WINDOW_WIDTH / 2, help_y + HELP_AREA_HEIGHT / 2, font, COLOR_HELP_TEXT, buffer);

    // Draw tabs
    let tabs = [
        (Tab::Specifications, "Data"),
        (Tab::Verification, "Verify"),
        (Tab::DesignSnippets, "Snippets"),
        (Tab::Designs, "Designs"),
        (Tab::Help, "Help"),
        (Tab::Menu, "Menu"),
    ];

    let tab_width = WINDOW_WIDTH / tabs.len();
    for (i, (tab, label)) in tabs.iter().enumerate() {
        let tx = i * tab_width;
        let is_active = editor.active_tab == *tab;
        let bg_color = if is_active { COLOR_TAB_ACTIVE_BG } else { COLOR_TAB_BG };

        // Draw tab background (just the tab bar, not content)
        for y in tab_y..content_y {
            for x in tx..tx + tab_width {
                if x < WINDOW_WIDTH {
                    let color = if x == tx { COLOR_BUTTON_LIGHT } else if x == tx + tab_width - 1 { COLOR_BUTTON_DARK } else { bg_color };
                    buffer[y * WINDOW_WIDTH + x] = color;
                }
            }
        }

        // Draw tab text
        let text_x = tx + tab_width / 2;
        let text_y = tab_y + TAB_HEIGHT / 2;
        draw_text(label, text_x, text_y, font, COLOR_TAB_TEXT, buffer);
    }

    // Draw content area background
    for y in content_y..WINDOW_HEIGHT {
        for x in 0..WINDOW_WIDTH {
            buffer[y * WINDOW_WIDTH + x] = 0xd0d0d0; // Light gray content background
        }
    }

    // Draw tab content
    match editor.active_tab {
        Tab::Verification => {
            render_verification_tab(editor, content_y, font, sim, sim_running, sim_waveform_index, buffer);
        }
        Tab::DesignSnippets => {
            render_snippets_tab(editor, content_y, font, buffer);
        }
        Tab::Designs => {
            render_designs_tab(editor, content_y, font, buffer);
        }
        Tab::Help => {
            let help_lines = [
                "Shift+arrows: switch tabs | Shift+j/k: scroll snippets",
                "Shift+D: delete snippet | y: yank | p: paste | r: rotate",
                "v: start selection | d: delete | sd/md: delete silicon/metal",
            ];
            for (i, line) in help_lines.iter().enumerate() {
                draw_text(line, 10, content_y + 15 + i * 16, font, COLOR_BUTTON_TEXT, buffer);
            }
        }
        Tab::Menu => {
            render_menu_tab(editor, content_y, font, buffer);
        }
        Tab::Specifications => {
            render_specifications_tab(editor, content_y, font, buffer);
        }
    }
}

/// Render the menu tab content with music selection
fn render_menu_tab(editor: &EditorState, content_y: usize, font: &HashMap<char, Vec<Vec<bool>>>, buffer: &mut [u32]) {
    // Title
    draw_text_left("Music Selection", 10, content_y + 12, font, COLOR_BUTTON_TEXT, buffer);

    // Music track buttons
    let tracks = [
        MusicTrack::None,
        MusicTrack::AnalogSequence,
        MusicTrack::GroovyBeat,
        MusicTrack::RetroLoop,
    ];

    let btn_x = 10;
    let btn_w = 180;
    let btn_h = 28;
    let btn_spacing = 6;

    for (i, track) in tracks.iter().enumerate() {
        let btn_y = content_y + 32 + i * (btn_h + btn_spacing);
        let is_selected = editor.current_music == *track;

        // Draw button background
        for y in btn_y..btn_y + btn_h {
            for x in btn_x..btn_x + btn_w {
                if x < WINDOW_WIDTH && y < WINDOW_HEIGHT {
                    let color = if y == btn_y || x == btn_x {
                        COLOR_BUTTON_LIGHT
                    } else if y == btn_y + btn_h - 1 || x == btn_x + btn_w - 1 {
                        COLOR_BUTTON_DARK
                    } else {
                        COLOR_BUTTON_BG
                    };
                    buffer[y * WINDOW_WIDTH + x] = color;
                }
            }
        }

        // Draw selection indicator (green border)
        if is_selected {
            let border = 2;
            for y in btn_y..btn_y + btn_h {
                for x in btn_x..btn_x + btn_w {
                    if x < WINDOW_WIDTH && y < WINDOW_HEIGHT {
                        let on_border = x < btn_x + border || x >= btn_x + btn_w - border
                            || y < btn_y + border || y >= btn_y + btn_h - border;
                        if on_border {
                            buffer[y * WINDOW_WIDTH + x] = 0x00aa00; // Green border for selected
                        }
                    }
                }
            }
        }

        // Draw track name
        let text_y = btn_y + btn_h / 2;
        draw_text_left(track.display_name(), btn_x + 10, text_y, font, COLOR_BUTTON_TEXT, buffer);
    }

    // Attribution text
    let attr_x = btn_x + btn_w + 40;
    draw_text_left("Music from Freesound.org:", attr_x, content_y + 40, font, 0x606060, buffer);
    draw_text_left("Analog Sequence - Xinematix (CC BY 4.0)", attr_x, content_y + 60, font, 0x707070, buffer);
    draw_text_left("Groovy Beat - Seth_Makes_Sounds (CC0)", attr_x, content_y + 78, font, 0x707070, buffer);
    draw_text_left("Retro Loop - ProdByRey (CC0)", attr_x, content_y + 96, font, 0x707070, buffer);
}

/// Render the specifications tab content
fn render_specifications_tab(editor: &EditorState, content_y: usize, font: &HashMap<char, Vec<Vec<bool>>>, buffer: &mut [u32]) {
    // Get the currently selected level
    if let Some(level) = editor.levels.get(editor.selected_level) {
        // Check if level is completed for coloring
        let completed = editor.save_data.is_level_complete(&level.name);

        // Draw specification lines starting near the top
        // The spec text itself contains the part number/header, so no separate title needed
        let line_height = 14;
        let start_y = content_y + 8;  // Start closer to top (was +32)

        for (i, line) in level.specification.iter().enumerate() {
            let y = start_y + i * line_height;
            if y + line_height < WINDOW_HEIGHT {
                // First line (part number) is green if completed
                let color = if i == 0 && completed { 0x00aa00 } else { 0x404040 };
                draw_text_left(line, 10, y, font, color, buffer);
            }
        }
    } else {
        draw_text_left("No level selected", 10, content_y + 20, font, 0x808080, buffer);
        draw_text_left("Use Shift+Up/Down on Verify tab to select a level", 10, content_y + 40, font, 0x808080, buffer);
    }
}

/// Render the snippets tab content
fn render_snippets_tab(editor: &EditorState, content_y: usize, font: &HashMap<char, Vec<Vec<bool>>>, buffer: &mut [u32]) {
    let list_width = WINDOW_WIDTH / 2;
    let preview_x = list_width;

    // Draw divider between list and preview
    for y in content_y..WINDOW_HEIGHT {
        if list_width < WINDOW_WIDTH {
            buffer[y * WINDOW_WIDTH + list_width] = COLOR_BUTTON_DARK;
        }
    }

    // Draw snippet list
    if editor.snippets.is_empty() {
        draw_text("No snippets saved", 10, content_y + 20, font, 0x808080, buffer);
        draw_text("Select area with 'v', then 'y' to save", 10, content_y + 40, font, 0x808080, buffer);
    } else {
        for (i, snippet) in editor.snippets.iter().enumerate() {
            let item_y = content_y + 8 + i * 18;
            if item_y + 16 > WINDOW_HEIGHT {
                break;
            }

            // Highlight selected snippet
            if i == editor.selected_snippet {
                for y in item_y - 2..item_y + 14 {
                    for x in 2..list_width - 2 {
                        if y < WINDOW_HEIGHT {
                            buffer[y * WINDOW_WIDTH + x] = 0xa0c0ff; // Light blue highlight
                        }
                    }
                }
            }

            draw_text(&snippet.name, 10, item_y + 6, font, COLOR_BUTTON_TEXT, buffer);
        }
    }

    // Draw preview of selected snippet
    if let Some(snippet) = editor.snippets.get(editor.selected_snippet) {
        let preview_cell_size = 6; // Small cells for preview
        let preview_start_x = preview_x + 10;
        let preview_start_y = content_y + 10;

        // Draw preview grid background
        let preview_w = snippet.width * preview_cell_size;
        let preview_h = snippet.height * preview_cell_size;
        for y in preview_start_y..preview_start_y + preview_h {
            for x in preview_start_x..preview_start_x + preview_w {
                if x < WINDOW_WIDTH && y < WINDOW_HEIGHT {
                    buffer[y * WINDOW_WIDTH + x] = COLOR_BACKGROUND;
                }
            }
        }

        // Draw snippet cells
        for (sy, row) in snippet.nodes.iter().enumerate() {
            for (sx, node) in row.iter().enumerate() {
                let px = preview_start_x + sx * preview_cell_size;
                let py = preview_start_y + sy * preview_cell_size;

                // Draw silicon
                let silicon_color = match node.silicon {
                    Silicon::N => Some(COLOR_N_TYPE),
                    Silicon::P => Some(COLOR_P_TYPE),
                    Silicon::Gate { channel } => Some(match channel {
                        SiliconKind::N => COLOR_N_TYPE,
                        SiliconKind::P => COLOR_P_TYPE,
                    }),
                    Silicon::None => None,
                };

                if let Some(color) = silicon_color {
                    for dy in 1..preview_cell_size - 1 {
                        for dx in 1..preview_cell_size - 1 {
                            let x = px + dx;
                            let y = py + dy;
                            if x < WINDOW_WIDTH && y < WINDOW_HEIGHT {
                                buffer[y * WINDOW_WIDTH + x] = color;
                            }
                        }
                    }
                }

                // Draw metal (semi-transparent overlay)
                if node.metal {
                    for dy in 1..preview_cell_size - 1 {
                        for dx in 1..preview_cell_size - 1 {
                            let x = px + dx;
                            let y = py + dy;
                            if x < WINDOW_WIDTH && y < WINDOW_HEIGHT {
                                buffer[y * WINDOW_WIDTH + x] = alpha_blend(COLOR_METAL, buffer[y * WINDOW_WIDTH + x], 0.5);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Render the designs tab content
fn render_designs_tab(editor: &EditorState, content_y: usize, font: &HashMap<char, Vec<Vec<bool>>>, buffer: &mut [u32]) {
    let list_width = WINDOW_WIDTH / 2;

    // Draw divider between list and preview
    for y in content_y..WINDOW_HEIGHT {
        if list_width < WINDOW_WIDTH {
            buffer[y * WINDOW_WIDTH + list_width] = COLOR_BUTTON_DARK;
        }
    }

    // Draw design list
    if editor.designs.is_empty() {
        draw_text("No designs saved", 10, content_y + 20, font, 0x808080, buffer);
        draw_text("Press 'w' or Shift+W to save design", 10, content_y + 40, font, 0x808080, buffer);
    } else {
        for (i, design) in editor.designs.iter().enumerate() {
            let item_y = content_y + 8 + i * 18;
            if item_y + 16 > WINDOW_HEIGHT {
                break;
            }

            // Highlight selected design
            if i == editor.selected_design {
                for y in item_y - 2..item_y + 14 {
                    for x in 2..list_width - 2 {
                        if y < WINDOW_HEIGHT {
                            buffer[y * WINDOW_WIDTH + x] = 0xa0c0ff; // Light blue highlight
                        }
                    }
                }
            }

            draw_text(&design.name, 10, item_y + 6, font, COLOR_BUTTON_TEXT, buffer);
        }
    }

    // Draw instructions on the right side
    let preview_x = list_width + 10;
    draw_text_left("Shift+J/K: navigate", preview_x, content_y + 20, font, 0x606060, buffer);
    draw_text_left("Shift+R: load design", preview_x, content_y + 40, font, 0x606060, buffer);
    draw_text_left("Shift+D: delete design", preview_x, content_y + 60, font, 0x606060, buffer);
    draw_text_left("Shift+W: save design", preview_x, content_y + 80, font, 0x606060, buffer);
}

/// Render the verification tab content (levels list and waveforms)
fn render_verification_tab(editor: &EditorState, content_y: usize, font: &HashMap<char, Vec<Vec<bool>>>, sim: &SimState, sim_running: bool, sim_waveform_index: usize, buffer: &mut [u32]) {
    let list_width = 150;
    let waveform_x = list_width + 10;

    // Draw divider between list and waveform area
    for y in content_y..WINDOW_HEIGHT {
        if list_width < WINDOW_WIDTH {
            buffer[y * WINDOW_WIDTH + list_width] = COLOR_BUTTON_DARK;
        }
    }

    // Draw level list (max 10 visible with scrolling)
    let max_visible = 10;
    if editor.levels.is_empty() {
        draw_text("No levels found", 10, content_y + 20, font, 0x808080, buffer);
        draw_text("Add .json files to levels/", 10, content_y + 40, font, 0x808080, buffer);
    } else {
        // Draw scroll indicator if needed
        if editor.level_scroll_offset > 0 {
            draw_text_left("^", list_width / 2, content_y + 2, font, 0x606060, buffer);
        }

        let visible_levels = editor.levels.iter().enumerate()
            .skip(editor.level_scroll_offset)
            .take(max_visible);

        for (idx, (i, level)) in visible_levels.enumerate() {
            let item_y = content_y + 8 + idx * 18;
            if item_y + 16 > WINDOW_HEIGHT {
                break;
            }

            // Highlight selected level
            if i == editor.selected_level {
                for y in item_y - 2..item_y + 14 {
                    for x in 2..list_width - 2 {
                        if y < WINDOW_HEIGHT {
                            buffer[y * WINDOW_WIDTH + x] = 0xa0c0ff; // Light blue highlight
                        }
                    }
                }
            }

            // Completion indicator and color
            let completed = editor.save_data.is_level_complete(&level.name);
            let indicator = if completed { "*" } else { " " };
            let display_name = format!("{} {}", indicator, level.name);
            let text_color = if completed { 0x00aa00 } else { COLOR_BUTTON_TEXT };
            draw_text(&display_name, 6, item_y + 6, font, text_color, buffer);
        }

        // Draw scroll indicator if more below
        if editor.level_scroll_offset + max_visible < editor.levels.len() {
            let bottom_y = content_y + 8 + max_visible * 18;
            draw_text_left("v", list_width / 2, bottom_y, font, 0x606060, buffer);
        }
    }

    // Check if we have simulation output to display
    let has_sim_output = !sim.output_history.is_empty();

    // Draw waveform preview for selected level
    if let Some(level) = editor.levels.get(editor.selected_level) {
        // Draw waveforms
        let wave_start_y = content_y + 30;
        let wave_height = 12;
        let time_step_width = 3;  // Narrower ticks (was 5)
        let wave_x_start = waveform_x + 40;

        // Only show waveforms with display: true
        let displayed_waveforms: Vec<_> = level.waveforms.iter().filter(|w| w.display).collect();

        // Draw gray dashed vertical lines every 2 ticks
        let max_ticks = level.waveforms.iter().map(|w| w.values.len()).max().unwrap_or(64);
        let wave_area_height = displayed_waveforms.len() * (wave_height + 8);
        for tick in (0..max_ticks).step_by(2) {
            let x = wave_x_start + tick * time_step_width;
            if x >= WINDOW_WIDTH {
                break;
            }
            // Draw dashed line (every other pixel)
            for y in wave_start_y..(wave_start_y + wave_area_height).min(WINDOW_HEIGHT) {
                if (y % 4) < 2 {  // Dashed pattern: 2 on, 2 off
                    buffer[y * WINDOW_WIDTH + x] = 0xc0c0c0;  // Light gray
                }
            }
        }

        for (wi, waveform) in displayed_waveforms.iter().enumerate() {
            let y_base = wave_start_y + wi * (wave_height + 8);
            if y_base + wave_height > WINDOW_HEIGHT {
                break;
            }

            // Label
            let pin_label = level.pins.get(waveform.pin_index).map(|s| s.as_str()).unwrap_or("?");
            draw_text_left(pin_label, waveform_x, y_base + wave_height / 2, font, 0x404040, buffer);

            // Draw expected waveform (inputs always blue, outputs gray when sim has output, otherwise green)
            let chars: Vec<char> = waveform.values.chars().collect();
            for (t, &ch) in chars.iter().enumerate() {
                let x = wave_x_start + t * time_step_width;
                let y_high = y_base;
                let y_low = y_base + wave_height - 2;

                let value = ch == '1';
                let y = if value { y_high } else { y_low };

                // Color: blue for inputs, gray for expected outputs when sim running, green otherwise
                let color = if waveform.is_input {
                    0x0000ff // Blue for inputs
                } else if has_sim_output {
                    0x808080 // Gray for expected outputs when we have sim data
                } else {
                    0x00aa00 // Green for outputs when no sim data
                };

                // Draw horizontal line at current level
                for dx in 0..time_step_width {
                    if x + dx < WINDOW_WIDTH && y < WINDOW_HEIGHT {
                        buffer[y * WINDOW_WIDTH + x + dx] = color;
                    }
                }

                // Draw vertical transition if needed
                if t > 0 {
                    let prev_value = chars[t - 1] == '1';
                    if prev_value != value {
                        for dy in y_high..=y_low {
                            if x < WINDOW_WIDTH && dy < WINDOW_HEIGHT {
                                buffer[dy * WINDOW_WIDTH + x] = color;
                            }
                        }
                    }
                }
            }

            // Draw actual simulated output on top (in green) for output waveforms
            if !waveform.is_input && has_sim_output {
                let pin_idx = waveform.pin_index;
                for t in 0..sim.output_history.len() {
                    let value = sim.output_history[t][pin_idx];
                    let x = wave_x_start + t * time_step_width;
                    let y_high = y_base;
                    let y_low = y_base + wave_height - 2;
                    let y = if value { y_high } else { y_low };

                    // Draw horizontal line at current level (green for actual)
                    for dx in 0..time_step_width {
                        if x + dx < WINDOW_WIDTH && y < WINDOW_HEIGHT {
                            buffer[y * WINDOW_WIDTH + x + dx] = 0x00aa00;
                        }
                    }

                    // Draw vertical transition if needed
                    if t > 0 && sim.output_history[t - 1][pin_idx] != value {
                        for dy in y_high..=y_low {
                            if x < WINDOW_WIDTH && dy < WINDOW_HEIGHT {
                                buffer[dy * WINDOW_WIDTH + x] = 0x00aa00;
                            }
                        }
                    }
                }
            }
        }

        // Draw current simulation position marker
        if sim_running {
            let marker_x = wave_x_start + sim_waveform_index * time_step_width;
            if marker_x < WINDOW_WIDTH {
                let marker_height = displayed_waveforms.len() * (wave_height + 8) + 10;
                for y in wave_start_y..WINDOW_HEIGHT.min(wave_start_y + marker_height) {
                    buffer[y * WINDOW_WIDTH + marker_x] = 0xff0000; // Red vertical line
                }
            }
        }
    }

    // Show simulation status or result
    if sim_running {
        draw_text_left("RUNNING (Space to stop)", waveform_x, content_y + 10, font, 0x008800, buffer);
    } else if let (Some(accuracy), Some(passed)) = (sim.last_accuracy, sim.last_passed) {
        // Show accuracy result
        let pct = (accuracy * 100.0) as u32;
        let result_text = if passed {
            format!("{}% PASS", pct)
        } else {
            format!("{}% FAIL", pct)
        };
        let result_color = if passed { 0x00aa00 } else { 0xff0000 };
        draw_text_left(&result_text, waveform_x, content_y + 10, font, result_color, buffer);
    } else {
        draw_text_left("Press Space to simulate", waveform_x, content_y + 10, font, 0x606060, buffer);
    }
}

/// Check which tab was clicked (if any)
fn get_clicked_tab(mx: usize, my: usize) -> Option<Tab> {
    let tab_y = GRID_PIXEL_HEIGHT + HELP_AREA_HEIGHT;
    let content_y = tab_y + TAB_HEIGHT;

    // Check if y is in tab bar area (not the content area below)
    if my < tab_y || my >= content_y {
        return None;
    }

    let tabs = [
        Tab::Specifications,
        Tab::Verification,
        Tab::DesignSnippets,
        Tab::Designs,
        Tab::Help,
        Tab::Menu,
    ];

    let tab_width = WINDOW_WIDTH / tabs.len();
    let tab_index = mx / tab_width;
    if tab_index < tabs.len() {
        Some(tabs[tab_index])
    } else {
        None
    }
}

/// Check which music button was clicked (if any) in the Menu tab
fn get_clicked_music_button(mx: usize, my: usize) -> Option<MusicTrack> {
    let content_y = GRID_PIXEL_HEIGHT + HELP_AREA_HEIGHT + TAB_HEIGHT;
    let btn_x = 10;
    let btn_w = 180;
    let btn_h = 28;
    let btn_spacing = 6;

    // Check if x is within button bounds
    if mx < btn_x || mx >= btn_x + btn_w {
        return None;
    }

    let tracks = [
        MusicTrack::None,
        MusicTrack::AnalogSequence,
        MusicTrack::GroovyBeat,
        MusicTrack::RetroLoop,
    ];

    for (i, track) in tracks.iter().enumerate() {
        let btn_y = content_y + 32 + i * (btn_h + btn_spacing);
        if my >= btn_y && my < btn_y + btn_h {
            return Some(*track);
        }
    }

    None
}

/// Check which panel button was clicked (if any)
fn get_clicked_button(mx: usize, my: usize) -> Option<EditMode> {
    let panel_x = GRID_PIXEL_WIDTH;
    let btn_x = panel_x + BUTTON_MARGIN;
    let btn_w = PANEL_WIDTH - BUTTON_MARGIN * 2;

    // Check if x is within button bounds
    if mx < btn_x || mx >= btn_x + btn_w {
        return None;
    }

    let modes = [
        EditMode::NSilicon,
        EditMode::PSilicon,
        EditMode::Metal,
        EditMode::Via,
        EditMode::DeleteMetal,
        EditMode::DeleteSilicon,
        EditMode::DeleteAll,
        EditMode::Visual,
        EditMode::MouseSelect,
    ];

    for (i, mode) in modes.iter().enumerate() {
        let btn_y = BUTTON_MARGIN + i * (BUTTON_HEIGHT + BUTTON_MARGIN);
        if my >= btn_y && my < btn_y + BUTTON_HEIGHT {
            return Some(*mode);
        }
    }

    None
}

/// Render the UI panel with mode buttons
fn render_panel(editor: &EditorState, font: &HashMap<char, Vec<Vec<bool>>>, buffer: &mut [u32]) {
    let panel_x = GRID_PIXEL_WIDTH;

    // Fill panel background
    for y in 0..WINDOW_HEIGHT {
        for x in panel_x..WINDOW_WIDTH {
            buffer[y * WINDOW_WIDTH + x] = COLOR_PANEL_BG;
        }
    }

    // Button definitions: (mode, label, number, icon_type)
    let buttons: [(EditMode, &str, &str, ButtonIcon); 9] = [
        (EditMode::NSilicon, "N-SI", "1", ButtonIcon::NSilicon),
        (EditMode::PSilicon, "P-SI", "2", ButtonIcon::PSilicon),
        (EditMode::Metal, "METAL", "3", ButtonIcon::Metal),
        (EditMode::Via, "VIA", "4", ButtonIcon::Via),
        (EditMode::DeleteMetal, "DEL M", "5", ButtonIcon::DeleteX),
        (EditMode::DeleteSilicon, "DEL S", "6", ButtonIcon::DeleteX),
        (EditMode::DeleteAll, "DEL", "7", ButtonIcon::DeleteX),
        (EditMode::Visual, "VISUAL", "8", ButtonIcon::Select),
        (EditMode::MouseSelect, "SELECT", "9", ButtonIcon::Select),
    ];

    for (i, (mode, label, number, icon)) in buttons.iter().enumerate() {
        let btn_y = BUTTON_MARGIN + i * (BUTTON_HEIGHT + BUTTON_MARGIN);
        let btn_x = panel_x + BUTTON_MARGIN;
        let btn_w = PANEL_WIDTH - BUTTON_MARGIN * 2;
        let btn_h = BUTTON_HEIGHT;

        let is_active = editor.mode == *mode;
        draw_button(btn_x, btn_y, btn_w, btn_h, label, number, *icon, is_active, font, buffer);
    }
}

#[derive(Clone, Copy)]
enum ButtonIcon {
    NSilicon,
    PSilicon,
    Metal,
    Via,
    DeleteX,
    Select,
}

/// Draw a panel button with icon, label, and number
fn draw_button(
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    label: &str,
    number: &str,
    icon: ButtonIcon,
    active: bool,
    font: &HashMap<char, Vec<Vec<bool>>>,
    buffer: &mut [u32],
) {
    // Draw button background with beveled edges
    for py in y..y + h {
        for px in x..x + w {
            if px < WINDOW_WIDTH && py < WINDOW_HEIGHT {
                let color = if py == y || px == x {
                    COLOR_BUTTON_LIGHT
                } else if py == y + h - 1 || px == x + w - 1 {
                    COLOR_BUTTON_DARK
                } else {
                    COLOR_BUTTON_BG
                };
                buffer[py * WINDOW_WIDTH + px] = color;
            }
        }
    }

    // Draw active border (red outline) if this is the current mode
    if active {
        let border = 2;
        for py in y..y + h {
            for px in x..x + w {
                if px < WINDOW_WIDTH && py < WINDOW_HEIGHT {
                    let on_border = px < x + border || px >= x + w - border
                        || py < y + border || py >= y + h - border;
                    if on_border {
                        buffer[py * WINDOW_WIDTH + px] = COLOR_BUTTON_ACTIVE;
                    }
                }
            }
        }
    }

    // Draw icon (left side of button)
    let icon_x = x + 4;
    let icon_y = y + 4;
    let icon_size = 20;
    draw_button_icon(icon_x, icon_y, icon_size, icon, buffer);

    // Draw label text (center-right area)
    let text_x = x + 28;
    let text_y = y + 10;
    draw_text(label, text_x + 20, text_y + 6, font, COLOR_BUTTON_TEXT, buffer);

    // Draw number (bottom right corner)
    let num_x = x + w - 12;
    let num_y = y + h - 14;
    draw_text(number, num_x, num_y, font, COLOR_BUTTON_TEXT, buffer);
}

/// Draw a small icon representing the button's function
fn draw_button_icon(x: usize, y: usize, size: usize, icon: ButtonIcon, buffer: &mut [u32]) {
    match icon {
        ButtonIcon::NSilicon => {
            // Red/brown filled square
            for py in y + 2..y + size - 2 {
                for px in x + 2..x + size - 2 {
                    if px < WINDOW_WIDTH && py < WINDOW_HEIGHT {
                        buffer[py * WINDOW_WIDTH + px] = COLOR_N_TYPE;
                    }
                }
            }
            // Black outline
            for px in x + 2..x + size - 2 {
                if y + 2 < WINDOW_HEIGHT { buffer[(y + 2) * WINDOW_WIDTH + px] = COLOR_OUTLINE; }
                if y + size - 3 < WINDOW_HEIGHT { buffer[(y + size - 3) * WINDOW_WIDTH + px] = COLOR_OUTLINE; }
            }
            for py in y + 2..y + size - 2 {
                if x + 2 < WINDOW_WIDTH { buffer[py * WINDOW_WIDTH + x + 2] = COLOR_OUTLINE; }
                if x + size - 3 < WINDOW_WIDTH { buffer[py * WINDOW_WIDTH + x + size - 3] = COLOR_OUTLINE; }
            }
        }
        ButtonIcon::PSilicon => {
            // Yellow filled square
            for py in y + 2..y + size - 2 {
                for px in x + 2..x + size - 2 {
                    if px < WINDOW_WIDTH && py < WINDOW_HEIGHT {
                        buffer[py * WINDOW_WIDTH + px] = COLOR_P_TYPE;
                    }
                }
            }
            // Black outline
            for px in x + 2..x + size - 2 {
                if y + 2 < WINDOW_HEIGHT { buffer[(y + 2) * WINDOW_WIDTH + px] = COLOR_OUTLINE; }
                if y + size - 3 < WINDOW_HEIGHT { buffer[(y + size - 3) * WINDOW_WIDTH + px] = COLOR_OUTLINE; }
            }
            for py in y + 2..y + size - 2 {
                if x + 2 < WINDOW_WIDTH { buffer[py * WINDOW_WIDTH + x + 2] = COLOR_OUTLINE; }
                if x + size - 3 < WINDOW_WIDTH { buffer[py * WINDOW_WIDTH + x + size - 3] = COLOR_OUTLINE; }
            }
        }
        ButtonIcon::Metal => {
            // Gray layered rectangles (like original)
            let colors = [0xcccccc, 0xaaaaaa, 0x888888];
            for (i, &color) in colors.iter().enumerate() {
                let offset = i * 3;
                for py in y + 4 + offset..y + 12 + offset {
                    for px in x + 3..x + size - 3 {
                        if px < WINDOW_WIDTH && py < WINDOW_HEIGHT {
                            buffer[py * WINDOW_WIDTH + px] = color;
                        }
                    }
                }
            }
        }
        ButtonIcon::Via => {
            // Circle/ring
            let cx = x + size / 2;
            let cy = y + size / 2;
            let outer_r = 7i32;
            let inner_r = 4i32;
            for dy in -outer_r..=outer_r {
                for dx in -outer_r..=outer_r {
                    let dist_sq = dx * dx + dy * dy;
                    let px = (cx as i32 + dx) as usize;
                    let py = (cy as i32 + dy) as usize;
                    if px < WINDOW_WIDTH && py < WINDOW_HEIGHT {
                        if dist_sq <= outer_r * outer_r && dist_sq >= inner_r * inner_r {
                            buffer[py * WINDOW_WIDTH + px] = COLOR_VIA;
                        }
                    }
                }
            }
        }
        ButtonIcon::DeleteX => {
            // Red X
            let cx = x + size / 2;
            let cy = y + size / 2;
            for i in 0..12i32 {
                for t in -1..=1 {
                    let px1 = (cx as i32 - 5 + i) as usize;
                    let py1 = (cy as i32 - 5 + i + t) as usize;
                    let px2 = (cx as i32 + 5 - i) as usize;
                    let py2 = (cy as i32 - 5 + i + t) as usize;
                    if px1 < WINDOW_WIDTH && py1 < WINDOW_HEIGHT {
                        buffer[py1 * WINDOW_WIDTH + px1] = COLOR_DELETE_X;
                    }
                    if px2 < WINDOW_WIDTH && py2 < WINDOW_HEIGHT {
                        buffer[py2 * WINDOW_WIDTH + px2] = COLOR_DELETE_X;
                    }
                }
            }
        }
        ButtonIcon::Select => {
            // Dashed rectangle
            for px in x + 4..x + size - 4 {
                if (px - x) % 4 < 2 {
                    if y + 4 < WINDOW_HEIGHT { buffer[(y + 4) * WINDOW_WIDTH + px] = COLOR_OUTLINE; }
                    if y + size - 5 < WINDOW_HEIGHT { buffer[(y + size - 5) * WINDOW_WIDTH + px] = COLOR_OUTLINE; }
                }
            }
            for py in y + 4..y + size - 4 {
                if (py - y) % 4 < 2 {
                    if x + 4 < WINDOW_WIDTH { buffer[py * WINDOW_WIDTH + x + 4] = COLOR_OUTLINE; }
                    if x + size - 5 < WINDOW_WIDTH { buffer[py * WINDOW_WIDTH + x + size - 5] = COLOR_OUTLINE; }
                }
            }
        }
    }
}

/// Draw a semi-transparent overlay on a cell
fn draw_cell_overlay(grid_x: usize, grid_y: usize, color: u32, alpha: f32, buffer: &mut [u32]) {
    let x_start = grid_x * CELL_SIZE;
    let y_start = grid_y * CELL_SIZE;

    for y in y_start..(y_start + CELL_SIZE).min(WINDOW_HEIGHT) {
        for x in x_start..(x_start + CELL_SIZE).min(WINDOW_WIDTH) {
            let idx = y * WINDOW_WIDTH + x;
            buffer[idx] = alpha_blend(color, buffer[idx], alpha);
        }
    }
}

/// Draw cursor outline around a cell
fn draw_cursor(grid_x: usize, grid_y: usize, buffer: &mut [u32]) {
    let x_start = grid_x * CELL_SIZE;
    let y_start = grid_y * CELL_SIZE;
    let x_end = (x_start + CELL_SIZE - 1).min(WINDOW_WIDTH - 1);
    let y_end = (y_start + CELL_SIZE - 1).min(WINDOW_HEIGHT - 1);

    // Draw border
    for x in x_start..=x_end {
        if y_start < WINDOW_HEIGHT {
            buffer[y_start * WINDOW_WIDTH + x] = COLOR_CURSOR;
        }
        if y_end < WINDOW_HEIGHT {
            buffer[y_end * WINDOW_WIDTH + x] = COLOR_CURSOR;
        }
    }
    for y in y_start..=y_end {
        if x_start < WINDOW_WIDTH {
            buffer[y * WINDOW_WIDTH + x_start] = COLOR_CURSOR;
        }
        if x_end < WINDOW_WIDTH {
            buffer[y * WINDOW_WIDTH + x_end] = COLOR_CURSOR;
        }
    }
}

fn draw_text(
    text: &str,
    center_x: usize,
    center_y: usize,
    font: &HashMap<char, Vec<Vec<bool>>>,
    color: u32,
    buffer: &mut [u32],
) {
    // Calculate total text width
    let mut total_width = 0;
    let mut max_height = 0;
    for ch in text.chars() {
        if let Some(glyph) = font.get(&ch) {
            if !glyph.is_empty() {
                total_width += glyph[0].len() + 1; // +1 for spacing
                max_height = max_height.max(glyph.len());
            }
        }
    }
    if total_width > 0 {
        total_width -= 1; // Remove trailing space
    }

    // Starting position (centered)
    let start_x = center_x.saturating_sub(total_width / 2);
    let start_y = center_y.saturating_sub(max_height / 2);

    let mut cursor_x = start_x;
    for ch in text.chars() {
        if let Some(glyph) = font.get(&ch) {
            for (row_idx, row) in glyph.iter().enumerate() {
                for (col_idx, &pixel) in row.iter().enumerate() {
                    if pixel {
                        let px = cursor_x + col_idx;
                        let py = start_y + row_idx;
                        if px < WINDOW_WIDTH && py < WINDOW_HEIGHT {
                            buffer[py * WINDOW_WIDTH + px] = color;
                        }
                    }
                }
            }
            cursor_x += glyph.get(0).map(|r| r.len()).unwrap_or(0) + 1;
        }
    }
}

/// Draw text left-aligned at the given position (x is left edge, y is vertical center)
fn draw_text_left(
    text: &str,
    left_x: usize,
    center_y: usize,
    font: &HashMap<char, Vec<Vec<bool>>>,
    color: u32,
    buffer: &mut [u32],
) {
    // Calculate max height for vertical centering
    let mut max_height = 0;
    for ch in text.chars() {
        if let Some(glyph) = font.get(&ch) {
            max_height = max_height.max(glyph.len());
        }
    }

    let start_y = center_y.saturating_sub(max_height / 2);
    let mut cursor_x = left_x;

    for ch in text.chars() {
        if let Some(glyph) = font.get(&ch) {
            for (row_idx, row) in glyph.iter().enumerate() {
                for (col_idx, &pixel) in row.iter().enumerate() {
                    if pixel {
                        let px = cursor_x + col_idx;
                        let py = start_y + row_idx;
                        if px < WINDOW_WIDTH && py < WINDOW_HEIGHT {
                            buffer[py * WINDOW_WIDTH + px] = color;
                        }
                    }
                }
            }
            cursor_x += glyph.get(0).map(|r| r.len()).unwrap_or(0) + 1;
        }
    }
}

fn render_cell(circuit: &Circuit, grid_x: usize, grid_y: usize, pins: &[Pin], buffer: &mut [u32]) {
    let x_start = grid_x * CELL_SIZE;
    let y_start = grid_y * CELL_SIZE;

    // Fill cell interior with beveled effect
    for y in (y_start + 1)..(y_start + CELL_SIZE - 1) {
        for x in (x_start + 1)..(x_start + CELL_SIZE - 1) {
            if x < WINDOW_WIDTH && y < WINDOW_HEIGHT {
                let color = if y == y_start + 1 || x == x_start + 1 {
                    COLOR_CELL_LIGHT
                } else if y == y_start + CELL_SIZE - 2 || x == x_start + CELL_SIZE - 2 {
                    COLOR_CELL_DARK
                } else {
                    COLOR_CELL_MID
                };
                buffer[y * WINDOW_WIDTH + x] = color;
            }
        }
    }

    // Draw grid lines
    for x in x_start..(x_start + CELL_SIZE) {
        if x < WINDOW_WIDTH {
            buffer[y_start * WINDOW_WIDTH + x] = COLOR_GRID_LINE;
        }
    }
    for y in y_start..(y_start + CELL_SIZE) {
        if y < WINDOW_HEIGHT {
            buffer[y * WINDOW_WIDTH + x_start] = COLOR_GRID_LINE;
        }
    }

    // Get node data
    let node = match circuit.get_node(grid_x, grid_y) {
        Some(n) => n,
        None => return,
    };

    // No corner fills for silicon layers
    let no_corner_fills = [false; 4];

    // Determine what to draw based on node's silicon type
    match node.silicon {
        Silicon::None => {}
        Silicon::N => {
            let conns = get_layer_connections(circuit, grid_x, grid_y, Layer::NSilicon);
            draw_layer(buffer, x_start, y_start, &conns, COLOR_N_TYPE, no_corner_fills);
        }
        Silicon::P => {
            let conns = get_layer_connections(circuit, grid_x, grid_y, Layer::PSilicon);
            draw_layer(buffer, x_start, y_start, &conns, COLOR_P_TYPE, no_corner_fills);
        }
        Silicon::Gate { channel } => {
            // Draw channel first (spans full connection), then gate on top (narrower, darker)
            let (channel_layer, gate_layer, channel_color, gate_color) = match channel {
                SiliconKind::P => (Layer::PSilicon, Layer::NSilicon, COLOR_P_TYPE, COLOR_N_GATE),
                SiliconKind::N => (Layer::NSilicon, Layer::PSilicon, COLOR_N_TYPE, COLOR_P_GATE),
            };
            let channel_conns = get_layer_connections(circuit, grid_x, grid_y, channel_layer);
            let gate_conns = get_layer_connections(circuit, grid_x, grid_y, gate_layer);
            // Channel draws as normal wire
            draw_layer(buffer, x_start, y_start, &channel_conns, channel_color, no_corner_fills);
            // Gate draws narrower
            draw_gate_layer(buffer, x_start, y_start, &gate_conns, gate_color);
        }
    }

    // Draw via (between silicon and metal)
    if node.via {
        draw_via(buffer, x_start, y_start);
    }

    // Draw metal layer on top with alpha blending
    if node.metal {
        let conns = get_layer_connections(circuit, grid_x, grid_y, Layer::Metal);
        // For pin cells, fill corners where diagonal neighbors are also pin cells
        let corner_fills = if is_pin_cell(grid_x, grid_y, pins) {
            get_pin_corner_fills(grid_x, grid_y, pins)
        } else {
            no_corner_fills
        };
        draw_layer_alpha(buffer, x_start, y_start, &conns, COLOR_METAL, METAL_ALPHA, corner_fills);
    }
}

/// Render a ghost preview of a snippet at the given position
fn render_snippet_ghost(snippet: &Snippet, dest_x: usize, dest_y: usize, buffer: &mut [u32]) {
    const GHOST_ALPHA: f32 = 0.5;

    // For each cell in the snippet, draw ghost overlays for materials
    for (sy, row) in snippet.nodes.iter().enumerate() {
        for (sx, node) in row.iter().enumerate() {
            let gx = dest_x + sx;
            let gy = dest_y + sy;

            // Skip if outside grid
            if gx >= GRID_WIDTH || gy >= GRID_HEIGHT {
                continue;
            }

            let x_start = gx * CELL_SIZE;
            let y_start = gy * CELL_SIZE;

            // Draw silicon ghost
            match node.silicon {
                Silicon::None => {}
                Silicon::N => {
                    draw_ghost_fill(buffer, x_start, y_start, COLOR_N_TYPE, GHOST_ALPHA);
                }
                Silicon::P => {
                    draw_ghost_fill(buffer, x_start, y_start, COLOR_P_TYPE, GHOST_ALPHA);
                }
                Silicon::Gate { channel } => {
                    let color = match channel {
                        SiliconKind::P => COLOR_P_TYPE,
                        SiliconKind::N => COLOR_N_TYPE,
                    };
                    draw_ghost_fill(buffer, x_start, y_start, color, GHOST_ALPHA);
                }
            }

            // Draw via ghost
            if node.via {
                draw_ghost_fill(buffer, x_start, y_start, 0x808080, GHOST_ALPHA);
            }

            // Draw metal ghost
            if node.metal {
                draw_ghost_fill(buffer, x_start, y_start, COLOR_METAL, GHOST_ALPHA * 0.7);
            }
        }
    }

    // Draw ghost for horizontal edges (just highlight the connection area between cells)
    for (sy, row) in snippet.h_edges.iter().enumerate() {
        for (sx, edge) in row.iter().enumerate() {
            let gx = dest_x + sx;
            let gy = dest_y + sy;
            if gx + 1 >= GRID_WIDTH || gy >= GRID_HEIGHT {
                continue;
            }

            let x_start = gx * CELL_SIZE;
            let y_start = gy * CELL_SIZE;

            // Draw edge indicator between cells
            if edge.n_silicon {
                draw_ghost_edge_h(buffer, x_start, y_start, COLOR_N_TYPE, GHOST_ALPHA);
            }
            if edge.p_silicon {
                draw_ghost_edge_h(buffer, x_start, y_start, COLOR_P_TYPE, GHOST_ALPHA);
            }
            if edge.metal {
                draw_ghost_edge_h(buffer, x_start, y_start, COLOR_METAL, GHOST_ALPHA * 0.7);
            }
        }
    }

    // Draw ghost for vertical edges
    for (sy, row) in snippet.v_edges.iter().enumerate() {
        for (sx, edge) in row.iter().enumerate() {
            let gx = dest_x + sx;
            let gy = dest_y + sy;
            if gx >= GRID_WIDTH || gy + 1 >= GRID_HEIGHT {
                continue;
            }

            let x_start = gx * CELL_SIZE;
            let y_start = gy * CELL_SIZE;

            if edge.n_silicon {
                draw_ghost_edge_v(buffer, x_start, y_start, COLOR_N_TYPE, GHOST_ALPHA);
            }
            if edge.p_silicon {
                draw_ghost_edge_v(buffer, x_start, y_start, COLOR_P_TYPE, GHOST_ALPHA);
            }
            if edge.metal {
                draw_ghost_edge_v(buffer, x_start, y_start, COLOR_METAL, GHOST_ALPHA * 0.7);
            }
        }
    }
}

/// Draw a ghost fill for a cell
fn draw_ghost_fill(buffer: &mut [u32], x_start: usize, y_start: usize, color: u32, alpha: f32) {
    let margin = CELL_SIZE / 4;
    for y in (y_start + margin)..(y_start + CELL_SIZE - margin) {
        for x in (x_start + margin)..(x_start + CELL_SIZE - margin) {
            if x < WINDOW_WIDTH && y < WINDOW_HEIGHT {
                let idx = y * WINDOW_WIDTH + x;
                buffer[idx] = alpha_blend(color, buffer[idx], alpha);
            }
        }
    }
}

/// Draw ghost indicator for horizontal edge (right side of cell)
fn draw_ghost_edge_h(buffer: &mut [u32], x_start: usize, y_start: usize, color: u32, alpha: f32) {
    let margin = CELL_SIZE / 4;
    let edge_x = x_start + CELL_SIZE - 2;
    for y in (y_start + margin)..(y_start + CELL_SIZE - margin) {
        for x in edge_x..(edge_x + 4) {
            if x < WINDOW_WIDTH && y < WINDOW_HEIGHT {
                let idx = y * WINDOW_WIDTH + x;
                buffer[idx] = alpha_blend(color, buffer[idx], alpha);
            }
        }
    }
}

/// Draw ghost indicator for vertical edge (bottom of cell)
fn draw_ghost_edge_v(buffer: &mut [u32], x_start: usize, y_start: usize, color: u32, alpha: f32) {
    let margin = CELL_SIZE / 4;
    let edge_y = y_start + CELL_SIZE - 2;
    for y in edge_y..(edge_y + 4) {
        for x in (x_start + margin)..(x_start + CELL_SIZE - margin) {
            if x < WINDOW_WIDTH && y < WINDOW_HEIGHT {
                let idx = y * WINDOW_WIDTH + x;
                buffer[idx] = alpha_blend(color, buffer[idx], alpha);
            }
        }
    }
}

/// Get connection flags for a layer at a given cell
fn get_layer_connections(circuit: &Circuit, x: usize, y: usize, layer: Layer) -> [bool; 4] {
    [
        circuit.is_connected(x, y, Direction::Up, layer),
        circuit.is_connected(x, y, Direction::Down, layer),
        circuit.is_connected(x, y, Direction::Left, layer),
        circuit.is_connected(x, y, Direction::Right, layer),
    ]
}

// Tile pixel values
const E: u8 = 0; // Empty
const F: u8 = 1; // Fill
const O: u8 = 2; // Outline

/// Get the tile for a given connection pattern
/// corner_fills: [top_left, top_right, bottom_left, bottom_right] - fill corners for pin cells
fn get_tile(up: bool, down: bool, left: bool, right: bool, corner_fills: [bool; 4]) -> [[u8; CELL_SIZE]; CELL_SIZE] {
    let mut tile = [[E; CELL_SIZE]; CELL_SIZE];

    // Wire occupies most of the cell (leaving small margin, proportional to cell size)
    let margin = CELL_SIZE / 10;  // ~3 at 32px, ~1-2 at 16px
    let margin = if margin < 1 { 1 } else { margin };
    let wire_start = margin;
    let wire_end = CELL_SIZE - margin;
    let corner_radius = CELL_SIZE / 10;
    let corner_radius = if corner_radius < 1 { 1 } else { corner_radius };

    let [fill_tl, fill_tr, fill_bl, fill_br] = corner_fills;

    // Helper to check if a position is in the wire region (before corner rounding)
    let in_center_rect = |x: usize, y: usize| {
        x >= wire_start && x < wire_end && y >= wire_start && y < wire_end
    };
    let in_up_arm = |x: usize, y: usize| up && x >= wire_start && x < wire_end && y < wire_start;
    let in_down_arm = |x: usize, y: usize| down && x >= wire_start && x < wire_end && y >= wire_end;
    let in_left_arm = |x: usize, y: usize| left && y >= wire_start && y < wire_end && x < wire_start;
    let in_right_arm = |x: usize, y: usize| right && y >= wire_start && y < wire_end && x >= wire_end;

    // Corner fill regions (for pin cells where diagonal neighbor is also a pin cell)
    let in_corner_fill = |x: usize, y: usize| {
        (fill_tl && x < wire_start && y < wire_start) ||
        (fill_tr && x >= wire_end && y < wire_start) ||
        (fill_bl && x < wire_start && y >= wire_end) ||
        (fill_br && x >= wire_end && y >= wire_end)
    };

    let in_rect_shape = |x: usize, y: usize| {
        in_center_rect(x, y) || in_up_arm(x, y) || in_down_arm(x, y) || in_left_arm(x, y) || in_right_arm(x, y) || in_corner_fill(x, y)
    };

    // Check if a point should be cut off for corner rounding
    // Only round corners of the center square where there's no connection AND no corner fill
    let in_rounded_corner = |x: usize, y: usize| -> bool {
        // Top-left of center (only if no up and no left connection and not filled)
        if !up && !left && !fill_tl && x < wire_start + corner_radius && y < wire_start + corner_radius {
            let dx = (wire_start + corner_radius - 1) as isize - x as isize;
            let dy = (wire_start + corner_radius - 1) as isize - y as isize;
            if dx + dy >= corner_radius as isize {
                return true;
            }
        }
        // Top-right of center (only if no up and no right connection and not filled)
        if !up && !right && !fill_tr && x >= wire_end - corner_radius && y < wire_start + corner_radius {
            let dx = x as isize - (wire_end - corner_radius) as isize;
            let dy = (wire_start + corner_radius - 1) as isize - y as isize;
            if dx + dy >= corner_radius as isize {
                return true;
            }
        }
        // Bottom-left of center (only if no down and no left connection and not filled)
        if !down && !left && !fill_bl && x < wire_start + corner_radius && y >= wire_end - corner_radius {
            let dx = (wire_start + corner_radius - 1) as isize - x as isize;
            let dy = y as isize - (wire_end - corner_radius) as isize;
            if dx + dy >= corner_radius as isize {
                return true;
            }
        }
        // Bottom-right of center (only if no down and no right connection and not filled)
        if !down && !right && !fill_br && x >= wire_end - corner_radius && y >= wire_end - corner_radius {
            let dx = x as isize - (wire_end - corner_radius) as isize;
            let dy = y as isize - (wire_end - corner_radius) as isize;
            if dx + dy >= corner_radius as isize {
                return true;
            }
        }

        false
    };

    let in_shape = |x: usize, y: usize| {
        in_rect_shape(x, y) && !in_rounded_corner(x, y)
    };

    // Fill the tile
    for y in 0..CELL_SIZE {
        for x in 0..CELL_SIZE {
            if in_shape(x, y) {
                // Check if on edge (neighbor not in shape)
                // But DON'T draw outline on edges that connect to adjacent cells
                let at_top_edge = y == 0;
                let at_bottom_edge = y == CELL_SIZE - 1;
                let at_left_edge = x == 0;
                let at_right_edge = x == CELL_SIZE - 1;

                // Skip outline on connecting edges
                let skip_top = up && at_top_edge;
                let skip_bottom = down && at_bottom_edge;
                let skip_left = left && at_left_edge;
                let skip_right = right && at_right_edge;

                let has_empty_above = y > 0 && !in_shape(x, y - 1);
                let has_empty_below = y < CELL_SIZE - 1 && !in_shape(x, y + 1);
                let has_empty_left = x > 0 && !in_shape(x - 1, y);
                let has_empty_right = x < CELL_SIZE - 1 && !in_shape(x + 1, y);

                let on_edge = (!skip_top && (at_top_edge || has_empty_above))
                    || (!skip_bottom && (at_bottom_edge || has_empty_below))
                    || (!skip_left && (at_left_edge || has_empty_left))
                    || (!skip_right && (at_right_edge || has_empty_right));

                tile[y][x] = if on_edge { O } else { F };
            }
        }
    }

    tile
}

fn draw_layer(
    buffer: &mut [u32],
    cell_x: usize,
    cell_y: usize,
    conns: &[bool; 4], // [up, down, left, right]
    color: u32,
    corner_fills: [bool; 4], // [top_left, top_right, bottom_left, bottom_right]
) {
    let [conn_up, conn_down, conn_left, conn_right] = conns;
    let tile = get_tile(*conn_up, *conn_down, *conn_left, *conn_right, corner_fills);

    // Determine start positions - extend into grid line area when connected
    let start_x = if *conn_left { 0 } else { 1 };
    let start_y = if *conn_up { 0 } else { 1 };

    for ty in start_y..CELL_SIZE {
        for tx in start_x..CELL_SIZE {
            let pixel_type = tile[ty][tx];
            if pixel_type == E {
                continue;
            }

            let px = cell_x + tx;
            let py = cell_y + ty;

            if px < WINDOW_WIDTH && py < WINDOW_HEIGHT {
                let draw_color = if pixel_type == O { COLOR_OUTLINE } else { color };
                buffer[py * WINDOW_WIDTH + px] = draw_color;
            }
        }
    }
}

/// Get a narrower tile for gate rendering (the interrupting part of a transistor)
fn get_gate_tile(up: bool, down: bool, left: bool, right: bool) -> [[u8; CELL_SIZE]; CELL_SIZE] {
    let mut tile = [[E; CELL_SIZE]; CELL_SIZE];

    // Gate is narrower than normal wire (proportional to cell size)
    let margin = CELL_SIZE * 22 / 100;  // ~7 at 32px, ~3-4 at 16px
    let margin = if margin < 2 { 2 } else { margin };
    let wire_start = margin;
    let wire_end = CELL_SIZE - margin;
    let corner_radius = CELL_SIZE / 16;
    let corner_radius = if corner_radius < 1 { 1 } else { corner_radius };

    let in_center_rect = |x: usize, y: usize| {
        x >= wire_start && x < wire_end && y >= wire_start && y < wire_end
    };
    let in_up_arm = |x: usize, y: usize| up && x >= wire_start && x < wire_end && y < wire_start;
    let in_down_arm = |x: usize, y: usize| down && x >= wire_start && x < wire_end && y >= wire_end;
    let in_left_arm = |x: usize, y: usize| left && y >= wire_start && y < wire_end && x < wire_start;
    let in_right_arm = |x: usize, y: usize| right && y >= wire_start && y < wire_end && x >= wire_end;

    let in_rect_shape = |x: usize, y: usize| {
        in_center_rect(x, y) || in_up_arm(x, y) || in_down_arm(x, y) || in_left_arm(x, y) || in_right_arm(x, y)
    };

    // Corner rounding for gate
    let in_rounded_corner = |x: usize, y: usize| -> bool {
        if !up && !left && x < wire_start + corner_radius && y < wire_start + corner_radius {
            let dx = (wire_start + corner_radius - 1) as isize - x as isize;
            let dy = (wire_start + corner_radius - 1) as isize - y as isize;
            if dx + dy >= corner_radius as isize { return true; }
        }
        if !up && !right && x >= wire_end - corner_radius && y < wire_start + corner_radius {
            let dx = x as isize - (wire_end - corner_radius) as isize;
            let dy = (wire_start + corner_radius - 1) as isize - y as isize;
            if dx + dy >= corner_radius as isize { return true; }
        }
        if !down && !left && x < wire_start + corner_radius && y >= wire_end - corner_radius {
            let dx = (wire_start + corner_radius - 1) as isize - x as isize;
            let dy = y as isize - (wire_end - corner_radius) as isize;
            if dx + dy >= corner_radius as isize { return true; }
        }
        if !down && !right && x >= wire_end - corner_radius && y >= wire_end - corner_radius {
            let dx = x as isize - (wire_end - corner_radius) as isize;
            let dy = y as isize - (wire_end - corner_radius) as isize;
            if dx + dy >= corner_radius as isize { return true; }
        }
        false
    };

    let in_shape = |x: usize, y: usize| {
        in_rect_shape(x, y) && !in_rounded_corner(x, y)
    };

    for y in 0..CELL_SIZE {
        for x in 0..CELL_SIZE {
            if in_shape(x, y) {
                let at_top_edge = y == 0;
                let at_bottom_edge = y == CELL_SIZE - 1;
                let at_left_edge = x == 0;
                let at_right_edge = x == CELL_SIZE - 1;

                let skip_top = up && at_top_edge;
                let skip_bottom = down && at_bottom_edge;
                let skip_left = left && at_left_edge;
                let skip_right = right && at_right_edge;

                let has_empty_above = y > 0 && !in_shape(x, y - 1);
                let has_empty_below = y < CELL_SIZE - 1 && !in_shape(x, y + 1);
                let has_empty_left = x > 0 && !in_shape(x - 1, y);
                let has_empty_right = x < CELL_SIZE - 1 && !in_shape(x + 1, y);

                let on_edge = (!skip_top && (at_top_edge || has_empty_above))
                    || (!skip_bottom && (at_bottom_edge || has_empty_below))
                    || (!skip_left && (at_left_edge || has_empty_left))
                    || (!skip_right && (at_right_edge || has_empty_right));

                tile[y][x] = if on_edge { O } else { F };
            }
        }
    }

    tile
}

fn draw_gate_layer(
    buffer: &mut [u32],
    cell_x: usize,
    cell_y: usize,
    conns: &[bool; 4],
    color: u32,
) {
    let [conn_up, conn_down, conn_left, conn_right] = conns;
    let tile = get_gate_tile(*conn_up, *conn_down, *conn_left, *conn_right);

    let start_x = if *conn_left { 0 } else { 1 };
    let start_y = if *conn_up { 0 } else { 1 };

    for ty in start_y..CELL_SIZE {
        for tx in start_x..CELL_SIZE {
            let pixel_type = tile[ty][tx];
            if pixel_type == E {
                continue;
            }

            let px = cell_x + tx;
            let py = cell_y + ty;

            if px < WINDOW_WIDTH && py < WINDOW_HEIGHT {
                let draw_color = if pixel_type == O { COLOR_OUTLINE } else { color };
                buffer[py * WINDOW_WIDTH + px] = draw_color;
            }
        }
    }
}

/// Alpha blend two colors: result = alpha * fg + (1-alpha) * bg
fn alpha_blend(fg: u32, bg: u32, alpha: f32) -> u32 {
    let fg_r = ((fg >> 16) & 0xFF) as f32;
    let fg_g = ((fg >> 8) & 0xFF) as f32;
    let fg_b = (fg & 0xFF) as f32;

    let bg_r = ((bg >> 16) & 0xFF) as f32;
    let bg_g = ((bg >> 8) & 0xFF) as f32;
    let bg_b = (bg & 0xFF) as f32;

    let r = (alpha * fg_r + (1.0 - alpha) * bg_r) as u32;
    let g = (alpha * fg_g + (1.0 - alpha) * bg_g) as u32;
    let b = (alpha * fg_b + (1.0 - alpha) * bg_b) as u32;

    (r << 16) | (g << 8) | b
}

/// Draw a via as a hollow circle (ring) in the center of the cell
fn draw_via(buffer: &mut [u32], cell_x: usize, cell_y: usize) {
    let center_x = cell_x + CELL_SIZE / 2;
    let center_y = cell_y + CELL_SIZE / 2;
    // Proportional to cell size (~5 and ~7 at 32px)
    let inner_radius = (CELL_SIZE * 5 / 32) as isize;
    let inner_radius = if inner_radius < 2 { 2 } else { inner_radius };
    let outer_radius = (CELL_SIZE * 7 / 32) as isize;
    let outer_radius = if outer_radius < 3 { 3 } else { outer_radius };

    for dy in -(outer_radius as isize)..=(outer_radius as isize) {
        for dx in -(outer_radius as isize)..=(outer_radius as isize) {
            let dist_sq = dx * dx + dy * dy;
            let px = (center_x as isize + dx) as usize;
            let py = (center_y as isize + dy) as usize;

            if px < WINDOW_WIDTH && py < WINDOW_HEIGHT {
                // Only draw the ring (between inner and outer radius)
                if dist_sq >= inner_radius * inner_radius && dist_sq <= outer_radius * outer_radius {
                    buffer[py * WINDOW_WIDTH + px] = COLOR_VIA;
                }
            }
        }
    }
}

/// Draw a layer with alpha blending (for metal transparency)
fn draw_layer_alpha(
    buffer: &mut [u32],
    cell_x: usize,
    cell_y: usize,
    conns: &[bool; 4],
    color: u32,
    alpha: f32,
    corner_fills: [bool; 4], // [top_left, top_right, bottom_left, bottom_right]
) {
    let [conn_up, conn_down, conn_left, conn_right] = conns;
    let tile = get_tile(*conn_up, *conn_down, *conn_left, *conn_right, corner_fills);

    let start_x = if *conn_left { 0 } else { 1 };
    let start_y = if *conn_up { 0 } else { 1 };

    for ty in start_y..CELL_SIZE {
        for tx in start_x..CELL_SIZE {
            let pixel_type = tile[ty][tx];
            if pixel_type == E {
                continue;
            }

            let px = cell_x + tx;
            let py = cell_y + ty;

            if px < WINDOW_WIDTH && py < WINDOW_HEIGHT {
                let fg_color = if pixel_type == O { COLOR_OUTLINE } else { color };
                let bg_color = buffer[py * WINDOW_WIDTH + px];
                // Use full alpha for outline, partial for fill
                let effective_alpha = if pixel_type == O { 1.0 } else { alpha };
                buffer[py * WINDOW_WIDTH + px] = alpha_blend(fg_color, bg_color, effective_alpha);
            }
        }
    }
}
