#!/usr/bin/env bash
# Regenerate the Rust client bindings for the TUI from the SpacetimeDB
# server module. This is a LOCAL codegen step (no server state is mutated).
#
# Usage:
#   ./scripts/gen-bindings.sh                # use cached WASM if present
#   ./scripts/gen-bindings.sh --build        # rebuild the WASM first
#   ./scripts/gen-bindings.sh --module <name>
#
# The script tries the prebuilt WASM in
#   sync/spacetimedb/target/wasm32-unknown-unknown/release/spacenix.wasm
# first (fast, no SpacetimeDB server required). If missing or `--build` is
# passed, it runs `spacetime build` to produce it.

set -euo pipefail

SCRIPT_PATH="$(readlink -f "${BASH_SOURCE[0]}")"
# Script lives at <repo>/tui/scripts/gen-bindings.sh, so two levels up is
# the workspace root.
REPO_ROOT="$(cd -- "$(dirname -- "$SCRIPT_PATH")/../.." && pwd)"
TUI_DIR="$REPO_ROOT/tui"
SYNC_DIR="$REPO_ROOT/sync"
SPACETIMEDB_DIR="$SYNC_DIR/spacetimedb"
LOCAL_JSON="$SYNC_DIR/spacetime.local.json"
WASM="$SPACETIMEDB_DIR/target/wasm32-unknown-unknown/release/spacenix.wasm"

MODULE=""
DO_BUILD=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --build) DO_BUILD=1; shift ;;
    --module) MODULE="$2"; shift 2 ;;
    *) echo "unknown flag: $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$MODULE" && -f "$LOCAL_JSON" ]]; then
  MODULE="$(jq -r '.database // .module // empty' "$LOCAL_JSON" 2>/dev/null || true)"
fi

if [[ $DO_BUILD -eq 1 || ! -f "$WASM" ]]; then
  echo "Building STDB module WASM…"
  spacetime build -p "$SPACETIMEDB_DIR"
fi

if [[ ! -f "$WASM" ]]; then
  echo "Could not find $WASM after build. Did `spacetime build` succeed?" >&2
  exit 1
fi

OUT_DIR="$TUI_DIR/module_bindings/src"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Generating Rust bindings from $WASM…"
spacetime generate \
  --lang rust \
  --out-dir "$TMP_DIR" \
  --bin-path "$WASM" \
  --yes

mkdir -p "$OUT_DIR"
# Clear the previous contents so a removed reducer/procedure from the server
# is also removed locally.
find "$OUT_DIR" -mindepth 1 -delete
cp -R "$TMP_DIR/." "$OUT_DIR/"

# The codegen writes `mod.rs`; rename it to `lib.rs` for the crate to be
# usable as a library, and prepend a hand-written header.
if [[ -f "$OUT_DIR/mod.rs" ]]; then
  TMP_HEAD="$(mktemp)"
  {
    cat <<'EOF'
//! Auto-generated SpacetimeDB client bindings for the `spacenix` module.
//!
//! Regenerate with `./scripts/gen-bindings.sh`.
//!
//! The body of this file is produced by `spacetime generate`. The header
//! comment above is the only line maintained by hand.

#![allow(unused, clippy::all, non_camel_case_types, non_snake_case)]

EOF
    cat "$OUT_DIR/mod.rs"
  } > "$TMP_HEAD"
  mv "$TMP_HEAD" "$OUT_DIR/lib.rs"
  rm "$OUT_DIR/mod.rs"
fi

echo "Bindings written to $OUT_DIR (module: ${MODULE:-unknown})"
