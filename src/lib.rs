use lofty::file::TaggedFile;
use lofty::{prelude::*, read_from_path};
use ratatui::prelude::Text;
use redb::Database;
use std::cmp::Ord;
use std::collections::HashMap;
use std::fs::{self, DirEntry};
use std::hash::Hash;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub mod db;
use crate::db::*;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Album {
    pub artist: String,
    pub album: String,
    selected: bool,
    pub date: String,
    pub songs: Vec<TrackDetails>,
    pub stats: (u64, u64, u64),
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct TrackDetails {
    pub artist: String,
    pub album: String,
    pub track_no: u32,
    pub title: String,
    pub date: String,
    pub song_path: String,
    pub duration: u64,
    //         (last_played, duration_played, date_added)
    pub stats: (u64, u64, u64),
    pub tags: Vec<String>,
}

impl From<&Album> for Text<'static> {
    fn from(album: &Album) -> Self {
        Text::from(format!(
            "{} - {} [{}]",
            album.artist, album.album, album.date,
        ))
    }
}

impl From<TrackDetails> for Text<'static> {
    fn from(track: TrackDetails) -> Self {
        Text::from(format!(
            "{}\n{}\n{} [Track {}] {:?}",
            track.artist, track.title, track.album, track.track_no, track.tags
        ))
    }
}

impl From<&TrackDetails> for Text<'static> {
    fn from(track: &TrackDetails) -> Self {
        Text::from(format!(
            "{} - {} ({}) [Track {}] {:?}", //  (last_played: {}, duration_played: {}, date_added: {})",
            track.artist, track.title, track.date, track.track_no, track.tags,
        ))
    }
}

// for all tracks in a library, put all songs that belong in the same
// album, in the same album, and collect those albums into one
// vector and return
pub fn build_albums(tracks: &Vec<TrackDetails>) -> Vec<Album> {
    // (artist, album) => album_stats
    let mut album_to_stats: HashMap<(&String, &String), (u64, u64, u64)> = HashMap::new();

    // (artist, album) => album_songs
    let mut album_to_songs: HashMap<(&String, &String), Vec<TrackDetails>> = HashMap::new();

    for track in tracks {
        album_to_songs
            .entry((&track.artist, &track.album))
            .or_default()
            .push(track.clone());

        let album_stats = album_to_stats
            .entry((&track.artist, &track.album))
            .or_insert((0, 0, 0));

        let last_played = std::cmp::max(album_stats.0, track.stats.0);
        let duration_played = album_stats.1 + track.stats.1;
        let date_added = std::cmp::max(album_stats.2, track.stats.2);
        *album_stats = (last_played, duration_played, date_added);
    }

    let mut albums = Vec::new();
    for ((artist, album), mut songs) in album_to_songs {
        songs.sort();
        albums.push(Album {
            artist: artist.to_string(),
            album: album.to_string(),
            date: songs
                .first()
                .map_or("Unknown Date".to_string(), |d| d.date.clone()),
            selected: false,
            stats: album_to_stats[&(artist, album)],
            songs,
        });
    }

    albums.sort();
    albums
}

// Filter a list of tracks by a query string.
// Matches case-insensitively against artist, album, and title.
// Returns a new Vec containing only the matching tracks.
pub fn filter_tracks(tracks: &[TrackDetails], query: &str) -> Vec<TrackDetails> {
    let q = query.to_lowercase();
    tracks
        .iter()
        .filter(|t| {
            t.artist.to_lowercase().contains(&q)
                || t.album.to_lowercase().contains(&q)
                || t.title.to_lowercase().contains(&q)
                || t.tags.contains(&q)
        })
        .cloned()
        .collect()
}

// TODO
// this is recusive dfs with no cycle checks
// idk if cylces are possible to create in file systems
// maybe with symlinks, but i don't wan't to deal
// with that right now

// dfs through all directories in music library
// and extract all music files
pub fn get_music_files(
    path: &Path,
    songs: &mut Vec<TrackDetails>,
    db: Option<&Database>,
) -> io::Result<()> {
    let it: fs::ReadDir = fs::read_dir(path)?;

    for entry in it {
        let entry: DirEntry = entry?;

        if entry.metadata()?.is_dir() {
            get_music_files(&entry.path(), songs, db)?;
        } else {
            let path: PathBuf = entry.path();
            let file_type = path.extension().and_then(|e| e.to_str()).unwrap();

            let is_audio = matches!(
                file_type.to_lowercase().as_str(),
                "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "aiff"
            );

            if is_audio && let Ok(ans) = get_audio_metadata(&path, db) {
                songs.push(ans);
            }
        }
    }

    Ok(())
}

