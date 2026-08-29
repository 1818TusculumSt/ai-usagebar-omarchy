#!/usr/bin/env bash
# Release build of the CLI binaries — everything else in this repo (the
# plugin deploy, the AUR packages, the frontends) consumes these.
#
#   ./scripts/build.sh            cargo build --release
#   ./scripts/build.sh <args>     extra flags pass through to cargo
#
# Same build as `make build` / `scripts/install.sh` step 0; kept separate so
# a plain "compile it" needs no deploy side effects.
set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)

cargo build --release --manifest-path "$REPO/Cargo.toml" "$@"

for bin in ai-usagebar-omarchy ai-usagebar-omarchy-tui; do
  path="$REPO/target/release/$bin"
  [[ -x $path ]] || { echo "expected binary missing: $path" >&2; exit 1; }
  size=$(du -h -- "$path" | cut -f1)
  echo "$path  ($size)"
done
