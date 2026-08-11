use music::{Album, TrackDetails};

// Clamp the cursor after moving "down" a list of `len` items.
// Never moves past the last valid index; a `len` of 0 stays at 0.
pub fn clamp_cursor_increment(cursor: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    std::cmp::min(len - 1, cursor + 1)
}

// Clamp the cursor after moving "up" a list. Never goes below 0.
pub fn clamp_cursor_decrement(cursor: usize) -> usize {
    cursor.saturating_sub(1)
}

// Returns the next playlist index, or `None` if `current` is already
// at (or past) the last playable index.
pub fn next_index(current: usize, len: usize) -> Option<usize> {
    if len == 0 || current + 1 >= len {
        return None;
    }
    Some(current + 1)
}

// Returns the previous playlist index, or `None` if `current` is
// already at index 0.
pub fn prev_index(current: usize) -> Option<usize> {
    if current == 0 {
        None
    } else {
        Some(current - 1)
    }
}

// Parses a user-entered command buffer of the form "<cmd> <arg>" into
// its two whitespace-separated parts. Returns `None` if the input isn't
// exactly two space-separated tokens.
pub fn parse_command_args(input: &str) -> Option<(&str, &str)> {
    let args: Vec<&str> = input.split(' ').collect();
    if args.len() != 2 {
        return None;
    }
    Some((args[0], args[1]))
}

// Finds the track matching `song_path` in `all_songs` and the
// corresponding track nested inside `albums` (via its parent album's
// artist/album match), and applies `update` to both, mutating them in
// place. No-ops (silently) if either lookup fails.
pub fn with_matching_song_and_album_song<F>(
    all_songs: &mut [TrackDetails],
    albums: &mut [Album],
    song_path: &str,
    update: F,
) where
    F: Fn(&mut TrackDetails),
{
    let Some(all_songs_ref) = all_songs.iter_mut().find(|s| s.song_path == song_path) else {
        return;
    };
    update(all_songs_ref);

    let (album_name, artist_name) = (all_songs_ref.album.clone(), all_songs_ref.artist.clone());

    let Some(album_song_ref) = albums
        .iter_mut()
        .find(|alb| alb.album == album_name && alb.artist == artist_name)
        .and_then(|alb| alb.songs.iter_mut().find(|s| s.song_path == song_path))
    else {
        return;
    };
    update(album_song_ref);
}

pub fn apply_duration_played(
    all_songs: &mut [TrackDetails],
    albums: &mut [Album],
    song_path: &str,
    time: u64,
) {
    with_matching_song_and_album_song(all_songs, albums, song_path, |s| s.stats.1 += time);
}

pub fn apply_last_played(
    all_songs: &mut [TrackDetails],
    albums: &mut [Album],
    song_path: &str,
    timestamp: u64,
) {
    with_matching_song_and_album_song(all_songs, albums, song_path, |s| s.stats.0 = timestamp);
}

pub fn apply_wipe_labels(all_songs: &mut [TrackDetails], albums: &mut [Album], song_path: &str) {
    with_matching_song_and_album_song(all_songs, albums, song_path, |s| s.tags.clear());
}

