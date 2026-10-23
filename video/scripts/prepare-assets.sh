#!/bin/sh
# Copies the studio's number face (JetBrains Mono, OFL, from the app's @fontsource bundle) into
# public/, where Remotion serves it. Run `pnpm install` in app/ first.
set -eu
cd "$(dirname "$0")/.."
FILES=../app/node_modules/@fontsource/jetbrains-mono/files
mkdir -p public/fonts
for weight in 400 500 700; do
  cp "$FILES/jetbrains-mono-latin-$weight-normal.woff2" public/fonts/
done
echo "assets ready: $(ls public/fonts | wc -l | tr -d ' ') faces"
