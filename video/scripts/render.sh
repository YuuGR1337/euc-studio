#!/bin/sh
# The master and the poster (Remotion), then the README files (scripts/finish.mjs).
set -eu
cd "$(dirname "$0")/.."
mkdir -p out
npx remotion render src/index.ts Showcase out/master.mp4
npx remotion still src/index.ts Poster out/poster.png
node scripts/finish.mjs
