# Changelog

## 0.1.0

First release of subs2srs for Linux, a Rust/GTK4 port of [subs2srs](https://github.com/Ajatt-Tools/subs2srs): create Anki flashcards from videos and subtitles.

### Highlights
- GTK4 interface that follows your GTK theme, with Light, Dark and Follow System modes
- ASS/SSA, SRT, WebVTT and LRC subtitles, plus text subtitle tracks embedded in MKV/MP4/WebM
- Audio clips (Opus/MP3/AAC), snapshots and video clips (MP4/WebM/AVI), made in parallel with ffmpeg
- Preview window to toggle lines, fix typos, check snapshots and play clips before generating
- Filters, time shifts (including per-episode rules), context lines and naming formats compatible with the original
- Dueling subtitles and audio extraction tools
- `subs2srs-cli` for headless use

### Install
**Binary (x86_64, glibc ≥ 2.39, GTK ≥ 4.12):** download `subs2srs-0.1.0-x86_64-linux.tar.gz`, extract it and run `sudo ./install.sh` (or `./install.sh ~/.local`).

**From source:** `git clone https://github.com/tomjfinch/subs2srs-Linux.git && cd subs2srs-Linux && make && sudo make install`. Gentoo users can use the live ebuild in `packaging/gentoo/`.

Requires ffmpeg and ffprobe with libopus, libmp3lame and libx264 (Gentoo: `USE="mp3 opus x264"`).
