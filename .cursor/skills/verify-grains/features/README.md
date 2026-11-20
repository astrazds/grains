# Grains verification map

Drive the browser build with `drive.mjs`. Read this index, then use one feature file as the recipe. The commands assume the repository root and a session that has already printed `ready`.

## Baseline preconditions

- Build `dist/` with the commands in `../SKILL.md`.
- Launch with `node .cursor/skills/verify-grains/scripts/drive.mjs launch --dist dist --port 8876 --display :97`.
- Require a `ready` line before any recipe.
- Run `node .cursor/skills/verify-grains/scripts/drive.mjs doctor` and require `doctor ok`.
- Start every recipe with `reload`. That load is the demo world, sand selected, fall running, radius 2.
- Do not drive a page this session did not start.

## Driving conventions

- `#grains` is the only element recipes touch. `wait-input` continues only when `#grains` and `#grains[tabindex]` each match one element, then focuses the canvas.
- `wait-frame` continues only when the sky sample and the panel sample show a drawn frame and `#grains-webgpu` is absent.
- Keys in the recipes are the keys a person presses. `key:2` is the 2 key.
- Pointer numbers are CSS pixels. The stroke proof pixel is under `(748, 348)`.
- Keep `pixels.json`. Cleanup does not remove it.

## Proof and skip reporting

- A passing recipe writes `pixels.json` whose samples show the edit and a later read of the same pixel.
- The corner sample has to stay put during a local edit. If it moves, the recipe did not show that edit.
- Report a recipe that cannot run with the command and the unmet precondition. Do not mark it passed through a different recipe.

## Features

- [Paint](./paint.md) draws stone while the fall is paused.
- [Pause](./pause.md) holds a sand mark, then lets it fall.
- [Erase](./erase.md) removes a stone mark with shift and the right button.
- [Clear](./clear.md) removes a stone mark with the C key and no pointer move.
- [Save and load](./save-load.md) downloads `world.grains` and loads that file back.
