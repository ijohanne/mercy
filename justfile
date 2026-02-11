# Mercy - Justfile

default:
    @just --list

# Run both backend and frontend in development mode
dev:
    #!/usr/bin/env bash
    set -euo pipefail
    trap 'echo "Shutting down..."; trap - EXIT; kill 0' SIGINT SIGTERM EXIT

    # Source .env
    if [ -f .env ]; then
        set -a; source .env; set +a
    else
        echo "Error: .env not found. Copy .env.example to .env and configure it."
        exit 1
    fi

    echo "Starting backend..."
    (cd backend && cargo run --release) &
    BACKEND_PID=$!

    echo "Starting frontend..."
    (cd frontend && bun run dev) &
    FRONTEND_PID=$!

    echo "Backend: http://127.0.0.1:${MERCY_LISTEN_ADDR##*:}"
    echo "Frontend: http://localhost:3000"
    echo "Press Ctrl+C to stop."

    wait $BACKEND_PID $FRONTEND_PID

# Run backend only
backend:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -f .env ]; then set -a; source .env; set +a; fi
    cd backend && cargo run --release

# Run frontend only
frontend:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -f .env ]; then set -a; source .env; set +a; fi
    cd frontend && bun run dev

# Build both packages
build:
    cd backend && cargo build --release
    cd frontend && bun run build

# Install frontend dependencies
install:
    cd frontend && bun install

# Stop dev processes
stop:
    #!/usr/bin/env bash
    lsof -ti:8090 2>/dev/null | xargs kill 2>/dev/null && echo "Backend stopped." || echo "No backend running."
    lsof -ti:3000 2>/dev/null | xargs kill 2>/dev/null && echo "Frontend stopped." || echo "No frontend running."

# Check backend compiles
check:
    cd backend && cargo check

# Format backend code
fmt:
    cd backend && cargo fmt

# Clean build artifacts
clean:
    cd backend && cargo clean
    cd frontend && rm -rf .next node_modules
