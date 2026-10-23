#!/bin/sh
# Builds public/audio/music.mp3 from the Mixkit track. The cut stays local (git ignores it):
# Mixkit's terms allow the music inside our video, not a copy of it on its own in the repository
# (public/audio/LICENSES.md). scripts/make.sh runs this when the file is missing.
#
# Track: "It's Love" by Michael Ramir C., Mixkit 834, 110 BPM, Mixkit Stock Music Free License
# (https://mixkit.co/license/#musicFree). Its beat grid runs from 21.850 s (measured for the
# SideMount trailer with an onset script over the decoded PCM), so the cut starts there: frame 0
# of the video is a downbeat, and src/timeline.ts cuts every scene on a beat (BPM = 110).
set -eu
cd "$(dirname "$0")/.."

SRC=scripts/.music-source.mp3
if [ ! -f "$SRC" ]; then
  curl -sSL -o "$SRC" https://assets.mixkit.co/music/834/834.mp3
fi
echo "c69dc74479239f63442a369891a57c7f9a05236a7090e4b694057c13f3843cf9  $SRC" | shasum -a 256 -c -

mkdir -p public/audio
ffmpeg -y -v error -ss 21.850 -t 26.3 -i "$SRC" \
  -af "afade=t=in:d=0.3,afade=t=out:st=23.3:d=3.0" \
  -ar 48000 -ac 2 -c:a libmp3lame -q:a 2 public/audio/music.mp3

ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 public/audio/music.mp3 |
  sed 's/^/music.mp3: /; s/$/ s/'
