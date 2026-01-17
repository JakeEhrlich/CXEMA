use minifb::{Key, Window, WindowOptions};
use std::collections::HashMap;
use std::time::{Duration, Instant};

// Grid dimensions (matches original game)
// 44 wide: 1 blank + 3 pin + 36 playable + 3 pin + 1 blank
// 27 high: 2 blank + (6 pins × 4 stride) + 2 blank
const GRID_WIDTH: usize = 44;
const GRID_HEIGHT: usize = 27;
const CELL_SIZE: usize = 16;

// Pin layout: 3x3 pins, left at x=1, right at x=40
const PIN_SIZE: usize = 3;

// Window size is just the grid
const WINDOW_WIDTH: usize = GRID_WIDTH * CELL_SIZE;
const WINDOW_HEIGHT: usize = GRID_HEIGHT * CELL_SIZE;

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
}

/// Visual mode sub-states
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VisualState {
    Normal,        // Just cursor, no selection
    Selecting,     // 'v' pressed, selecting area
    PlacingN,      // '+' then 'n' or just in N mode - placing N silicon
    PlacingP,      // '+' pressed - placing P silicon
    PlacingMetal,  // '=' pressed - placing metal
}

/// Editor state for construction
struct EditorState {
    mode: EditMode,
    visual_state: VisualState,

    // Cursor position (grid coordinates)
    cursor_x: usize,
    cursor_y: usize,

    // Selection anchor (for visual mode)
    selection_anchor: Option<(usize, usize)>,

    // Path construction state (for mouse-based modes)
    path_start: Option<(usize, usize)>,
    current_path: Vec<(usize, usize)>,

    // Mouse position (grid coordinates)
    mouse_grid_x: Option<usize>,
    mouse_grid_y: Option<usize>,
}

impl EditorState {
    fn new() -> Self {
        Self {
            mode: EditMode::Visual,  // Start in visual mode
            visual_state: VisualState::Normal,
            cursor_x: GRID_WIDTH / 2,
            cursor_y: GRID_HEIGHT / 2,
            selection_anchor: None,
            path_start: None,
            current_path: Vec::new(),
            mouse_grid_x: None,
            mouse_grid_y: None,
        }
    }

