# Mercy

Mercenary Exchange Locator Service -- a Rust server that automates finding special tiles in the Total Battle browser game using headless Chromium and template matching. Results are exposed via a REST API.

## Building

### Nix (recommended)

```sh
nix build
```

The output includes the binary at `result/bin/mercy` and reference images at `result/share/mercy/assets/`.

### Cargo

```sh
cargo build --release
```

## Development

There is a `.envrc` in the repo root which allows [direnv](https://direnv.net/) to automatically load the Nix dev shell. If you use direnv:

```sh
direnv allow
```

Otherwise, enter the shell manually:

```sh
nix develop
```

This provides the Rust toolchain, pkg-config, OpenSSL, and Chromium (on Linux).

### Running locally

Set the required environment variables:

| Variable | Required | Description |
|----------|----------|-------------|
| `MERCY_KINGDOMS` | yes | Comma-separated kingdom IDs (e.g. `109,110,112`) |
| `MERCY_AUTH_TOKEN` | yes | Bearer token for API authentication |
| `MERCY_TB_EMAIL` | yes | Total Battle login email |
| `MERCY_TB_PASSWORD` | yes | Total Battle login password |
| `MERCY_LISTEN_ADDR` | no | Listen address (default `0.0.0.0:8090`) |
| `MERCY_CHROMIUM_PATH` | no | Path to Chromium binary (auto-detected if unset) |
| `MERCY_HEADLESS` | no | `true` or `1` for headless mode (default `false`, use `xvfb-run` on servers) |
| `MERCY_SEARCH_TARGET` | no | Building name to search for (default `Mercenary Exchange`) |

Then run:

```sh
cargo run
```

### Debugging inside `nix develop`

Since the game requires a browser with WebGL, local debugging works best with a visible browser (headless mode off, which is the default). Inside `nix develop`:

1. Set your env vars (or use a `.env` file with a tool like `dotenv`):
   ```sh
   export MERCY_KINGDOMS=111
   export MERCY_AUTH_TOKEN=dev
   export MERCY_TB_EMAIL=you@example.com
   export MERCY_TB_PASSWORD=hunter2
   ```

2. Run the server:
   ```sh
   cargo run
   ```

3. Use the API to control the browser:
   ```sh
   # Prepare browser + login without scanning
   curl -X POST -H "Authorization: Bearer dev" localhost:8090/prepare

   # Navigate to specific coordinates and get a screenshot
   curl -H "Authorization: Bearer dev" "localhost:8090/goto?k=111&x=512&y=512" -o screenshot.png

   # Take a screenshot of the current view
   curl -H "Authorization: Bearer dev" localhost:8090/screenshot -o screenshot.png

   # Start scanning
   curl -X POST -H "Authorization: Bearer dev" localhost:8090/start

   # Check status
   curl -H "Authorization: Bearer dev" localhost:8090/status

   # View found exchanges
   curl -H "Authorization: Bearer dev" localhost:8090/exchanges
   ```

On a headless Linux server, wrap with `xvfb-run`:

```sh
xvfb-run -s '-screen 0 1920x1080x24' cargo run
```

## NixOS deployment

Import the module and configure the service:

```nix
{
  imports = [ mercy.nixosModules.default ];

  services.mercy = {
    enable = true;
    package = mercy.packages.x86_64-linux.default;
    kingdoms = "109,110,112,113,114";
    listenPort = 8090;
    authTokenFile = "/run/secrets/mercy-auth-token";
    tbEmailFile = "/run/secrets/mercy-email";
    tbPasswordFile = "/run/secrets/mercy-password";
    # searchTarget = "Mercenary Exchange";  # default
  };
}
```

Secrets are read from files at service start (not baked into the Nix store). The service runs under `DynamicUser` with security hardening. `xvfb-run` provides a virtual display for Chromium's WebGL.
