# Audio licences

The showcase's music comes from Mixkit (https://mixkit.co). Mixkit music is free for commercial
and personal projects and needs no attribution; the licence is
https://mixkit.co/license/#musicFree. Every other sound in the video is the firmware's own
buzzer, recorded from the runs (`app/src-tauri/src/engine/showcase.rs`).

| File        | Title                     | Artist           | Source                                     | Licence                         |
| ----------- | ------------------------- | ---------------- | ------------------------------------------ | ------------------------------- |
| `music.mp3` | It's Love (110 BPM, 1:37) | Michael Ramir C. | https://assets.mixkit.co/music/834/834.mp3 | Mixkit Stock Music Free License |

`music.mp3` is not in the repository: Mixkit's terms allow the music inside our video, not a
copy of it on its own. `scripts/make-music.sh` (run by `scripts/make.sh` when the file is
missing) downloads the track, checks its SHA-256 and cuts it: from 21.850 s (the first downbeat of
its beat grid), 26.3 s long (the video's 48 beats), faded in over 0.3 s and out over the last 3 s.
The committed `docs/assets/video/showcase.mp4` carries the music mixed into the video.
