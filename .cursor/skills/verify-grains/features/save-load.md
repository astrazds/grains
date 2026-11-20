# Save and load

Save downloads the grid as `world.grains`. Load opens a file picker and replaces the grid with that file. In the browser, S and L are those two actions. The native window writes `world.grains` in the current directory instead of downloading it.

## Sub-features

- `save-download` presses S and stores the download.
- `save-magic` requires the file to start with `GRNS`.
- `load-restore` clears the mark, loads the file, and requires the mark to return.

## How to get to it (user POV)

- Press S. The browser downloads `world.grains`. The panel button labelled save does the same thing.
- Press L and choose that file. The panel button labelled load opens the same picker.
- Copy link writes a hash on the page address. This recipe uses the file.

## Driving it with drive.mjs

Preconditions:

- `doctor ok` is true for this session.
- Paint stone while paused, then save. The download is the file load reads back. There is no checked-in world file.

```bash
node .cursor/skills/verify-grains/scripts/drive.mjs run --evidence .cursor/skills/verify-grains/evidence/save-load --actions reload,wait-input,key:Space,key:2,wait-frame,settle,drag:700:300:760:360:748:348,assert-paint,save,key:c,assert-cleared,load,assert-mark
```

- **Paint stone.** Same actions as [Paint](./paint.md) through `assert-paint`.
- **Save.** `save` presses S, waits for the download named `world.grains`, and writes it in the evidence directory.
- **Clear.** `key:c` then `assert-cleared`, so the sample is sky before the load.
- **Load.** `load` presses L and gives the picker that same `world.grains`.
- **Proof.** `assert-mark` waits until the stroke sample is within 20 of the stone mark again. The file's first four bytes are `GRNS`. `pixels.json` stores `reloaded`.

## Gotchas

- Save reads the grid back from the GPU. The download starts after that read. The action waits up to 20 seconds.
- A file with the wrong magic, the wrong version, or a short body is refused. The recipe uses the download from this page, not a hand-built file.
- Load on the native window reads `./world.grains` and does not open a picker. This recipe is the browser page.
