# Mediateor ☄️  —  just targets
#
# Install just: cargo install just  or  brew install just
# All cargo commands use an isolated target dir so they don't stomp each other.

export CARGO_TARGET_DIR := "/tmp/mt-just"

# Default: show the menu
default:
    @just --list

# Run the CLI demo on the roommate dispute (the canonical scenario)
demo:
    cargo run -q -p mediator-demo -- scenarios/roommate.json

# Run the CLI demo with the interactive TUI
tui:
    cargo run -q -p mediator-demo -- scenarios/roommate.json --tui

# Start the web front end (expects caches to exist; run `just cache` first)
web:
    @echo "☄  mediator-web → http://127.0.0.1:3000"
    cargo run -q -p mediator-web

# Regenerate all analysis caches (runs the full Isabelle pipeline per scenario)
cache:
    @for f in scenarios/*.json; do \
        case "$$f" in *.analysis.json) continue;; esac; \
        echo "  caching $$f…"; \
        cargo run -q -p mediator-demo -- "$$f" --write-cache; \
    done
    @echo "  ✓ all caches up to date"

# Full verification: build the Isabelle theory + run all cargo tests
verify:
    @echo "── isabelle build ───────────────────────────────────────────"
    isabelle build -D isabelle
    @echo "── cargo test ───────────────────────────────────────────────"
    cargo test --workspace

# Build the demo and web binaries (compile check for the demo path)
build:
    cargo build -p mediator-demo -p mediator-web

# One-command launch (build + cache + web + open browser)
run:
    ./run.sh
