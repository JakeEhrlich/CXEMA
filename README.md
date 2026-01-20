# CXEMA

An "improved" fan-made clone of [KOHCTPYKTOP: Engineer of the People](https://www.zachtronics.com/kohctpyktop-engineer-of-the-people/) by Zachtronics.

Design integrated circuits using silicon and metal layers to match the specifications on each chip's datasheet.

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

### Data Storage

- `.snippits/` - Saved circuit snippets
- `.designs/` - Saved full circuit designs

## Credits

Original game by [Zachtronics](https://www.zachtronics.com/).
