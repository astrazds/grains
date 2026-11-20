---
name: verify-grains
description: Drive the grains falling-sand browser build on WebGPU and prove a pointer stroke changed canvas pixels. Use when launching grains, checking the local page, or showing that paint, pause, erase, clear, or save changed the world.
---

# Verify grains

grains is a falling-sand sandbox. The browser build is the page to drive. It is the same build published at https://astrazds.github.io/grains/. The native window is `cargo run` after `mise install`. That window is titled grains and is 1024 by 600. It needs a display. This skill does not drive it.

Run every command from the repository root. Paths below are relative to that root.

## Launch

Build the page once. `dist/` is gitignored.

```bash
rustup target add wasm32-unknown-unknown
cargo build --release --lib --locked --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir dist target/wasm32-unknown-unknown/release/grains.wasm
cp web/index.html dist/index.html
```

Start the page, Xvfb, and Chromium. The driver downloads a local lavapipe build and Playwright 1.62.0 into `.cursor/skills/verify-grains/.tools/` when they are missing. It uses software Vulkan, not a screenshot.

```bash
node .cursor/skills/verify-grains/scripts/drive.mjs launch --dist dist --port 8876 --display :97
```

Leave that process running. Ready is a stdout line that starts with `ready` and includes `setup_ms`. The process prints it only after all of these are true.

- The verify server owns `127.0.0.1:8876` and serves `/?probe=1`.
- `#grains` matches one element, and `#grains[tabindex]` matches one element.
- `#grains-webgpu` is absent.
- `window.grainsPixels.corner` has a channel spread above 8, and `panel` has a spread of 3 or less.

Spread is the largest RGB channel minus the smallest. The corner sample is the sky. The panel sample is the egui window. A listening port without that line is not ready.

Launch refuses a second session, a taken port, or a taken display. Two browsers on one page will fight over the same stroke sample.

## Doctor

Run this before a drive whenever the page looks wrong. It does not click, type, or reload.

```bash
node .cursor/skills/verify-grains/scripts/drive.mjs doctor
```

Exit 0 prints `doctor ok` with the url, the driver pid, the port, `canvas=1`, and the wasm size. Anything else means do not drive. The check fails if the port is not this session, the wasm is under 100000 bytes, the canvas count is not 1, the fallback node is present, or the corner and panel samples are not a drawn frame.

## Drive

Read `features/README.md`, then one feature file. Run that file's action list unchanged. `reload` is the empty state. It loads the demo world, with sand selected and the fall running.

The page has one canvas, `#grains`. The panel is drawn inside that canvas. There are no DOM buttons. Keys are the stable controls. The pointer actions are CSS pixels in a 1024 by 600 viewport at `deviceScaleFactor` 1.5. The stroke sample is the canvas pixel under CSS `(748, 348)`. A drag has to end there. The left panel covers the top-left of the page, and the brush does not paint while the pointer is over it.

```bash
node .cursor/skills/verify-grains/scripts/drive.mjs run --evidence .cursor/skills/verify-grains/evidence/paint --actions reload,wait-input,key:Space,key:2,wait-frame,settle,drag:700:300:760:360:748:348,assert-paint
```

`key:2` selects stone. `key:Space` pauses the fall. `settle` refuses to continue when the corner, panel, or stroke sample is still changing. `assert-paint` passes only when the stroke sample stays changed by more than 20 and the corner and panel stay within 5. A frame that changes everywhere is not a brush mark. A sample that flickers for one read is not a mark.

The same command shape is in each feature file. `pass evidence=... verify_ms=...` is a pass. A thrown check is a failure. Write the failure down and leave the journey as it is.

## Evidence

The driver writes the directory you pass to `--evidence`. For the mapped features those directories are:

- `.cursor/skills/verify-grains/evidence/paint`
- `.cursor/skills/verify-grains/evidence/pause`
- `.cursor/skills/verify-grains/evidence/erase`
- `.cursor/skills/verify-grains/evidence/clear`
- `.cursor/skills/verify-grains/evidence/save-load`

`pixels.json` is the proof. It stores the samples from before the edit, during the pointer action, and after the button is up. `console.txt` is the page console. `screenshot.png` is a Playwright capture of the page. On this machine that capture stays blank, and `canvas.getContext("2d")` reads the WebGPU canvas as transparent. Do not treat either one as a pass or a failure of the simulation. `save` also writes `world.grains` in the evidence directory. The first four bytes are `GRNS`.

A pass has to come from the real page. Do not set materials from the console, and do not call an internal setter. `?probe=1` only copies three presented-texture pixels into `window.grainsPixels`. It does not paint.

## Cleanup

Stop the session the driver started. This kills that driver pid, its Xvfb pid, and its Chromium pid when `/proc/<pid>/cmdline` still matches. It does not kill by process name. It deletes `.cursor/skills/verify-grains/run/` and leaves every evidence directory listed above.

```bash
node .cursor/skills/verify-grains/scripts/drive.mjs cleanup
```

`cleanup ok` means the processes are gone. Confirm the evidence directory from the drive still contains `pixels.json`. Run cleanup after a failed launch too, so a broken attempt does not keep the port.

## Helpers

`drive.mjs` is the only helper. It is the way to start the browser. Do not read Playwright or `web/smoke.mjs` to find the WebGPU flags.

```bash
node .cursor/skills/verify-grains/scripts/drive.mjs launch --dist dist --port 8876 --display :97
node .cursor/skills/verify-grains/scripts/drive.mjs doctor
node .cursor/skills/verify-grains/scripts/drive.mjs run --evidence .cursor/skills/verify-grains/evidence/paint --actions reload,wait-input,key:Space,key:2,wait-frame,settle,drag:700:300:760:360:748:348,assert-paint
node .cursor/skills/verify-grains/scripts/drive.mjs cleanup
```

## Keep the map current

When the page's controls change, run `/maintain-verification-skill` and update the feature file that drifted.
