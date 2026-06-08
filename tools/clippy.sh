#!/bin/bash
set -euo pipefail

# Run clippy across the workspace (or a specific crate) with all features,
# all targets (lib + tests + benches + examples), and -D warnings.
#
# Usage:
#   ./tools/clippy.sh              # whole workspace
#   ./tools/clippy.sh nexus-shm    # single crate
#   ./tools/clippy.sh nexus-shm nexus-net  # multiple crates

if [ $# -eq 0 ]; then
    cargo clippy --workspace --all-features --all-targets -- -D warnings
else
    for crate in "$@"; do
        echo "--- $crate ---"
        cargo clippy -p "$crate" --all-features --all-targets -- -D warnings
    done
fi
