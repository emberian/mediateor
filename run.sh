#!/usr/bin/env bash
# run.sh — one-command launcher for Mediateor ☄️
#
# Builds the workspace, generates any missing analysis caches
# (skipping those already newer than their scenario), then
# starts the web server and opens it in the browser.
#
# Usage:
#   ./run.sh            # normal launch
#   ./run.sh --no-open  # skip opening the browser

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/mt-run}"
export CARGO_TARGET_DIR

NO_OPEN=0
for arg in "$@"; do
  [[ "$arg" == "--no-open" ]] && NO_OPEN=1
done

cd "$REPO"

# ── 1. Build the demo and web binaries ───────────────────────────────────
echo "☄  building…"
cargo build -q -p mediator-demo -p mediator-web

# ── 2. Ensure analysis caches are fresh ──────────────────────────────────
# For each scenarios/*.json, regenerate <name>.analysis.json if it is
# missing or older than the scenario file.
shopt -s nullglob
for scenario in "$REPO"/scenarios/*.json; do
  # Skip already-generated cache files (they end in .analysis.json)
  [[ "$scenario" == *.analysis.json ]] && continue

  cache="${scenario%.json}.analysis.json"

  if [[ -f "$cache" && "$cache" -nt "$scenario" ]]; then
    echo "  ✓ cache fresh: $(basename "$cache")"
    continue
  fi

  echo "  ⏳ generating cache for $(basename "$scenario")…"
  echo "     (Isabelle may take 30-60 s on first run)"
  cargo run -q -p mediator-demo -- "$scenario" --write-cache
  echo "  ✓ cached: $(basename "$cache")"
done

# ── 3. Launch the web server ─────────────────────────────────────────────
URL="http://127.0.0.1:3000"
echo ""
echo "☄  starting mediator-web on $URL"
echo "   operator cockpit → $URL/operator"
echo "   Robin's view     → $URL/party/robin"
echo "   Sam's view       → $URL/party/sam"
echo ""

if [[ "$NO_OPEN" -eq 0 ]] && command -v open &>/dev/null; then
  # Give the server a moment to bind, then open in the background.
  (sleep 1 && open "$URL") &
fi

exec cargo run -q -p mediator-web
