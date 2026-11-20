# Pause

Pause freezes the fall. Sand drawn while paused stays on the stroke sample. Pressing Space again lets that sand fall, and the sample changes. Stone cannot show this, because stone does not fall.

## Sub-features

- `pause-hold` paints sand while paused and requires the sample to hold.
- `pause-resume` presses Space again.
- `pause-fall` requires the sand mark to leave the sample while the sky corner stays.

## How to get to it (user POV)

- Press Space to pause. The panel checkbox is the same control, drawn inside the canvas.
- Press 3 for sand, drag, then press Space again.

## Driving it with drive.mjs

Preconditions:

- `doctor ok` is true for this session.
- `reload` starts the fall. The first `key:Space` is what pauses it. There is no separate paused save.

```bash
node .cursor/skills/verify-grains/scripts/drive.mjs run --evidence .cursor/skills/verify-grains/evidence/pause --actions reload,wait-input,key:Space,key:3,wait-frame,settle,drag:700:300:760:360:748:348,assert-paint,key:Space,assert-falls
```

- **Pause and choose sand.** `key:Space` then `key:3`.
- **Paint.** The drag and `assert-paint` are the same stroke as [Paint](./paint.md). The mark has to hold while paused.
- **Resume.** The second `key:Space` runs the fall.
- **Proof.** `assert-falls` waits until the stroke sample moves more than 20 away from the held sand mark. The corner has to stay within 5 of the paused sky. `pixels.json` stores `held` and `fell`.

## Gotchas

- Pausing after the sand has already fallen makes `assert-falls` wait on an empty sample. The recipe paints while paused, then resumes.
- Fire and smoke keep changing colour when the fall is running. This recipe uses sand.
- One Space press toggles. A third press would pause again and the sand would stay.
