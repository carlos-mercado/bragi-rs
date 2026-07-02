# bragi-rs

- Terminal-based, music browser and player.
- Vim motions.

### Keybinds

- `j`: move down.
- `k`: move up.
- `gg`: move to the top of the selection space.
- `G`: move to the bottom of the selection space.
- `/`: enter search mode.
- `p`: pause / play the song.
- `h`: previous song.
- `l`: next song.
- `Esc`:
  - if in search mode: break out of search mode, return to playlist.
  - if in normal mode: move back to base playlist.
- `Enter`:
  - if in search mode: Move into playlist with songs selected by search buffer.
  - if in normal mode: Open an album, or play a song.

### Disclaimers
- Very early in development.
- No `ALAC` support.
