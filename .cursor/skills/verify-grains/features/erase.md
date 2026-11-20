# Erase

Erase removes cells under the pointer. Hold Shift and drag with the right button. A right drag without Shift pans the view instead, and a pan moves the sky sample.

## Sub-features

- `erase-mark` paints a stone disc while paused.
- `erase-drag` shift-right-drags across that disc.
- `erase-local` requires the stroke sample to return to the sky colour while the corner stays.

## How to get to it (user POV)

- Press 2, left-drag to draw stone, then hold Shift and right-drag over the same cells.
- The right button without Shift pans. The middle button pans as well.

## Driving it with drive.mjs

Preconditions:

- `doctor ok` is true for this session.
- `reload`, then pause and paint stone, which is the mark erase removes.

```bash
node .cursor/skills/verify-grains/scripts/drive.mjs run --evidence .cursor/skills/verify-grains/evidence/erase --actions reload,wait-input,key:Space,key:2,wait-frame,settle,drag:700:300:760:360:748:348,assert-paint,erase:700:300:760:360:748:348,assert-erased
```

- **Paint stone.** Same actions as [Paint](./paint.md) through `assert-paint`.
- **Erase.** `erase:700:300:760:360:748:348` holds Shift, right-drags through the same points, and releases both.
- **Proof.** `assert-erased` passes when the stroke sample comes back within 20 of the pre-paint sky sample, the stone mark is gone by more than 20, and the corner stays within 5. It fails if the edit after the mark was the C key or another left drag. `pixels.json` stores `restored`.

## Gotchas

- Right-drag without Shift pans. The corner check fails that pan instead of calling it an erase.
- Clear also returns this one sample to sky. `assert-erased` does not accept a recipe whose edit was `key:c`.
- The canvas calls `preventDefault` on the context menu. A right click should not open one.
