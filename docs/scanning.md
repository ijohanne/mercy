# Scanning

How the scanner navigates the game map and finds exchanges.

- [Coordinate system](#coordinate-system)
- [Exchange spawn distribution](#exchange-spawn-distribution)
- [Viewport geometry](#viewport-geometry)
- [Scan patterns](#scan-patterns)
  - [`wide` -- single large spiral](#wide----single-large-spiral)
  - [`multi` -- 9 interleaved spirals](#multi----9-interleaved-spirals)
  - [`grid` -- full map sweep](#grid-default----full-map-sweep)
  - [`single` -- small spiral](#single----small-spiral)
- [Exchange logging](#exchange-logging)

## Coordinate system

The game uses a sheared orthographic projection (not isometric). Each kingdom is a 1024x1024 grid of game tiles.

### Game-to-pixel transform (at 25% zoom)

```
pixel_dx = 49.40 * game_dx
pixel_dy = -1.50 * game_dx + 28.32 * game_dy
```

- Game X maps almost purely to screen horizontal (49.40 px per tile)
- Game Y maps to screen vertical (28.32 px per tile)
- There is a small tilt: each game X unit shifts the view down by -1.50 px

### Pixel-to-game inverse

```
game_dx = screen_dx / 49.40
game_dy = (screen_dy + 1.50 * game_dx) / 28.32
```

Where `screen_dx` / `screen_dy` are pixel offsets from screen center (760, 400).

### Constants in code

`backend/src/scanner.rs`:

```rust
const PX_PER_GAME_X: f64 = 49.40;
const PX_PER_GAME_Y: f64 = 28.32;
const TILT_Y: f64 = -1.50;
```

### Calibration source

Derived from two known buildings in K:111 at 25% zoom:

| Building coords | Pixel offset from center |
|----------------|-------------------------|
| (502, 512) | (-494, +15) |
| (528, 524) | (+791, +316) |

### Known vertical offset

Template matching finds the visual center of a building sprite, but building sprites are taller than their tile footprint. This causes a consistent ~15-19px vertical offset between the matched pixel position and the tile's actual game coordinate anchor. This is small enough that clicking at screen center after a `navigate_to_coords` still lands on the correct tile.

### Re-calibration

If the zoom level changes, re-calibrate using:

1. `detector::find_best_match()` - returns the single highest-scoring match regardless of threshold
2. The scanner logs a `CALIBRATION:` line after each goto showing the pixel error from screen center

## Exchange spawn distribution

Analysis of 48 observed exchange locations (41 unique) collected from `example-locations.txt`. All coordinates are game tiles within a 1024x1024 kingdom grid.

### Raw data (sorted, deduplicated)

```
(40,458)   (71,727)   (100,684)  (118,668)  (144,214)  (159,709)
(185,363)  (201,577)  (202,864)  (217,203)  (229,817)  (236,782)
(244,822)  (257,867)  (275,233)  (290,928)  (342,98)   (361,903)
(416,844)  (433,187)  (505,43)   (531,117)  (632,158)  (640,850)
(667,873)  (685,89)   (709,863)  (758,298)  (816,808)  (831,207)
(840,652)  (841,689)  (859,427)  (859,877)  (866,578)  (872,294)
(875,223)  (932,648)  (940,208)  (947,573)  (970,412)
```

### Key findings

- **X range**: 40 -- 970 (nearly full map width)
- **Y range**: 43 -- 928 (nearly full map height)
- **Mean**: X=515, Y=533 (close to center, but the distribution is hollow)
- **Distance from center (512,512)**: all 41 points are >300 tiles from center
- **0 points** within 150 tiles of center, **0** within 300 tiles

The playable map extends nearly to the edges. The previous assumption that ocean/unusable terrain starts at ~200 was wrong.

### Distribution by 100-tile bins

```
X axis:                              Y axis:
  0-99:   ## (2)                       0-99:   ### (3)
100-199:  ##### (5)                  100-199:  ### (3)
200-299:  ######### (9)              200-299:  ######## (8)
300-399:  ## (2)                     300-399:  # (1)
400-499:  ## (2)                     400-499:  ### (3)
500-599:  ## (2)                     500-599:  ### (3)
600-699:  #### (4)                   600-699:  ##### (5)
700-799:  ## (2)                     700-799:  ### (3)
800-899:  ######### (9)              800-899:  ########## (10)
900-999:  #### (4)                   900-999:  ## (2)
```

Exchanges cluster near the edges and avoid the 300-600 center band. The 200-299 and 800-899 bins are the most populated on both axes.

> **Note:** 41 unique points is a small sample. As `exchanges.jsonl` accumulates confirmed locations, this analysis should be updated to verify the donut-shaped distribution holds and to check whether any map regions are underrepresented.

## Viewport geometry

At 25% zoom the browser viewport (1920x1080) shows approximately **34 x 33 game tiles** of usable detection area. The exact usable center is at pixel (760, 400) due to UI elements (minimap, toolbars, etc.) shifting the playable viewport.

This means a scan position at game coordinate (X, Y) can detect buildings within roughly X +/- 17, Y +/- 17.

## Scan patterns

Set via `MERCY_SCAN_PATTERN` (default: `grid`). Override ring count with `MERCY_SCAN_RINGS`.

All time estimates assume ~2.2 seconds per position (750ms navigate delay + screenshot + detection overlap). Detection rates are computed against 41 example locations using a ±17 tile viewport radius (i.e. whether any scan position actually passes close enough to detect each exchange, not just bounding-box coverage).

### Average time to first detection

| Pattern | Finds | Avg | Median | Min | Max |
|---------|-------|-----|--------|-----|-----|
| `wide` | 19/41 | 7.3 min | 6.9 min | 3.0 min | 12.1 min |
| `multi` | 22/41 | 11.6 min | 10.9 min | 0.2 min | 26.4 min |
| `grid` | **41/41** | 20.2 min | 22.3 min | 0.6 min | 35.6 min |
| `single` | 0/41 | -- | -- | -- | -- |

`wide` and `multi` are faster when they find something, but their deterministic positions leave permanent blind spots -- no number of repeated passes will detect exchanges in those gaps. `grid` guarantees detection within a single pass.

> **Note:** These statistics are based on only 41 unique sample points. More data from `exchanges.jsonl` will improve confidence in these numbers and may shift the optimal default. Re-run this analysis as the dataset grows.

### `wide` -- single large spiral

A single spiral centered at (512, 512) with step=50 (double the normal step). The large step allows many more rings before hitting the map boundary.

- **Default rings**: 9
- **Coverage area**: center +/- (50 x 9) = +/- 450 tiles -> range 62-962
- **Positions**: 361 (after clamp dedup)
- **Gaps within coverage**: step (50) minus viewport width (34) = 16 tile gaps between adjacent positions. Buildings in these gaps are not detected.
- **Detection rate on example data**: 19/41 = **46%**

Best balance of speed and reach. Covers the full map area quickly, though the 16-tile inter-position gaps reduce actual detection below the area coverage.

| Time | Positions | Exchanges detectable | % |
|------|-----------|---------------------|---|
| 0s | 0/361 | 0/41 | 0% |
| 20s | 10/361 | 0/41 | 0% |
| 1 min | 28/361 | 0/41 | 0% |
| 2 min | 55/361 | 0/41 | 0% |
| 5 min | 137/361 | 2/41 | 4% |
| 8 min | 219/361 | 14/41 | 34% |
| 10 min | 273/361 | 16/41 | 39% |
| **13 min** (done) | **361** | **19/41** | **46%** |

### `multi` -- 9 interleaved spirals

Places 9 spiral centers in a 3x3 grid to probe the full map:

```
(150,150)  (512,150)  (874,150)
(150,512)  (512,512)  (874,512)
(150,874)  (512,874)  (874,874)
```

Each spiral uses step=25 (the global `SCAN_STEP`). The spirals are interleaved by ring level: first all 9 centers (ring 0), then ring 1 of all 9 centers, etc. This gives broad spatial coverage within the first few seconds.

- **Default rings**: 4
- **Positions per center**: 1 + 8 + 16 + 24 + 32 = 81
- **Total**: 9 x 81 = 729 positions
- **Coverage per center**: center +/- (step x rings + 17) = +/- 117 tiles
- **Coverage zones**: 33-267, 395-629, 757-991 on each axis
- **Gaps**: ~128 tiles between zones (no detection in these bands)
- **Detection rate on example data**: 22/41 = **53%**

No gaps within each zone (step=25 < viewport=34), but the 128-tile inter-zone gaps miss exchanges in those bands. The interleaving means all 9 zones get a first probe (ring 0) in the first 9 steps (~20 seconds).

| Time | Positions | Exchanges detectable | % |
|------|-----------|---------------------|---|
| 0s | 0/729 | 0/41 | 0% |
| 20s | 10/729 | 1/41 | 2% |
| 1 min | 28/729 | 1/41 | 2% |
| 2 min | 55/729 | 2/41 | 4% |
| 5 min | 137/729 | 7/41 | 17% |
| 8 min | 219/729 | 10/41 | 24% |
| 10 min | 273/729 | 10/41 | 24% |
| 15 min | 410/729 | 15/41 | 36% |
| 20 min | 546/729 | 18/41 | 43% |
| 25 min | 682/729 | 21/41 | 51% |
| **27 min** (done) | **729** | **22/41** | **53%** |

### `grid` (default) -- full map sweep

Visits every point on a regular grid from (30,30) to (960,960) with step=30.

- **Step**: 30 (hardcoded, ignores `SCAN_STEP`)
- **Positions per axis**: (960 - 30) / 30 + 1 = 32
- **Total positions**: 32 x 32 = 1024
- **Coverage**: 30-960 on both axes (with viewport: 13-977)
- **Gaps**: none (step=30 < viewport width=34, so positions overlap)
- **Detection rate on example data**: 41/41 = **100%**

The thorough option. Scans row by row, left to right, top to bottom.

| Time | Positions | Exchanges detectable | % |
|------|-----------|---------------------|---|
| 0s | 0/1024 | 0/41 | 0% |
| 20s | 10/1024 | 0/41 | 0% |
| 1 min | 28/1024 | 1/41 | 2% |
| 2 min | 55/1024 | 1/41 | 2% |
| 5 min | 137/1024 | 4/41 | 9% |
| 8 min | 219/1024 | 8/41 | 19% |
| 10 min | 273/1024 | 12/41 | 29% |
| 15 min | 410/1024 | 15/41 | 36% |
| 20 min | 546/1024 | 18/41 | 43% |
| 25 min | 682/1024 | 22/41 | 53% |
| 30 min | 819/1024 | 29/41 | 70% |
| **38 min** (done) | **1024** | **41/41** | **100%** |

### `single` -- small spiral

A single spiral centered at (512, 512) with step=25.

- **Default rings**: 4
- **Coverage**: 512 +/- 117 = 395-629
- **Positions**: 81
- **Detection rate on example data**: 0/41 = **0%** (no exchanges spawn this close to center)

Only useful for development/testing or if an exchange is known to be near center.

| Time | Positions | Exchanges detectable | % |
|------|-----------|---------------------|---|
| 0s | 0/81 | 0/41 | 0% |
| 1 min | 28/81 | 0/41 | 0% |
| 2 min | 55/81 | 0/41 | 0% |
| **3 min** (done) | **81** | **0/41** | **0%** |

## Exchange logging

All `confirm_match` outcomes (confirmed, estimate, and rejected) are appended as JSON lines to the file configured by `MERCY_EXCHANGE_LOG` (default: `exchanges.jsonl`). Each line contains:

```json
{
  "timestamp": "2025-01-15T12:34:56Z",
  "kingdom": 111,
  "x": 872,
  "y": 294,
  "confirmed": true,
  "stored": true,
  "initial_score": 0.9523,
  "calibration_score": 0.9801,
  "scan_pattern": "multi",
  "scan_duration_secs": 142.5
}
```

- `confirmed`: popup text contained valid coordinates
- `stored`: exchange was added to state (false if duplicate)
- `initial_score`: template match score from the scan screenshot
- `calibration_score`: template match score from the goto screenshot (null if no match)