pub fn apply_add_label(
    all_songs: &mut [TrackDetails],
    albums: &mut [Album],
    song_path: &str,
    label: &str,
) {
    with_matching_song_and_album_song(all_songs, albums, song_path, |s| {
        s.tags.push(label.to_string())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(artist: &str, album: &str, title: &str, song_path: &str) -> TrackDetails {
        TrackDetails {
            artist: artist.to_string(),
            album: album.to_string(),
            title: title.to_string(),
            track_no: 1,
            date: "2020".to_string(),
            song_path: song_path.to_string(),
            duration: 180,
            stats: (0, 0, 0),
            tags: Vec::new(),
        }
    }

    fn album(artist: &str, name: &str, songs: Vec<TrackDetails>) -> Album {
        Album::new(
            artist.to_string(),
            name.to_string(),
            "2020".to_string(),
            songs,
            (0, 0, 0),
        )
    }

    // ---- clamp_cursor_increment / clamp_cursor_decrement ----

    #[test]
    fn clamp_increment_moves_cursor_forward() {
        assert_eq!(clamp_cursor_increment(0, 5), 1);
        assert_eq!(clamp_cursor_increment(3, 5), 4);
    }

    #[test]
    fn clamp_increment_stops_at_last_index() {
        assert_eq!(clamp_cursor_increment(4, 5), 4);
    }

    #[test]
    fn clamp_increment_empty_list_stays_zero() {
        assert_eq!(clamp_cursor_increment(0, 0), 0);
    }

    #[test]
    fn clamp_decrement_moves_cursor_back() {
        assert_eq!(clamp_cursor_decrement(3), 2);
    }

    #[test]
    fn clamp_decrement_stops_at_zero() {
        assert_eq!(clamp_cursor_decrement(0), 0);
    }

    // ---- next_index / prev_index ----

    #[test]
    fn next_index_advances_within_bounds() {
        assert_eq!(next_index(0, 3), Some(1));
        assert_eq!(next_index(1, 3), Some(2));
    }

    #[test]
    fn next_index_none_at_last_song() {
        assert_eq!(next_index(2, 3), None);
    }

    #[test]
    fn next_index_none_on_empty_queue() {
        assert_eq!(next_index(0, 0), None);
    }

    #[test]
    fn prev_index_moves_back_within_bounds() {
        assert_eq!(prev_index(2), Some(1));
        assert_eq!(prev_index(1), Some(0));
    }

    #[test]
    fn prev_index_none_at_first_song() {
        assert_eq!(prev_index(0), None);
    }

    // ---- parse_command_args ----

    #[test]
    fn parse_command_args_valid_input() {
        assert_eq!(
            parse_command_args("sort last_played"),
            Some(("sort", "last_played"))
        );
    }

    #[test]
    fn parse_command_args_rejects_wrong_arg_count() {
        assert_eq!(parse_command_args("sort"), None);
        assert_eq!(parse_command_args("sort last_played extra"), None);
        assert_eq!(parse_command_args(""), None);
    }

    // ---- apply_duration_played / apply_last_played / apply_wipe_labels / apply_add_label ----

    #[test]
    fn apply_duration_played_updates_song_and_album_copy() {
        let mut all_songs = vec![track("Radiohead", "OK Computer", "Karma Police", "/a.mp3")];
        let mut albums = vec![album(
            "Radiohead",
            "OK Computer",
            vec![track("Radiohead", "OK Computer", "Karma Police", "/a.mp3")],
        )];

        apply_duration_played(&mut all_songs, &mut albums, "/a.mp3", 42);

        assert_eq!(all_songs[0].stats.1, 42);
        assert_eq!(albums[0].songs[0].stats.1, 42);
    }

    #[test]
    fn apply_duration_played_missing_song_is_noop() {
        let mut all_songs = vec![track("Radiohead", "OK Computer", "Karma Police", "/a.mp3")];
        let mut albums = vec![album(
            "Radiohead",
            "OK Computer",
            vec![track("Radiohead", "OK Computer", "Karma Police", "/a.mp3")],
        )];

        apply_duration_played(&mut all_songs, &mut albums, "/does-not-exist.mp3", 42);

        assert_eq!(all_songs[0].stats.1, 0);
        assert_eq!(albums[0].songs[0].stats.1, 0);
    }

    #[test]
    fn apply_last_played_updates_timestamp_in_both_places() {
        let mut all_songs = vec![track("Radiohead", "OK Computer", "Karma Police", "/a.mp3")];
        let mut albums = vec![album(
            "Radiohead",
            "OK Computer",
            vec![track("Radiohead", "OK Computer", "Karma Police", "/a.mp3")],
        )];

        apply_last_played(&mut all_songs, &mut albums, "/a.mp3", 12345);

        assert_eq!(all_songs[0].stats.0, 12345);
        assert_eq!(albums[0].songs[0].stats.0, 12345);
    }

    #[test]
    fn apply_wipe_labels_clears_tags_in_both_places() {
        let mut song = track("Radiohead", "OK Computer", "Karma Police", "/a.mp3");
        song.tags = vec!["favorite".to_string(), "rock".to_string()];
        let mut album_song = song.clone();

        let mut all_songs = vec![song];
        let mut albums = vec![album("Radiohead", "OK Computer", vec![album_song.clone()])];
        albums[0].songs[0].tags = vec!["favorite".to_string(), "rock".to_string()];
        album_song.tags.clear();

        apply_wipe_labels(&mut all_songs, &mut albums, "/a.mp3");

        assert!(all_songs[0].tags.is_empty());
        assert!(albums[0].songs[0].tags.is_empty());
    }

    #[test]
    fn apply_add_label_pushes_tag_in_both_places() {
        let mut all_songs = vec![track("Radiohead", "OK Computer", "Karma Police", "/a.mp3")];
        let mut albums = vec![album(
            "Radiohead",
            "OK Computer",
            vec![track("Radiohead", "OK Computer", "Karma Police", "/a.mp3")],
        )];

        apply_add_label(&mut all_songs, &mut albums, "/a.mp3", "favorite");

        assert_eq!(all_songs[0].tags, vec!["favorite".to_string()]);
        assert_eq!(albums[0].songs[0].tags, vec!["favorite".to_string()]);
    }
}
