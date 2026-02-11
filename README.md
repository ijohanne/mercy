# Mercy

Mercenary Exchange Locator Service -- a Rust backend that automates finding special tiles in the Total Battle browser game using headless Chromium and template matching, with a Next.js admin interface for browser-based control.

## Quick Start

```sh
cp .env.example .env
# Edit .env with your credentials
just dev
```

This starts both the backend (port 8090) and frontend (port 3000). Open http://localhost:3000 and log in with the admin credentials from `.env`.

## Project Structure

```
mercy/
  backend/        Rust backend (axum REST API, chromiumoxide, template matching)
  frontend/       Next.js admin interface (React, Tailwind, shadcn/ui)
  nix/            NixOS module
  flake.nix       Nix flake (builds both packages)
  justfile        Development commands
  .env.example    Environment variable template
```

## Environment Variables

### Backend

| Variable | Required | Description |
|----------|----------|-------------|
| `MERCY_KINGDOMS` | yes | Comma-separated kingdom IDs (e.g. `109,110,112`) |
| `MERCY_AUTH_TOKEN` | yes | Bearer token for API authentication |
| `MERCY_TB_EMAIL` | yes | Total Battle login email |
| `MERCY_TB_PASSWORD` | yes | Total Battle login password |
| `MERCY_LISTEN_ADDR` | no | Listen address (default `0.0.0.0:8090`) |
| `MERCY_CHROMIUM_PATH` | no | Path to Chromium binary |
| `MERCY_HEADLESS` | no | `true` for headless mode |
| `MERCY_SEARCH_TARGET` | no | Building name to search for (default `Mercenary Exchange`). Maps to reference image: lowercased, spaces → `_`, plus `_ref.png` (e.g. `"Test Building"` → `test_building_ref.png`). **Quote values with spaces.** |

### Frontend

| Variable | Required | Description |
|----------|----------|-------------|
| `MERCY_ADMIN_USER` | no | Admin username (default `admin`) |
| `MERCY_ADMIN_PASSWORD` | yes | Admin password |
| `MERCY_BACKEND_URL` | no | Backend URL (default `http://127.0.0.1:8090`) |
| `MERCY_SESSION_SECRET` | yes | Cookie signing secret |

### Platform Notes

**macOS:** Set `MERCY_HEADLESS=true` and `MERCY_CHROMIUM_PATH` to your Chrome path:
```sh
MERCY_CHROMIUM_PATH="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
MERCY_HEADLESS=true
```

**Linux (desktop):** Leave `MERCY_HEADLESS` unset to see the browser window.

**Linux (headless server):** Use `xvfb-run` or set `MERCY_HEADLESS=true`.

## Development

### Prerequisites

Enter the Nix dev shell (provides Rust, bun, just, and dependencies):
```sh
direnv allow    # or: nix develop
```

Install frontend dependencies:
```sh
just install
```

### Commands

| Command | Description |
|---------|-------------|
| `dev` or `just dev` | Run both backend and frontend |
| `just backend` | Run backend only |
| `just frontend` | Run frontend only |
| `just build` | Build both for release |
| `just install` | Install frontend dependencies |
| `just check` | Check backend compiles |
| `just fmt` | Format backend code |
| `just stop` | Kill dev processes on ports 8090/3000 |
| `just clean` | Clean all build artifacts |

## Building

### Nix

```sh
nix build .#mercy-backend    # Rust backend
nix build .#mercy-frontend   # Next.js frontend
nix build                    # Default (backend)
```

### Manual

```sh
cd backend && cargo build --release
cd frontend && bun run build
```

## Backend API

All endpoints require `Authorization: Bearer <token>`.

| Method | Path | Description |
|--------|------|-------------|
| POST | `/prepare` | Launch browser and log in |
| POST | `/start` | Start scanning (or resume if paused) |
| POST | `/stop` | Stop scanning |
| POST | `/pause` | Pause scanning |
| POST | `/logout` | Kill browser session |
| GET | `/status` | Current phase, kingdom, exchange count |
| GET | `/exchanges` | List of found exchanges |
| GET | `/screenshot` | PNG screenshot of current browser view |
| GET | `/goto?k=&x=&y=` | Navigate to coordinates, return screenshot |

## NixOS Deployment

```nix
{
  imports = [ mercy.nixosModules.default ];

  services.mercy = {
    enable = true;
    backendPackage = mercy.packages.x86_64-linux.mercy-backend;
    frontendPackage = mercy.packages.x86_64-linux.mercy-frontend;
    kingdoms = "109,110,112,113,114";
    backendPort = 8090;
    frontendPort = 3000;

    # Shared secret (used by both backend and frontend)
    authTokenFile = "/run/secrets/mercy-auth-token";

    # Backend secrets
    tbEmailFile = "/run/secrets/mercy-email";
    tbPasswordFile = "/run/secrets/mercy-password";

    # Frontend secrets
    adminUserFile = "/run/secrets/mercy-admin-user";
    adminPasswordFile = "/run/secrets/mercy-admin-password";
    sessionSecretFile = "/run/secrets/mercy-session-secret";

    # Optional: nginx reverse proxy
    domain = "mercy.example.com";
    nginx.enableSSL = true;
  };
}
```

This creates two systemd services (`mercy-backend` and `mercy-frontend`) with security hardening. When `domain` is set, an nginx virtual host proxies traffic to the frontend. Secrets are read from files at service start.
