use lofty::file::TaggedFile;
use lofty::{prelude::*, read_from_path};
use ratatui::prelude::Text;
use redb::Database;
use std::cmp::Ord;
use std::collections::HashMap;
use std::fs::{self, DirEntry};
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{io, thread};
use wincode::SchemaRead;
use wincode::SchemaWrite;

pub mod config;
pub mod db;
use crate::db::*;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Album {
    pub artist: String,
    pub album: String,
    selected: bool,
    pub date: String,
    pub songs: Vec<TrackDetails>,
    pub stats: (u64, u64, u64),
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash, Default)]
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

#[derive(SchemaWrite, SchemaRead)]
pub struct TrackMetadata {
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
            "{} - {} [{}]",
            album.artist, album.album, album.date,
        ))
    }
}

impl From<TrackDetails> for Text<'static> {
    fn from(track: TrackDetails) -> Self {
        Text::from(format!(
            "{}\n{}\n{}\n[Track {}]",
            track.artist, track.title, track.album, track.track_no
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

pub fn builder(song_listener: Receiver<TrackDetails>) -> (Vec<Album>, Vec<TrackDetails>) {
    let mut albums_hashmap: HashMap<(String, String), Album> = HashMap::new();
    loop {
        match song_listener.try_recv() {
            Ok(track) => {
                let artist = track.artist.clone();
                let album_title = track.album.clone();
                let date = track.date.clone();
                let album = albums_hashmap
                    .entry((artist.clone(), album_title.clone()))
                    .or_insert(Album {
                        artist,
                        album: album_title,
                        date,
                        selected: false,
                        songs: Vec::new(),
                        stats: track.stats,
                    });

                album.songs.push(track.clone());

                // last_played: take the most recent
                album.stats.0 = album.stats.0.max(track.stats.0);
                // duration_played: accumulate
                album.stats.1 += track.stats.1;
                // date_added: take the earliest
                album.stats.2 = album.stats.2.min(track.stats.2);
                album.songs.push(track);
            }
            Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {}
        }
    }

    let songs: Vec<TrackDetails> = albums_hashmap
        .values()
        .flat_map(|a| a.songs.clone())
        .collect();
    let albums: Vec<Album> = albums_hashmap
        .values_mut()
        .map(|a| {
            a.songs.sort();
            a.clone()
        })
        .collect();
    (albums, songs)
}

pub fn divide_and_conquer(
    db: Option<Arc<Database>>,
    sender: Arc<Sender<TrackDetails>>,
    music_path: &Path,
) -> io::Result<()> {
    let it: fs::ReadDir = fs::read_dir(music_path)?;
    let bucket_count = 20;
    let mut buckets: Vec<Vec<fs::DirEntry>> = (0..bucket_count).map(|_| Vec::new()).collect();
    for (idx, path) in it.enumerate() {
        buckets[idx % bucket_count].push(path.unwrap());
    }

    let mut handles = Vec::new();
    for dir_bucket in buckets {
        let sender = Arc::clone(&sender);
        let db = db.clone(); // cheap Arc clone
        let handle = thread::spawn(move || {
            for file in dir_bucket {
                extract_music_from_dir(file, db.as_deref(), sender.clone());
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    Ok(())
}

// TODO
// this is recusive dfs with no cycle checks
// idk if cylces are possible to create in file systems
// maybe with symlinks, but i don't wan't to deal
// with that right now

// dfs through all directories in music library
// and extract all music files
fn extract_music_from_dir(
    file: DirEntry,
    db: Option<&Database>,
    sender: Arc<Sender<TrackDetails>>,
) -> Option<u32> {
    let mut total = 0;

    if file.metadata().ok()?.is_dir() {
        for child_file in fs::read_dir(file.path()).ok()? {
            if let Some(t) = extract_music_from_dir(child_file.unwrap(), db, sender.clone()) {
                total += t;
            }
        }
    } else {
        let file_path: PathBuf = file.path();
        let file_type = file_path.extension().and_then(|e| e.to_str());
        let mut is_audio = false;
        if let Some(file_type) = file_type {
            is_audio = matches!(
                file_type.to_lowercase().as_str(),
                "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "aiff"
            );
        }

        if is_audio {
            if db.is_some()
                && let Some(metadata) =
                    get_cached_metadata(db.unwrap(), file_path.to_str().unwrap_or_default())
            {
                let tags = get_playlist_labels(db, file_path.to_str().unwrap()).unwrap_or_default();
                let time_now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let stats = match read_or_insert(db, file_path.to_str().unwrap()) {
                    Some(stats) => stats,
                    _ => (0, 0, time_now),
                };

                sender
                    .send(TrackDetails {
                        artist: metadata.artist,
                        album: metadata.album,
                        track_no: metadata.track_no,
                        title: metadata.title,
                        date: metadata.date,
                        song_path: metadata.song_path,
                        duration: metadata.duration,
                        stats,
                        tags,
                    })
                    .expect("couldn't send track through tunnel");
            } else {
                if db.is_some()
                    && let Ok(track) = get_audio_metadata(&file_path, db)
                {
                    cache_metadata(
                        db.unwrap(),
                        file_path.to_str().unwrap(),
                        &TrackMetadata {
                            artist: track.artist.clone(),
                            album: track.album.clone(),
                            track_no: track.track_no,
                            title: track.title.clone(),
                            date: track.date.clone(),
                            song_path: track.song_path.clone(),
                            duration: track.duration,
                        },
                    )
                    .ok()?;

                    sender
                        .send(track)
                        .expect("couldn't send track through tunnel");
                }
            }
        }
    }

    Some(total)
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

// given a music file get the metadata of the track.
// artist, album title, release date,  ...
fn get_audio_metadata(
    path: &Path,
    db: Option<&Database>,
) -> Result<TrackDetails, Box<dyn std::error::Error>> {
    let tagged_file: TaggedFile = read_from_path(path)?;
    let Some(tag) = tagged_file.primary_tag() else {
        return Ok(TrackDetails::default());
    };
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
        true => Some(std::fs::read(&target_path).ok()?),
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
