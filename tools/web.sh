#!/usr/bin/env bash
# Builds the browser preview into $OUT (default target/web) and optionally serves it.
# Usage: tools/web.sh [--serve PORT] [--debug]
set -euo pipefail
cd "$(dirname "$0")/.."
target="${CARGO_TARGET_DIR:-target}"
out="${OUT:-$target/web}"
profile=web
port=""
while [ $# -gt 0 ]; do
  case "$1" in
    --serve) port="$2"; shift 2 ;;
    --debug) profile=dev; shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
dir=$([ "$profile" = dev ] && echo debug || echo "$profile")
cargo build -p mareader-web --lib --target wasm32-unknown-unknown --profile "$profile"
mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" \
  "$target/wasm32-unknown-unknown/$dir/mareader_web.wasm"
cp apps/mareader-web/index.html "$out/"
echo "built $out"
if [ -n "$port" ]; then
  exec python3 -m http.server "$port" --bind 0.0.0.0 --directory "$out"
fi
