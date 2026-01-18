# Current Task: Add Music Playback

## What Needs to Be Done
1. Add rodio imports to main.rs
2. Set up audio output stream and sink
3. Create enum for music tracks (NoMusic, AnalogSequence, GroovyBeat, RetroLoop)
4. Add music state to track current selection
5. Implement music playback with looping
6. Add Menu tab UI with buttons to select between tracks and "No Music"

## Music Files
Located in `music/` directory:
- `analog_sequence.wav` - Analog Sequence Melody by Xinematix
- `groovy_beat.wav` - Groovy Beat by Seth_Makes_Sounds
- `retro_loop.wav` - Retro Game Music Loop by ProdByRey

## Implementation Notes
- Use rodio crate (already added to Cargo.toml)
- Music should loop continuously
- Menu tab should show 4 buttons: "No Music", and one for each track
- Highlight currently selected track
- Switching tracks should stop current and start new one

## Code Locations to Modify
- Top of main.rs: add `use rodio::{...}` imports
- Add MusicTrack enum near other enums
- In main(): set up OutputStream and Sink
- render_bottom_area / Tab::Menu match arm: add music selection UI
- Handle mouse clicks on music buttons

## Future Enhancement (noted by user)
User wants to eventually add generative/synth music as an easter egg - rodio supports custom Source trait for this.
