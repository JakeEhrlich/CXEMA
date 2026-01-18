# CXEMA - Claude Context

## Project Overview
CXEMA is a faithful fan-made clone of KOHCTPYKTOP: Engineer of the People by Zachtronics - a circuit design puzzle game where you design integrated circuits using N-type and P-type silicon, metal layers, and vias.

## Tech Stack
- **Rust** with minifb for windowing/graphics
- **rodio** for audio (just added to Cargo.toml)
- **serde/serde_json** for level file parsing
- **bdf-parser** for Terminus font rendering

## Project Structure
- `src/main.rs` - Main game code (~3500 lines)
- `levels/*.json` - Level definitions with pin names and waveforms
- `music/` - WAV files for background music
- `terminus/` - BDF font files
- `.snippits/` - User-saved circuit snippets
- `.designs/` - User-saved full circuit designs

## Key Concepts
- **Grid**: 44x27 cells, with pins on left (x=1) and right (x=40) edges
- **Silicon**: N-type (red) and P-type (yellow), forms transistor gates when they cross
- **Metal**: Conductive layer on top, connected to silicon via "vias"
- **Simulation**: BFS-based signal propagation with gate logic (N-channel opens when HIGH, P-channel opens when LOW)

## Recent Work
- Implemented circuit simulation with proper signal propagation (metal <-> via <-> silicon)
- Added verification tab with waveform display (expected in gray, actual in green)
- Levels system with JSON format, `display` field to hide vcc waveforms
- Fixed propagation bug: silicon now propagates back UP through vias to metal

## Tabs
- Specs, Verify, Snippets, Designs, Help, Menu
- Menu tab needs music selection UI (current task)

## Music Files (in music/)
- `analog_sequence.wav` - Xinematix (CC BY 4.0)
- `groovy_beat.wav` - Seth_Makes_Sounds (CC0)
- `retro_loop.wav` - ProdByRey (CC0)
