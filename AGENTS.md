# AGENTS.md - Guidance for AI Agents Working on Mercy

## Project Overview

Mercy is a Rust server service that automates finding "Mercenary Exchange" tiles in the Total Battle browser game. It runs a headless Chromium browser, logs in, navigates the map, and scans specified kingdoms using template matching. Results are exposed via authenticated REST API.

## Architecture

- `src/config.rs` - Configuration from environment variables
- `src/state.rs` - Shared state types (`AppState = Arc<Mutex<AppStateInner>>`)
- `src/api.rs` - Axum REST endpoints with bearer token auth
- `src/browser.rs` - Chromium automation via chromiumoxide (CDP)
- `src/detector.rs` - Template matching with imageproc
- `src/scanner.rs` - Spiral scanning orchestrator
- `src/main.rs` - Entry point wiring API server + scanner
- `nix/module.nix` - NixOS service module
- `flake.nix` - Nix flake for building + dev shell

## Coding Conventions

### Error Handling
- `thiserror` for domain-specific error enums (e.g., `BrowserError`)
- `anyhow` in `main.rs` and top-level orchestration for contextual error propagation
- Use `.context("descriptive message")` when propagating errors
- No `.unwrap()` except in truly impossible cases with a justifying comment
- Log errors with `tracing::error!` before returning where appropriate

### Memory Management
- Never `.clone()` image data (`Vec<u8>`, `DynamicImage`). Instead:
  - Pass by reference (`&[u8]`, `&DynamicImage`) for read-only access
  - Move ownership when transferring to a new owner
  - Use `Arc<DynamicImage>` for shared read-only access across tasks
- Reference images are loaded once at startup as `Arc<DynamicImage>`, shared via `Arc::clone()`
- Screenshots: decode once into `DynamicImage`, pass by reference to template matching, then drop

### Async Patterns
- The scanner runs as a spawned tokio task, communicating via `Arc<Mutex<AppStateInner>>`
- Hold locks for the shortest time possible - clone what you need and drop the lock
- The browser event handler runs in its own spawned task
- Use `tokio::time::sleep` for delays, not `std::thread::sleep`

### Browser Automation Gotchas
- CDP mouse events need both `MousePressed` and `MouseReleased` for a proper click
- `navigator.webdriver` must be overridden via `AddScriptToEvaluateOnNewDocument` to avoid detection
- The user agent must not contain "HeadlessChrome"
- Game popups need time to appear - always sleep after clicking before reading popup text
- Escape key is the most reliable way to dismiss popups
- JavaScript `element.click()` doesn't always work in game canvases; prefer CDP `Input.dispatchMouseEvent`
- The `Browser` object must be kept alive (stored as `_browser` field) even if not directly used, otherwise the browser process terminates

### Template Matching
- Use `CrossCorrelationNormalized` method - produces scores in [0, 1] range
- Threshold of 0.6 works for initial detection, but always click-to-confirm
- Deduplicate nearby matches (within 20px) to avoid clicking the same tile multiple times
- Reference images must be smaller than the screenshot

## NixOS Deployment
- The service uses `xvfb-run` to provide a virtual X display (needed for WebGL)
- Secret files (auth token, email, password) are read by a wrapper script and exported as env vars
- `MemoryDenyWriteExecute = false` is required because Chromium uses JIT
- Assets are installed to `$out/share/mercy/assets/` by the Nix build

## Testing
- `cargo test` runs unit tests (coordinate parsing, spiral generation)
- Integration testing requires a running Chromium and actual game credentials
- `cargo clippy` should pass with no warnings

## Project Conventions
- `plans/` and `issues/` directories are gitignored (never checked in)
- No documentation files beyond this AGENTS.md unless explicitly requested
