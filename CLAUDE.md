# CXEMA - Claude Context

## Project Overview
CXEMA is a fan-made clone of KOHCTPYKTOP: Engineer of the People by Zachtronics - a circuit design puzzle game where you design integrated circuits using N-type and P-type silicon, metal layers, and vias. Features "pig-russian" text (English transliterated to Cyrillic).

## Tech Stack
- **Rust** with minifb for windowing/graphics
- **rodio** for procedural audio synthesis
- **serde/serde_json** for level file parsing
- **bdf-parser** for Terminus font rendering

## Project Structure
- `src/main.rs` - Main game code (~6000 lines)
- `levels/*.json` - Level definitions with pin names, waveforms, and pig-russian datasheets
- `terminus/` - BDF font files
- `.snippits/` - User-saved circuit snippets
- `.designs/` - User-saved full circuit designs

## Key Concepts
- **Grid**: 44x27 cells, with pins on left (x=1) and right (x=40) edges
- **Silicon**: N-type (red) and P-type (yellow), forms transistor gates when they cross
- **Metal**: Conductive layer on top, connected to silicon via "vias"
- **Simulation**: BFS-based signal propagation with gate logic (N-channel opens when HIGH, P-channel opens when LOW)

## Audio System
- Procedural synth with configurable waveform, scale, filter, envelope
- 8-bit style UI sound effects (clicks, mode switches, placement sounds)
- Verification success/failure sounds

## Pig-Russian Transliteration
All UI text uses "pig-russian" - English words transliterated to Cyrillic:
- Single chars: a→а, b→б, c→к, d→д, e→е, f→ф, g→г, h→х, i→и, j→ж, k→к, l→л, m→м, n→н, o→о, p→п, q→к, r→р, s→с, t→т, u→у, v→в, w→щ, x→кс, y→ы, z→з
- Word-start bigrams: th→ѳ, sh→щ, ch→х, sch→щ
- Word-end trigram: ing→инг

## Tabs
- СПЕКС (Specs) - Level datasheet
- ТЕСТ (Test/Verify) - Waveform verification
- СНИПС (Snippets) - Saved circuit fragments
- ДИЗНС (Designs) - Full circuit saves
- ХЕЛП (Help) - Controls and transistor behavior
- СИНѲ (Synth) - Music/audio controls
