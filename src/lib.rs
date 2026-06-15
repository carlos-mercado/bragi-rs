use lofty::file::TaggedFile;
use lofty::{prelude::*, read_from_path};
use ratatui::prelude::{Text};
use std::cmp::Ord;
use std::fs::{self, DirEntry};
use std::io;
use std::path::{Path, PathBuf};
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Album {
    artist: String,
    album: String,
    //duration: u64,
    selected: bool,
    pub songs: BTreeSet<TrackDetails>
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TrackDetails {
    pub artist: String,
    pub album: String,
    pub track_no: u32,
    pub title: String,
    pub date: String,
    pub song_path: String,
    pub duration: u64,
}

impl From<&Album> for Text<'static> {
    fn from(album: &Album) -> Self {
        Text::from(format!(
            "{} - {}",
            album.artist, album.album
        ))
    }
}

impl From<TrackDetails> for Text<'static> {
    fn from(track: TrackDetails) -> Self {
        Text::from(format!(
            "{}\n{}\n{} [Track {}]",
            track.artist, track.title, track.album, track.track_no,
        ))
    }
}

impl From<&TrackDetails> for Text<'static> {
    fn from(track: &TrackDetails) -> Self {
        Text::from(format!(
            "{} - {} ({}) [Track {}]",
            track.artist, track.title, track.date, track.track_no,
        ))
    }

}

pub fn build_albums(tracks: &Vec<TrackDetails>) -> Vec<Album> {
    // (artist, album) => 
    let mut album_to_songs: HashMap<(&String, &String), BTreeSet<TrackDetails>> = HashMap::new();
    for track in tracks {
        album_to_songs
            .entry((&track.artist, &track.album))
            .or_default()
            .insert(track.clone());
    }

    let mut albums = Vec::new();
    for ((artist, album), songs) in album_to_songs {
        albums.push(Album {
            artist: artist.to_string(),
            album: album.to_string(),
            selected: false,
            songs,
        });
    }

    albums.sort();
    albums
}

/// Filter a list of tracks by a query string.
/// Matches case-insensitively against artist, album, and title.
/// Returns a new Vec containing only the matching tracks.
pub fn filter_tracks(tracks: &[TrackDetails], query: &str) -> Vec<TrackDetails> {
    let q = query.to_lowercase();
    tracks
        .iter()
        .cloned()
        .filter(|t| {
            t.artist.to_lowercase().contains(&q)
                || t.album.to_lowercase().contains(&q)
                || t.title.to_lowercase().contains(&q)
        })
        .collect()
}


// TODO
// this is recusive dfs with no cycle checks
// idk if cylces are possible to create in file systems
// maybe with symlinks, but i don't wan't to deal
// with that right now

pub fn get_music_files(path: &Path, songs: &mut Vec<TrackDetails>) -> io::Result<()> {
    let mut it: fs::ReadDir = fs::read_dir(path)?;

    while let Some(entry) = it.next() {
        let entry: DirEntry = entry?;

        if entry.metadata()?.is_dir() {
            get_music_files(&entry.path(), songs)?;
        } else {
            let path: PathBuf = entry.path();
            let file_type = path.extension().and_then(|e| e.to_str()).unwrap();

            let is_audio = matches!(
                file_type.to_lowercase().as_str(),
                "mp3" | "flac" | "alac" | "wav" | "aac" | "ogg" | "m4a" | "aiff"
            );

            if is_audio {
                match get_audio_metadata(&path) {
                    Ok(ans) => songs.push(ans),
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

fn get_audio_metadata(path: &Path) -> Result<TrackDetails, Box<dyn std::error::Error>> {
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

    let duration = tagged_file.properties().duration().as_secs();
    Ok(TrackDetails {
        artist,
        album,
        title,
        track_no,
        date,
        song_path: path.to_string_lossy().to_string(),
        duration,
    })
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
        let mut tracks = vec![
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
