# grains

grains is a GPU falling-sand sandbox. The same frame runs in a native window and in the browser on WebGPU. On launch, a pile of sand falls onto stone ledges.

## Run the native window

Install the pinned Rust toolchain, then start the window.

```
mise install
cargo run
```

The window title is grains. The inner size is 1024 by 600 physical pixels. Each frame runs several Margolus passes. Sand sinks through water and oil, oil floats on water, and a falling liquid stays in a column until it lands. Smoke rises on two passes out of four, drifts sideways, and fades. Wood next to fire burns. Fire rises a little and leaves smoke. Water puts fire out and becomes steam, and steam rises until it condenses.

## Controls

Left click paints. Right click erases. The scroll wheel changes the radius. The terminal prints the material and the radius when either changes.

Press `1` for empty, `2` for stone, `3` for sand, `4` for water, `5` for oil, `6` for smoke, `7` for wood, `8` for fire, or `9` for steam. Press `+` or `-` to change how many simulation passes run each frame. The terminal prints that number. Press `C` to clear the grid. Press Space to pause the fall. The brush still paints while the fall is paused.

## Build the web page

Install the `wasm32-unknown-unknown` target and wasm-bindgen 0.2.130. Then build the library and the page.

```
rustup target add wasm32-unknown-unknown
cargo build --release --lib --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir dist target/wasm32-unknown-unknown/release/grains.wasm
cp web/index.html dist/
```

Serve `dist` over HTTP and open it in a browser with WebGPU.

## GitHub Pages

GitHub Pages deploys from `main` only after the repo's Pages source is set to GitHub Actions.

## Roadmap

7. A colourize pass that replaces the debug colours with a palette.
8. Bloom: bright pass, separable blur, composite.
9. An egui overlay for the material picker.
