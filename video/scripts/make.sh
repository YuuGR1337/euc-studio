#!/bin/sh
# The showcase from scratch (video/README.md): record the runs, film them in the studio, cut,
# encode. `sh video/scripts/make.sh [record|capture|footage|render]` starts at that step.
# Needs the cached stock firmware, the MuJoCo library, our compat build (firmware/build) for the
# comparison, `pnpm install` in app/ and `npm install` in video/.
set -eu
video="$(cd "$(dirname "$0")/.." && pwd)"
repo="$(cd "$video/.." && pwd)"
from="${1:-record}"
step() {
  case "$from" in
    record) return 0 ;;
    capture) [ "$1" != record ] ;;
    footage) [ "$1" = footage ] || [ "$1" = render ] ;;
    render) [ "$1" = render ] ;;
    *) echo "unknown step $from" >&2 && exit 2 ;;
  esac
}
mkdir -p "$video/out/runs" "$video/out/frames"

if step record; then
  # The real engine: the runs, their sound, their worlds and the comparison's stream.
  export MUJOCO_DYNAMIC_LINK_DIR="${MUJOCO_DYNAMIC_LINK_DIR:-$repo/.cache/mujoco/mujoco-3.9.0/lib}"
  export DYLD_LIBRARY_PATH="$MUJOCO_DYNAMIC_LINK_DIR"
  EUC_SHOWCASE="$video/shots.json" EUC_SHOWCASE_OUT="$video/out/runs" CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-3}" \
    cargo test --release --manifest-path "$repo/app/src-tauri/Cargo.toml" --lib engine::showcase -- --ignored --nocapture
fi
if step capture; then
  # The studio in headless WebKit, one shot at a time.
  node "$video/capture/capture.mjs" "$repo/app" "$video/out/runs" "$video/out/frames" "$video/shots.json"
fi
if step footage; then
  sh "$video/scripts/prepare-assets.sh"
  node "$video/scripts/footage.mjs"
fi
if step render; then
  # The music cut is built locally from the Mixkit track (pinned by its SHA-256), never committed.
  [ -f "$video/public/audio/music.mp3" ] || sh "$video/scripts/make-music.sh"
  sh "$video/scripts/render.sh"
fi
