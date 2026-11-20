# Paint

Paint draws the selected material in a disc under the pointer. Stone stays where it was drawn while the fall is paused. The corner of the sky does not move.

## Sub-features

- `paint-pause` stops the fall before the stroke, so falling sand is not the sample.
- `paint-stone` selects stone with the 2 key.
- `paint-stroke` drags the pointer across the stroke sample.
- `paint-hold` reads the same sample again after the button is up.

## How to get to it (user POV)

- Focus the page and press 2, then drag with the left button.
- The same drag paints whichever material is selected. A fresh load selects sand. This recipe selects stone first.

## Driving it with drive.mjs

Preconditions:

- `doctor ok` is true for this session.
- The action list starts with `reload`, which is the demo world.

```bash
node .cursor/skills/verify-grains/scripts/drive.mjs run --evidence .cursor/skills/verify-grains/evidence/paint --actions reload,wait-input,key:Space,key:2,wait-frame,settle,drag:700:300:760:360:748:348,assert-paint
```

- **Pause.** Press Space. The action is `key:Space`. The later `settle` step fails if the stroke sample is still changing.
- **Choose stone.** Press 2. The action is `key:2`.
- **Wait for the frame.** `wait-frame` returns when the sky sample has channel spread above 8 and the panel sample has spread of 3 or less.
- **Drag.** Left button from `(700, 300)` through `(760, 360)` to `(748, 348)`. The action is `drag:700:300:760:360:748:348`.
- **Proof.** `assert-paint` requires the stroke sample to stay more than 20 away from the pre-drag sample, and the corner and panel to stay within 5. `pixels.json` in the evidence directory stores `baseline`, `after`, and `held`.

## Gotchas

- A running fall can change the stroke sample with no pointer input. `settle` exists so that change cannot pass as a stroke.
- A camera move or a full redraw changes the corner sample too. `assert-paint` fails when the corner moves.
- The panel occupies the upper left. A drag that starts there does not paint.
- `screenshot.png` stays blank. The proof is `pixels.json`.
