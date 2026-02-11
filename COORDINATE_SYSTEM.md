# Coordinate System

The game uses a sheared orthographic projection (not isometric). Each kingdom is a 1024x1024 grid of game tiles.

## Game-to-pixel transform (at 25% zoom)

```
pixel_dx = 49.40 * game_dx
pixel_dy = -1.50 * game_dx + 28.32 * game_dy
```

- Game X maps almost purely to screen horizontal (49.40 px per tile)
- Game Y maps to screen vertical (28.32 px per tile)
- There is a small tilt: each game X unit shifts the view down by -1.50 px

## Pixel-to-game inverse

```
game_dx = screen_dx / 49.40
game_dy = (screen_dy + 1.50 * game_dx) / 28.32
```

Where `screen_dx` / `screen_dy` are pixel offsets from screen center (960, 540).

## Constants in code

`src/scanner.rs`:

```rust
const PX_PER_GAME_X: f64 = 49.40;
const PX_PER_GAME_Y: f64 = 28.32;
const TILT_Y: f64 = -1.50;
```

## Calibration source

Derived from two known buildings in K:111 at 25% zoom:

| Building coords | Pixel offset from center |
|----------------|-------------------------|
| (502, 512) | (-494, +15) |
| (528, 524) | (+791, +316) |

## Known vertical offset

Template matching finds the visual center of a building sprite, but building sprites are taller than their tile footprint. This causes a consistent ~15-19px vertical offset between the matched pixel position and the tile's actual game coordinate anchor. This is small enough that clicking at screen center after a `navigate_to_coords` still lands on the correct tile.

## Re-calibration

If the zoom level changes, re-calibrate using:

1. `detector::find_best_match()` - returns the single highest-scoring match regardless of threshold
2. The scanner logs a `CALIBRATION:` line after each goto showing the pixel error from screen center
