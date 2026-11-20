# Clear

Clear empties the grid. Press C. The pointer does not have to be on the cells. The stroke sample returns to sky, including a mark the pointer never revisits.

## Sub-features

- `clear-mark` paints stone while paused.
- `clear-key` presses C with no further pointer action.
- `clear-sky` requires that sample to return to the pre-paint sky colour.

## How to get to it (user POV)

- Press C. The panel button labelled clear does the same thing. It is drawn inside the canvas, not as a DOM button.
- The fall can stay paused. Clear still removes the cells.

## Driving it with drive.mjs

Preconditions:

- `doctor ok` is true for this session.
- Paint a stone mark first. A fresh grid has no mark at the stroke sample, so C would not change it.

```bash
node .cursor/skills/verify-grains/scripts/drive.mjs run --evidence .cursor/skills/verify-grains/evidence/clear --actions reload,wait-input,key:Space,key:2,wait-frame,settle,drag:700:300:760:360:748:348,assert-paint,key:c,assert-cleared
```

- **Paint stone.** Same actions as [Paint](./paint.md) through `assert-paint`.
- **Clear.** `key:c` with the pointer up.
- **Proof.** `assert-cleared` passes when the stroke sample comes back within 20 of the sky sample and the corner stays within 5. It fails if a drag or an erase ran after the mark. `pixels.json` stores `restored`.

## Gotchas

- Erase of this same pixel looks the same in `pixels.json`. The action list is what makes it a clear. Do not add a drag after `assert-paint`.
- C also drops a stroke that was still queued. Pause first so the fall does not repaint the sample before the check.
