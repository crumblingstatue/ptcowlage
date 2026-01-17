# 🐄 Pxtone Cowlage 🐄

A [PxTone](<https://en.wikipedia.org/wiki/PxTone>) player and editor made with Rust and egui.

Includes among other things:
- Playback interface with waveform visualization
- A piano roll
- Built in voice viewer/editor
- You can play units on your (qwerty) keyboard (maybe one day MIDI keyboard support)

Powered by the [ptcow](<https://github.com/crumblingstatue/ptcow/>) PxTone playback library.

## Building

You need [Rust](<https://rust-lang.org/>) 1.92 or later to build the project.
You can use [rustup](<https://rustup.rs/>) to install it if you don't have it.

You can then do `cargo build --release`, and find the `ptcowlage` executable in `target/release`.

You can also try

```
cargo install --locked --git https://github.com/crumblingstatue/ptcowlage.git
```

Which will put `ptcowlage` into your `$HOME/.cargo/bin`.
