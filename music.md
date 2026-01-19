# Audio System Guide

A brief overview of the procedural audio techniques used for the ambient music, CRT hum, and UI sound effects.

---

## Ambient Synth (80s Style Pads)

The ambient music uses classic analog synth techniques to create warm, evolving pads.

### Signal Chain

```
OSC 1 (saw) ─┬─► FILTER (lowpass) ─► ENVELOPE ─► REVERB ─► OUT
OSC 2 (saw) ─┘         ▲
                       │
                 LFO (sine) 
```

### Key Components

| Component | Purpose |
|-----------|---------|
| **Two detuned oscillators** | Slight pitch difference (~8 cents) creates thickness and warmth, mimicking analog drift |
| **Low-pass filter** | Cuts harsh high frequencies for a mellow tone. Cutoff around 800-1200 Hz |
| **LFO → Filter** | Slow sine wave (0.1-0.5 Hz) modulates filter cutoff, creating gentle "breathing" movement |
| **Slow envelope** | Attack of 1-3 seconds, release of 2-4 seconds for pad-like swells |
| **Reverb** | Adds space and atmosphere (wet/dry mix around 40/60) |

### Musical Approach

- Simple chord progressions: Cmaj7 → Am7 → Fmaj7 → G
- Extended chords (7ths) for that dreamy quality
- Slow tempo, chords change every 4-8 seconds
- Sawtooth waves are classic 80s; triangle for softer tones

---

## CRT Monitor Hum

The CRT hum layers several components to recreate that old monitor ambiance.

### Components

| Layer | Frequency | Character |
|-------|-----------|-----------|
| **Mains hum** | 60 Hz (US) / 50 Hz (EU) | The fundamental low drone |
| **Harmonics** | 120, 180, 240 Hz | Adds buzzy texture (decreasing volume per harmonic) |
| **Scan whine** | ~12-15.7 kHz | High-pitched squeal from horizontal scan |
| **Static** | Filtered noise | Electrical crackle/interference |

### Signal Chain

```
MAINS (60Hz sine) ────────────────┬─► MASTER ─► OUT
HARMONICS (120, 180, 240Hz) ──────┤
SCAN WHINE (12kHz + LFO drift) ───┤
STATIC (filtered noise) ──────────┘
```

### Special Effects

- **Power on**: Low "thunk" (40-50 Hz, fast decay) followed by hum fading in
- **Power off**: Descending pitch sweep (200→30 Hz)
- **Degauss**: 60 Hz base with fast wobble LFO (12→2 Hz) that slows down over ~2 seconds

---

## UI Sound Effects (8-bit Style)

The UI sounds use lo-fi techniques to achieve that crunchy Amiga/Atari aesthetic.

### Techniques

| Technique | How It Works |
|-----------|--------------|
| **Bit-crushing** | Quantizes waveform to ~4 bits using a waveshaper, adds digital grit |
| **Noise transients** | Short noise burst at sound start mimics cheap DAC "click" |
| **Rapid arpeggios** | Fast frequency switching (classic chip limitation workaround) |
| **Detuned oscillators** | Two slightly off-pitch squares for thick, wobbly sound |
| **Pitch sweeps** | Frequency slides up or down for expressive "bwoop" character |

### Sound Design Summary

| Sound | Approach |
|-------|----------|
| **Button click** | 900→600 Hz sweep + noise transient, ~40ms |
| **Tab switch** | Two-note arpeggio (440→660 Hz), detuned pair |
| **Mode 1-9** | Unique arpeggio pattern per mode, ascending intervals |
| **Invalid/Error** | Low buzz (120 Hz) with fast wobble LFO, ~200ms |
| **Place** | High soft pip (2400→1800 Hz triangle) + tiny noise, ~25ms |
| **Delete** | Descending sweep (300→50 Hz), ~120ms |
| **Blocked** | Two detuned low squares (110/117 Hz), ~80ms |

### Design Principles

- **Square waves only** for authentic chip sound
- **Short durations** (20-100ms) for snappy feedback
- **Pitch indicates meaning**: ascending = positive, descending = negative/removal
- **Layer noise + tone** for satisfying transients
- **Bit-crush everything** for consistent lo-fi character

---

## FunDSP Translation

These Web Audio techniques map directly to FunDSP in Rust:

| Web Audio | FunDSP |
|-----------|--------|
| `OscillatorNode` | `saw()`, `sine()`, `square()` |
| `BiquadFilter` | `lowpass_hz()`, `highpass_hz()` |
| `GainNode` | `* 0.5` (multiply operator) |
| LFO modulation | Pipe sine to parameter |
| Noise | `noise()` |
| Reverb | `reverb_stereo()` |
| ADSR envelope | `adsr_live()` |

### Example: Button Click in FunDSP

```rust
fn button_click() -> impl AudioUnit {
    let tone = square_hz(900.0); // Would need pitch envelope
    let noise = noise() >> highpass_hz(1000.0, 0.5) * 0.15;
    
    (tone + noise) >> lowpass_hz(2000.0, 1.0) * 0.3
    // Apply short envelope in game logic
}
```

---

## Tips for Implementation

1. **Keep sounds short** — UI feedback should be < 100ms
2. **Vary pitch for context** — different modes/states get different frequencies
3. **Layer for richness** — noise + tone + sub often sounds better than pure tones
4. **Use envelopes** — even 5ms attack smooths clicks, longer release avoids abrupt cutoff
5. **Bit-crush consistently** — apply to all UI sounds for cohesive aesthetic
6. **Test at low volume** — sounds should work when quiet, not rely on being loud
