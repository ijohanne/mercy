# Scanning

How the scanner navigates the game map and finds exchanges.

- [Coordinate system](#coordinate-system)
- [Exchange spawn distribution](#exchange-spawn-distribution)
- [Viewport geometry](#viewport-geometry)
- [Scan patterns](#scan-patterns)
  - [`known` -- compiled-in historical hotspots](#known----compiled-in-historical-hotspots)
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

Analysis of 104,297 historical exchange spawns across 295 kingdoms (75,141 unique coordinate pairs). Data is compiled into the binary at build time from `backend/assets/known_locations.csv`.

### Key findings

- **295 kingdoms** with historical spawn data
- **~337 unique locations per kingdom** on average (max 885 in K:10)
- Spawns cover nearly the full 1024x1024 map, clustering near edges
- The center band (300-600) has fewer spawns, confirming the donut-shaped distribution seen in earlier small samples
- Locations are reused across respawns — frequency data enables density-based scan ordering

## Viewport geometry

At 25% zoom the browser viewport (1920x1080) shows approximately **34 x 33 game tiles** of usable detection area. The exact usable center is at pixel (760, 400) due to UI elements (minimap, toolbars, etc.) shifting the playable viewport.

This means a scan position at game coordinate (X, Y) can detect buildings within roughly X +/- 17, Y +/- 17.

## Scan patterns

Set via `MERCY_SCAN_PATTERN` (default: `grid`). Override ring count with `MERCY_SCAN_RINGS`.

All time estimates assume ~2.2 seconds per position (750ms navigate delay + screenshot + detection overlap).

### Pattern comparison

Benchmarked against all 99,477 unique historical spawn locations across 295 kingdoms. Detection rate = percentage of those locations within ±17 tiles (one viewport) of any scan position.

#### Detection rate (will you find it at all?)

| Pattern | Positions | Detection rate | Min | Median | Max |
|---------|----------:|---------------:|----:|-------:|----:|
| `known@100%` | ~241 | **100.0%** | 100.0% | 100.0% | 100.0% |
| `grid` | 1024 | 99.6% | 97.8% | 100.0% | 100.0% |
| `known@90%` | ~206 | 92.8% | 88.5% | 92.7% | 100.0% |
| `known@80%` | ~171 | 84.5% | 72.2% | 84.3% | 100.0% |
| `known@70%` | ~136 | 75.3% | 61.1% | 75.2% | 100.0% |
| `multi` | 729 | 57.4% | 0.0% | 57.1% | 100.0% |
| `wide` | 361 | 48.8% | 0.0% | 48.6% | 100.0% |
| `single` | 81 | 0.0% | 0.0% | 0.0% | 3.6% |

`known@100%` achieves perfect detection in ~241 positions — compared to `grid`'s 1024 positions for 99.6%. Meanwhile `multi` and `wide` use 729 and 361 positions respectively but only find 57% and 49% of exchanges due to blind spots.

#### Time to first detection (how fast do you find it?)

| Pattern | Positions | Avg time | Median time | P90 time | Misses |
|---------|----------:|---------:|------------:|---------:|-------:|
| `known@70%` | ~136 | **2.3 min** | **1.9 min** | **5.0 min** | 24,602 |
| `known@80%` | ~171 | 2.8 min | 2.2 min | 6.2 min | 15,417 |
| `known@90%` | ~206 | 3.3 min | 2.5 min | 7.5 min | 7,175 |
| `known@100%` | ~241 | 3.7 min | 2.8 min | 8.7 min | 0 |
| `single` | 81 | 1.5 min | 1.8 min | 2.8 min | 99,428 |
| `wide` | 361 | 7.6 min | 7.5 min | 11.5 min | 50,950 |
| `multi` | 729 | 12.2 min | 11.7 min | 24.0 min | 42,363 |
| `grid` | 1024 | 18.3 min | 17.5 min | 33.3 min | 355 |

`known@70%` finds detectable exchanges in a median of **1.9 minutes** — 9x faster than `grid` (17.5 min median). Even `known@100%` at 2.8 min median is 6x faster than `grid`, because density-sorting checks the most likely spots first.

`wide` and `multi` have no intelligence about where exchanges actually spawn — they scan geometric patterns that happen to miss over half the map. `grid` is thorough but slow. The `known` pattern is both fast and accurate because it targets historical hotspots.

**Recommendation**: Use `known` with 70-80% coverage when cycling through many kingdoms (speed over guaranteed detection). Use 90-100% when scanning fewer kingdoms and wanting high confidence.

### `known` -- compiled-in historical hotspots

Uses 104,297 historical spawn records compiled into the binary at build time. For each kingdom, locations are clustered into 25x25 game-unit cells (matching the viewport) and sorted by descending spawn frequency — the most historically active areas are scanned first.

- **Data**: 295 kingdoms, ~337 unique locations per kingdom (avg), pre-clustered into ~240-450 scan positions
- **Ordering**: density-sorted (most frequent spawn cells first)
- **Per-kingdom**: only locations for the kingdom being scanned are visited
- **Fallback**: kingdoms without historical data fall back to `grid`
- **No external files**: data is compiled into the binary from `backend/assets/known_locations.csv`

Since positions are density-sorted, the exchange is most likely found in the first ~100 positions (the historical hotspots), well before the full scan completes.

#### Top 20 kingdoms by data volume

Positions at each coverage tier (100% = all cells, lower = only the densest hotspots):

| Kingdom | Spawns | 70% | 80% | 90% | 100% |
|--------:|-------:|----:|----:|----:|-----:|
| 10 | 935 | 203 (7.4m) | 249 (9.1m) | 338 (12.4m) | 431 (15.8m) |
| 157 | 877 | 216 (7.9m) | 275 (10.1m) | 363 (13.3m) | 450 (16.5m) |
| 89 | 810 | 228 (8.4m) | 309 (11.3m) | 390 (14.3m) | 471 (17.3m) |
| 158 | 787 | 212 (7.8m) | 291 (10.7m) | 370 (13.6m) | 448 (16.4m) |
| 28 | 785 | 203 (7.4m) | 274 (10.0m) | 353 (12.9m) | 431 (15.8m) |
| 59 | 770 | 211 (7.7m) | 288 (10.6m) | 365 (13.4m) | 442 (16.2m) |
| 44 | 766 | 181 (6.6m) | 230 (8.4m) | 307 (11.3m) | 383 (14.0m) |
| 155 | 748 | 209 (7.7m) | 284 (10.4m) | 359 (13.2m) | 433 (15.9m) |
| 75 | 739 | 185 (6.8m) | 255 (9.3m) | 329 (12.1m) | 402 (14.7m) |
| 147 | 700 | 189 (6.9m) | 249 (9.1m) | 319 (11.7m) | 389 (14.3m) |
| 173 | 694 | 187 (6.9m) | 257 (9.4m) | 326 (12.0m) | 395 (14.5m) |
| 190 | 687 | 204 (7.5m) | 273 (10.0m) | 342 (12.5m) | 410 (15.0m) |
| 94 | 687 | 211 (7.7m) | 280 (10.3m) | 349 (12.8m) | 417 (15.3m) |
| 51 | 683 | 181 (6.6m) | 242 (8.9m) | 310 (11.4m) | 378 (13.9m) |
| 166 | 657 | 198 (7.3m) | 264 (9.7m) | 330 (12.1m) | 395 (14.5m) |
| 62 | 650 | 187 (6.9m) | 252 (9.2m) | 317 (11.6m) | 382 (14.0m) |
| 7 | 633 | 180 (6.6m) | 243 (8.9m) | 306 (11.2m) | 369 (13.5m) |
| 40 | 611 | 181 (6.6m) | 237 (8.7m) | 300 (11.0m) | 361 (13.2m) |
| 24 | 585 | 172 (6.3m) | 232 (8.5m) | 296 (10.9m) | 355 (13.0m) |
| 31 | 580 | 160 (5.9m) | 217 (8.0m) | 275 (10.1m) | 328 (12.0m) |

*295 kingdoms total. Full stats available via `known_locations::KINGDOM_STATS`.*
*Scan times in parentheses assume ~2.2s per position.*

#### Coverage tiers (`MERCY_KNOWN_COVERAGE`)

Not every historical spawn location is equally likely. The `MERCY_KNOWN_COVERAGE` setting (default: 80) controls what percentage of historical spawn weight to cover before stopping. Lower values scan fewer positions — dramatically faster, at the cost of skipping the rarest spawn locations.

Since exchanges pop up, get taken, and respawn frequently, speed matters more than guaranteed detection. Finding the *next* exchange quickly is more valuable than ensuring you never miss one in an unlikely spot.

| Coverage | Avg positions | Avg scan time | Positions saved | Recommended for |
|---------:|--------------:|--------------:|----------------:|-----------------|
| **70%** | **136** | **5.0 min** | **40%** | Fast cycling through many kingdoms |
| **80%** | 171 | 6.3 min | 27% | Default — good balance of speed and coverage |
| **90%** | 206 | 7.6 min | 13% | Conservative — few blind spots |
| **100%** | 241 | 8.8 min | 0% | Exhaustive — checks every historical location |

For comparison, the full `grid` pattern scans 1024 positions (~37 min) with no intelligence about where exchanges actually appear.

**Example**: For Kingdom 10 (most data, 935 historical spawns):

| Coverage | Positions | Scan time |
|---------:|----------:|----------:|
| 70% | 203 | 7.4 min |
| 80% | 249 | 9.1 min |
| 90% | 338 | 12.4 min |
| 100% | 431 | 15.8 min |

At 70% coverage, the scanner checks the 203 densest cells (where 70% of all historical spawns occurred) and skips the remaining 228 cells that account for only 30% of spawns. If the exchange happens to be in a rare spot, the next scan cycle will catch it — and since exchanges respawn frequently, the expected time to detection remains very low.

#### Regenerating the data

To update the compiled-in data with new historical spawns:

```sh
# Update assets/known_locations.csv (format: k,x,y per line)
cd backend
python3 gen_known_locations.py   # regenerates src/known_locations.rs
cargo build --release
```

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

> **Note:** The compiled-in historical data can be kept current by merging newly confirmed exchanges from `exchanges.jsonl` into `backend/assets/known_locations.csv` and regenerating with `python3 gen_known_locations.py`. Raw historical data is archived in `docs/historical-spawns.csv`.
