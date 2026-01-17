# 🐄 Pxtone Cowlage 🐄

A [PxTone](<https://en.wikipedia.org/wiki/PxTone>) player and editor made with Rust and egui.

Includes among other things:
- Playback interface with waveform visualization
![screenshot][screenshot1]
- A piano roll
![screenshot][screenshot2]
- Built in voice viewer/editor
- You can play units on your (qwerty) keyboard (maybe one day MIDI keyboard support)
- MIDI (`.mid`) import
- PiyoPiyo (`.pmd`) import

[screenshot1]: https://private-user-images.githubusercontent.com/1521976/537214664-d3cf3d8e-e8ca-44a3-9a07-7bf201369ab8.png?jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJpc3MiOiJnaXRodWIuY29tIiwiYXVkIjoicmF3LmdpdGh1YnVzZXJjb250ZW50LmNvbSIsImtleSI6ImtleTUiLCJleHAiOjE3Njg2ODU1NjEsIm5iZiI6MTc2ODY4NTI2MSwicGF0aCI6Ii8xNTIxOTc2LzUzNzIxNDY2NC1kM2NmM2Q4ZS1lOGNhLTQ0YTMtOWEwNy03YmYyMDEzNjlhYjgucG5nP1gtQW16LUFsZ29yaXRobT1BV1M0LUhNQUMtU0hBMjU2JlgtQW16LUNyZWRlbnRpYWw9QUtJQVZDT0RZTFNBNTNQUUs0WkElMkYyMDI2MDExNyUyRnVzLWVhc3QtMSUyRnMzJTJGYXdzNF9yZXF1ZXN0JlgtQW16LURhdGU9MjAyNjAxMTdUMjEyNzQxWiZYLUFtei1FeHBpcmVzPTMwMCZYLUFtei1TaWduYXR1cmU9MmVkZmU4ODFiNjdjNzhkNTczOWRiM2U2N2VmMzgwMWExNDI0MDIyNjkwZTNmYzExZWRkMGQ1NTM0NmFhNzA3MiZYLUFtei1TaWduZWRIZWFkZXJzPWhvc3QifQ.lWbyuONlD3BdZt0f5otbisBeUCjpolMG0-qQfEymuco

[screenshot2]: https://private-user-images.githubusercontent.com/1521976/537215313-6dc7af82-cd78-4cb8-b948-ccd3d6388302.png?jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJpc3MiOiJnaXRodWIuY29tIiwiYXVkIjoicmF3LmdpdGh1YnVzZXJjb250ZW50LmNvbSIsImtleSI6ImtleTUiLCJleHAiOjE3Njg2ODU1NjEsIm5iZiI6MTc2ODY4NTI2MSwicGF0aCI6Ii8xNTIxOTc2LzUzNzIxNTMxMy02ZGM3YWY4Mi1jZDc4LTRjYjgtYjk0OC1jY2QzZDYzODgzMDIucG5nP1gtQW16LUFsZ29yaXRobT1BV1M0LUhNQUMtU0hBMjU2JlgtQW16LUNyZWRlbnRpYWw9QUtJQVZDT0RZTFNBNTNQUUs0WkElMkYyMDI2MDExNyUyRnVzLWVhc3QtMSUyRnMzJTJGYXdzNF9yZXF1ZXN0JlgtQW16LURhdGU9MjAyNjAxMTdUMjEyNzQxWiZYLUFtei1FeHBpcmVzPTMwMCZYLUFtei1TaWduYXR1cmU9OTkyNzNhNTFhY2Q0YzMyYzA0NGZhZDk5YjZlNDkwYjkzOGEwZmM1YzM4ZTMwN2QxNzFiZDQzODg3OTdhODgyMiZYLUFtei1TaWduZWRIZWFkZXJzPWhvc3QifQ.fbcmO7TB-cGy-bQV6euoYX6co4bEVXdlkO5Rq8Q3JVo

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
