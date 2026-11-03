# grains

grains is a GPU falling-sand sandbox. The same frame runs in a native window and in the browser on WebGPU.

## Run the native window

Install the pinned Rust toolchain, then start the window.

```
mise install
cargo run
```

The window title is grains. The inner size is 1024 by 600 physical pixels.

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

3. Ping-pong storage and a compute pass that copies the grid.
4. Margolus 2x2 blocks with an alternating offset.
5. Material rules for sand, water, and stone.
6. A brush compute pass.
7. A colourize pass that replaces the debug RGB with a palette.
8. Bloom: bright pass, separable blur, composite.
9. An egui overlay for the material picker.