// given a music file get the metadata of the track.
// artist, album title, release date,  ...
fn get_audio_metadata(
    path: &Path,
    db: Option<&Database>,
) -> Result<TrackDetails, Box<dyn std::error::Error>> {
    let tagged_file: TaggedFile = read_from_path(path)?;
    let tag = tagged_file.primary_tag().unwrap();
    let title = tag.title().unwrap_or("Unknown Title".into()).to_string();
    let artist = tag.artist().unwrap_or("Unknown Artist".into()).to_string();
    let album = tag.album().unwrap_or("Unknown Album".into()).to_string();
    let date = tag
        .date()
        .unwrap_or(lofty::tag::items::Timestamp {
            year: (1900),
            month: (Some(1)),
            day: (Some(1)),
            hour: (Some(0)),
            minute: (Some(0)),
            second: (Some(0)),
        })
        .to_string();
    let track_no = tag.track().unwrap_or(0);
    let song_path = path.to_string_lossy().to_string();
    let duration = tagged_file.properties().duration().as_secs();

    let time_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let song_stats = match read_or_insert(db, &song_path) {
        Some(stats) => stats,
        _ => (0, 0, time_now),
    };

    let tags = get_playlist_labels(db, &song_path).unwrap_or_default();

    Ok(TrackDetails {
        artist,
        album,
        title,
        track_no,
        date,
        song_path,
        duration,
        stats: song_stats,
        tags,
    })
}

pub fn get_song_art(song: &TrackDetails) -> Option<Vec<u8>> {
    let parent_dir = std::path::Path::new(&song.song_path).parent()?;
    let target_path = parent_dir.join("cover.jpg");

    match target_path.exists() {
        true => Some(std::fs::read(&target_path).unwrap_or_default()),
        false => get_embedded_song_art(song),
    }
}

pub fn get_embedded_song_art(song: &TrackDetails) -> Option<Vec<u8>> {
    let tagged_file: TaggedFile = read_from_path(&song.song_path).ok()?;
    let tag = tagged_file.primary_tag().unwrap();
    let pics = tag.pictures();

    if pics.is_empty() {
        return None;
    }

    Some(pics[0].clone().into_data())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(artist: &str, album: &str, title: &str) -> TrackDetails {
        TrackDetails {
            artist: artist.to_string(),
            album: album.to_string(),
            title: title.to_string(),
            track_no: 1,
            date: "2020".to_string(),
            song_path: "/fake/path.mp3".to_string(),
            duration: 180,
            stats: (0, 0, 0),
            tags: Vec::new(),
        }
    }

    fn library() -> Vec<TrackDetails> {
        vec![
            track("Radiohead", "OK Computer", "Karma Police"),
            track("Radiohead", "Kid A", "Everything in Its Right Place"),
            track("Pink Floyd", "The Wall", "Comfortably Numb"),
            track(
                "Pink Floyd",
                "Wish You Were Here",
                "Shine On You Crazy Diamond",
            ),
            track("David Bowie", "Ziggy Stardust", "Starman"),
        ]
    }

    // 1. Empty query returns every track unchanged.
    #[test]
    fn empty_query_returns_all() {
        let lib = library();
        let result = filter_tracks(&lib, "");
        assert_eq!(result.len(), lib.len());
    }

    // 2. Query that matches nothing returns an empty list.
    #[test]
    fn no_match_returns_empty() {
        let result = filter_tracks(&library(), "zzznomatch");
        assert!(result.is_empty());
    }

    // 3. Artist match returns the correct tracks.
    #[test]
    fn filter_by_artist() {
        let result = filter_tracks(&library(), "Radiohead");
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|t| t.artist == "Radiohead"));
    }

    // 4. Album match returns the correct track.
    #[test]
    fn filter_by_album() {
        let result = filter_tracks(&library(), "Kid A");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Everything in Its Right Place");
    }

    // 5. Title match returns the correct track.
    #[test]
    fn filter_by_title() {
        let result = filter_tracks(&library(), "Starman");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].artist, "David Bowie");
    }

    // 6. Search is case-insensitive.
    #[test]
    fn filter_is_case_insensitive() {
        let upper = filter_tracks(&library(), "RADIOHEAD");
        let lower = filter_tracks(&library(), "radiohead");
        let mixed = filter_tracks(&library(), "rAdIoHeAd");
        assert_eq!(upper.len(), lower.len());
        assert_eq!(lower.len(), mixed.len());
    }

    // 7. Partial query matches across all fields.
    #[test]
    fn partial_query_matches() {
        // "wall" matches the album "The Wall" and also "Wish You Were Here" does not contain it,
        // so expect exactly one result.
        let result = filter_tracks(&library(), "wall");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].album, "The Wall");
    }

    // 8. A query matching multiple artists returns all of them.
    #[test]
    fn filter_matches_multiple_artists() {
        // "pink" matches both Pink Floyd tracks.
        let result = filter_tracks(&library(), "pink");
        assert_eq!(result.len(), 2);
    }

    // 9. Ord: tracks sort by artist → album → track_no → title.
    #[test]
    fn tracks_sort_correctly() {
        let mut tracks = [
            track("Radiohead", "OK Computer", "Karma Police"),
            track("Radiohead", "Kid A", "Everything in Its Right Place"),
            track("David Bowie", "Ziggy Stardust", "Starman"),
        ];
        tracks.sort();
        assert_eq!(tracks[0].artist, "David Bowie");
        assert_eq!(tracks[1].album, "Kid A");
        assert_eq!(tracks[2].album, "OK Computer");
    }

    // 10. Text<'static> display format (owned) is correct.
    #[test]
    fn track_display_format_owned() {
        use ratatui::prelude::Text;
        let t = track("Radiohead", "OK Computer", "Karma Police");
        let text = Text::from(t);
        let rendered = text.to_string();
        assert!(rendered.contains("Radiohead"));
        assert!(rendered.contains("Karma Police"));
        assert!(rendered.contains("Track 1"));
    }
}
