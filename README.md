# bragi-rs

- Terminal-based, music browser and player.
- Vim motions.
- Very early in development.

### Keybinds

- `j`: move down.
- `k`: move up.
- `/`: enter search mode.
- `gg`: move to the top of the playlist .
- `G`: move to the bottom of the playlist.
- `p`: pause / play the song.
- `Esc`:
  - if in search mode: break out of search mode, return to playlist.
  - if in normal mode: move back to base playlist.
- `Enter`:
  - if in search mode: Move into playlist with songs selected by search buffer.
  - if in normal mode: Open an album, or play a song.