    fn get_selection(&self) -> Option<(usize, usize, usize, usize)> {
        if let Some((ax, ay)) = self.selection_anchor {
            let min_x = ax.min(self.cursor_x);
            let max_x = ax.max(self.cursor_x);
            let min_y = ay.min(self.cursor_y);
            let max_y = ay.max(self.cursor_y);
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
                if !blocked || (nx, ny) == end {
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
        if !is_playable(x, y) {
            continue;
        }

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

        // Connect to previous node in path
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
        if !is_playable(x, y) {
            continue;
        }

        // Place node
        if let Some(node) = circuit.get_node_mut(x, y) {
            node.metal = true;
        }

        // Connect to previous node in path
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

// ============================================================================
// Main
// ============================================================================

fn main() {
    let mut buffer: Vec<u32> = vec![0; WINDOW_WIDTH * WINDOW_HEIGHT];
    let mut circuit = Circuit::new();
    let mut editor = EditorState::new();

    // Load font for pin labels
    let font = load_font("terminus/ter-u14b.bdf");

    // Create pins and place their metal
    let pins = create_pins();
    setup_pins(&mut circuit, &pins);

    // Add some test patterns to visualize
    setup_test_pattern(&mut circuit);

    let mut window = Window::new(
        "KOHCTPYKTOP: Engineer of the People",
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
        handle_input(&mut circuit, &mut editor, &new_keys, &window);

        // Update mouse position
        if let Some((mx, my)) = window.get_mouse_pos(minifb::MouseMode::Clamp) {
            let grid_x = (mx as usize) / CELL_SIZE;
            let grid_y = (my as usize) / CELL_SIZE;
            if grid_x < GRID_WIDTH && grid_y < GRID_HEIGHT {
                editor.mouse_grid_x = Some(grid_x);
                editor.mouse_grid_y = Some(grid_y);

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
            }
        }

        // Get current mouse button states
        let left_down = window.get_mouse_down(minifb::MouseButton::Left);
        let right_down = window.get_mouse_down(minifb::MouseButton::Right);

        // Detect mouse button press (transition from not pressed to pressed)
        let left_clicked = left_down && !prev_left_down;
        let right_clicked = right_down && !prev_right_down;

        // Handle mouse clicks with debouncing
        handle_mouse(&mut circuit, &mut editor, left_clicked, right_clicked, left_down, right_down);

        // Render
        render(&circuit, &editor, &pins, &font, &mut buffer);

        window
            .update_with_buffer(&buffer, WINDOW_WIDTH, WINDOW_HEIGHT)
            .unwrap();

        prev_keys = current_keys;
        prev_left_down = left_down;
        prev_right_down = right_down;
    }
}

/// Handle keyboard input
fn handle_input(circuit: &mut Circuit, editor: &mut EditorState, new_keys: &[Key], window: &Window) {
    // Check if shift is held
    let shift_held = window.is_key_down(Key::LeftShift) || window.is_key_down(Key::RightShift);

    for key in new_keys {
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

            // Visual mode keys (only active in Visual mode)
            _ if editor.mode == EditMode::Visual => {
                handle_visual_mode_key(circuit, editor, *key, shift_held);
            }

            _ => {}
        }
    }
}

/// Handle visual mode keyboard input
fn handle_visual_mode_key(circuit: &mut Circuit, editor: &mut EditorState, key: Key, shift_held: bool) {
    let prev_x = editor.cursor_x;
    let prev_y = editor.cursor_y;

    match key {
        // Movement: hjkl or arrow keys
        Key::H | Key::Left => {
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

        // 'd' - delete selection or cursor position
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

        // Escape - exit any sub-mode, return to normal visual mode
        Key::Escape => {
            editor.visual_state = VisualState::Normal;
            editor.selection_anchor = None;
        }

        _ => {}
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
            // In visual mode, mouse clicks can move cursor
            if left_clicked {
                editor.cursor_x = grid_x;
                editor.cursor_y = grid_y;
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

fn render(circuit: &Circuit, editor: &EditorState, pins: &[Pin], font: &HashMap<char, Vec<Vec<bool>>>, buffer: &mut [u32]) {
    // Fill background
    for pixel in buffer.iter_mut() {
        *pixel = COLOR_BACKGROUND;
    }

    // Draw all cells (pins are just cells with metal)
    for grid_y in 0..GRID_HEIGHT {
        for grid_x in 0..GRID_WIDTH {
            render_cell(circuit, grid_x, grid_y, buffer);
        }
    }

    // Draw pin labels centered in their 3x3 area
    for pin in pins {
        // Center of 3x3 pin area
        let center_x = pin.x * CELL_SIZE + (PIN_SIZE * CELL_SIZE) / 2;
        let center_y = pin.y * CELL_SIZE + (PIN_SIZE * CELL_SIZE) / 2;
        draw_text(&pin.label, center_x, center_y, font, COLOR_PIN_TEXT, buffer);
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

    // Draw cursor (for visual mode)
    if editor.mode == EditMode::Visual {
        draw_cursor(editor.cursor_x, editor.cursor_y, buffer);
    }

    // Draw mode indicator at top-left
    let mode_text = match editor.mode {
        EditMode::NSilicon => "1:N-Si",
        EditMode::PSilicon => "2:P-Si",
        EditMode::Metal => "3:Metal",
        EditMode::Via => "4:Via",
        EditMode::DeleteMetal => "5:DelM",
        EditMode::DeleteSilicon => "6:DelS",
        EditMode::DeleteAll => "7:DelA",
        EditMode::Visual => match editor.visual_state {
            VisualState::Normal => "8:Vis",
            VisualState::Selecting => "8:V-Sel",
            VisualState::PlacingN => "8:V-N",
            VisualState::PlacingP => "8:V-P",
            VisualState::PlacingMetal => "8:V-M",
        },
    };
    draw_text(mode_text, 40, 10, font, 0xffffff, buffer);
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

fn render_cell(circuit: &Circuit, grid_x: usize, grid_y: usize, buffer: &mut [u32]) {
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

    // Determine what to draw based on node's silicon type
    match node.silicon {
        Silicon::None => {}
        Silicon::N => {
            let conns = get_layer_connections(circuit, grid_x, grid_y, Layer::NSilicon);
            draw_layer(buffer, x_start, y_start, &conns, COLOR_N_TYPE);
        }
        Silicon::P => {
            let conns = get_layer_connections(circuit, grid_x, grid_y, Layer::PSilicon);
            draw_layer(buffer, x_start, y_start, &conns, COLOR_P_TYPE);
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
            draw_layer(buffer, x_start, y_start, &channel_conns, channel_color);
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
        draw_layer_alpha(buffer, x_start, y_start, &conns, COLOR_METAL, METAL_ALPHA);
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
fn get_tile(up: bool, down: bool, left: bool, right: bool) -> [[u8; CELL_SIZE]; CELL_SIZE] {
    let mut tile = [[E; CELL_SIZE]; CELL_SIZE];

    // Wire occupies most of the cell (leaving small margin, proportional to cell size)
    let margin = CELL_SIZE / 10;  // ~3 at 32px, ~1-2 at 16px
    let margin = if margin < 1 { 1 } else { margin };
    let wire_start = margin;
    let wire_end = CELL_SIZE - margin;
    let corner_radius = CELL_SIZE / 10;
    let corner_radius = if corner_radius < 1 { 1 } else { corner_radius };

    // Helper to check if a position is in the wire region (before corner rounding)
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

    // Check if a point should be cut off for corner rounding
    // Only round corners of the center square where there's no connection
    let in_rounded_corner = |x: usize, y: usize| -> bool {
        // Top-left of center (only if no up and no left connection)
        if !up && !left && x < wire_start + corner_radius && y < wire_start + corner_radius {
            let dx = (wire_start + corner_radius - 1) as isize - x as isize;
            let dy = (wire_start + corner_radius - 1) as isize - y as isize;
            if dx + dy >= corner_radius as isize {
                return true;
            }
        }
        // Top-right of center (only if no up and no right connection)
        if !up && !right && x >= wire_end - corner_radius && y < wire_start + corner_radius {
            let dx = x as isize - (wire_end - corner_radius) as isize;
            let dy = (wire_start + corner_radius - 1) as isize - y as isize;
            if dx + dy >= corner_radius as isize {
                return true;
            }
        }
        // Bottom-left of center (only if no down and no left connection)
        if !down && !left && x < wire_start + corner_radius && y >= wire_end - corner_radius {
            let dx = (wire_start + corner_radius - 1) as isize - x as isize;
            let dy = y as isize - (wire_end - corner_radius) as isize;
            if dx + dy >= corner_radius as isize {
                return true;
            }
        }
        // Bottom-right of center (only if no down and no right connection)
        if !down && !right && x >= wire_end - corner_radius && y >= wire_end - corner_radius {
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
) {
    let [conn_up, conn_down, conn_left, conn_right] = conns;
    let tile = get_tile(*conn_up, *conn_down, *conn_left, *conn_right);

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
) {
    let [conn_up, conn_down, conn_left, conn_right] = conns;
    let tile = get_tile(*conn_up, *conn_down, *conn_left, *conn_right);

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
