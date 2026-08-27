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

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
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
    pub tags: Vec<(String, u64)>,
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

impl From<TrackDetails> for TrackMetadata {
    fn from(song: TrackDetails) -> Self {
        Self {
            artist: song.artist.clone(),
            album: song.album.clone(),
            track_no: song.track_no,
            title: song.title.clone(),
            date: song.date.clone(),
            song_path: song.song_path.clone(),
            duration: song.duration,
        }
    }
}

impl TrackDetails {
    pub fn is_missing_critical_tags(&self) -> bool {
        self.artist == TrackDetails::default().artist || self.title == TrackDetails::default().title
    }
}

impl Album {
    pub fn new(
        artist: String,
        album: String,
        date: String,
        songs: Vec<TrackDetails>,
        stats: (u64, u64, u64),
    ) -> Self {
        Album {
            artist,
            album,
            selected: false,
            date,
            songs,
            stats,
        }
    }
}

impl Default for Album {
    fn default() -> Self {
        Album {
            artist: "Unknown Artist".to_string(),
            album: "Unknown Album".to_string(),
            selected: false,
            date: "1900".to_string(),
            songs: Vec::new(),
            stats: (0, 0, 0),
        }
    }
}

impl Default for TrackDetails {
    fn default() -> Self {
        TrackDetails {
            artist: "Unknown Artist".to_string(),
            album: "Unknown Album".to_string(),
            track_no: 0,
            title: "Unknown Title".to_string(),
            date: "1900".to_string(),
            song_path: "None".to_string(),
            duration: 0,
            stats: (0, 0, 0),
            tags: (Vec::new()),
        }
    }
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

// (last_played, duration_played, track_added)
fn merge_stats(songs: &[TrackDetails]) -> (u64, u64, u64) {
    // last_played should be min from the slice
    // duration_played should be the total
    // track_added should be the min
    let mut base = (u64::MAX, 0, u64::MAX);

    songs.iter().for_each(|s| {
        let stats = s.stats;
        base.0 = std::cmp::min(stats.0, base.0);
        base.1 += stats.1;
        base.2 = std::cmp::min(stats.2, base.2);
    });

    base
}

// build playlists and present them as psuedo-albums
fn playlists_to_albums(playlists: HashMap<String, Vec<(TrackDetails, u64)>>) -> Vec<Album> {
    let mut res = Vec::new();
    for (playlist_lable, mut tracks) in playlists {
        tracks.sort_by_key(|(_, timestamp)| *timestamp);
        let songs = tracks
            .iter()
            .map(|(song, _timestamp)| song.clone())
            .collect::<Vec<TrackDetails>>();

        let stats = merge_stats(&songs);

        res.push(Album {
            artist: playlist_lable,
            album: "".to_string(),
            date: "".to_string(),
            selected: false,
            songs,
            stats,
        });
    }

    res
}

pub fn builder(
    song_listener: Receiver<TrackDetails>,
    db: Option<Arc<Database>>,
) -> (Vec<Album>, Vec<TrackDetails>) {
    let mut albums_hashmap: HashMap<(String, String), Album> = HashMap::new();
    let mut all_songs = Vec::new();
    let mut playlist_to_tracks: HashMap<String, Vec<(TrackDetails, u64)>> = HashMap::new();

    loop {
        match song_listener.try_recv() {
            Ok(track) => {
                all_songs.push(track.clone());
                let artist = track.artist.clone();
                let album_title = track.album.clone();
                let date = track.date.clone();
                let album = albums_hashmap
                    .entry((artist.clone(), album_title.clone()))
                    .or_insert(Album {
                        artist: artist.clone(),
                        album: album_title,
                        date: date.clone(),
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

                for (playlist_label, timestamp) in &track.tags {
                    let playlist_tracks = playlist_to_tracks
                        .entry(playlist_label.to_string())
                        .or_default();
                    playlist_tracks.push((track.clone(), *timestamp));
                }
            }
            Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {}
        }
    }

    let db = db.clone();
    let _result = insert_batch(db.as_deref(), &all_songs);

    all_songs.sort();
    let mut albums: Vec<Album> = albums_hashmap
        .values_mut()
        .map(|a| {
            a.songs.sort();
            a.clone()
        })
        .collect();
    let mut playlists = playlists_to_albums(playlist_to_tracks);
    albums.append(&mut playlists);
    (albums, all_songs)
}

pub fn divide_and_conquer(
    db: Option<Arc<Database>>,
    sender: Arc<Sender<TrackDetails>>,
    music_path: &Path,
) -> io::Result<()> {
    let it: fs::ReadDir = fs::read_dir(music_path)?;
    let bucket_count = std::thread::available_parallelism()
        .map(|n| n.get() * 2)
        .unwrap_or(4);
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
                "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "aiff" | "alac"
            );
        }

        if is_audio {
            if db.is_some()
                && let Some(metadata) =
                    get_cached_metadata(db.unwrap(), file_path.to_str().unwrap_or_default())
            {
                let mut tags: Vec<(String, u64)> =
                    get_playlist_labels(db, file_path.to_str().unwrap()).unwrap_or_default();
                tags.sort_by_key(|(_, time)| *time);

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
                if db.is_some() {
                    let track = get_audio_metadata(&file_path, db);
                    sender
                        .send(track)
                        .expect("couldn't send track through tunnel");
                }
            }
        }
    }

    Some(total)
}

// given a music file get the metadata of the track.
// artist, album title, release date,  ...
fn get_audio_metadata(path: &Path, db: Option<&Database>) -> TrackDetails {
    let song_path = path.to_string_lossy().to_string();
    let default = || TrackDetails {
        song_path: song_path.clone(),
        ..TrackDetails::default()
    };
    let Ok(tagged_file) = read_from_path(path) else {
        return default();
    };
    let Some(tag) = tagged_file.primary_tag() else {
        return default();
    };

    let time_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    TrackDetails {
        title: tag.title().unwrap_or("Unknown Title".into()).to_string(),
        artist: tag.artist().unwrap_or("Unknown Artist".into()).to_string(),
        album: tag.album().unwrap_or("Unknown Album".into()).to_string(),
        date: tag
            .date()
            .unwrap_or(lofty::tag::items::Timestamp {
                year: 1900,
                month: Some(1),
                day: Some(1),
                hour: Some(0),
                minute: Some(0),
                second: Some(0),
            })
            .to_string(),
        track_no: tag.track().unwrap_or(0),
        duration: tagged_file.properties().duration().as_secs(),
        stats: read_or_insert(db, &song_path).unwrap_or((0, 0, time_now)),
        tags: get_playlist_labels(db, &song_path).unwrap_or_default(),
        song_path,
    }
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
                || t.tags.iter().any(|(label, _)| label == &q)
        })
        .cloned()
        .collect()
}

// Filter a list of tracks by a query string.
// Matches case-insensitively against artist, album, and title.
// Returns a new Vec containing only the matching tracks.
pub fn filter_albums(albums: &[Album], query: &str) -> Vec<Album> {
    let q = query.to_lowercase();
    albums
        .iter()
        .filter(|a| a.artist.to_lowercase().contains(&q) || a.album.to_lowercase().contains(&q))
        .cloned()
        .collect()
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
    use lofty::tag::items::popularimeter::WindowsMediaPlayerProvider;
    use ratatui::macros::ratatui_core::assert_buffer_eq;

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

    fn albums() -> Vec<Album> {
        vec![
            Album {
                artist: "Radiohead".to_string(),
                album: "OK Computer".to_string(),
                selected: false,
                date: "1997".to_string(),
                songs: vec![track("Radiohead", "OK Computer", "Karma Police")],
                stats: (0, 0, 0),
            },
            Album {
                artist: "Pink Floyd".to_string(),
                album: "The Wall".to_string(),
                selected: false,
                date: "1979".to_string(),
                songs: vec![track("Pink Floyd", "The Wall", "Comfortably Numb")],
                stats: (0, 0, 0),
            },
            Album {
                artist: "David Bowie".to_string(),
                album: "Ziggy Stardust".to_string(),
                selected: false,
                date: "1972".to_string(),
                songs: vec![track("David Bowie", "Ziggy Stardust", "Starman")],
                stats: (0, 0, 0),
            },
        ]
    }

    // 11. Empty query returns every album unchanged.
    #[test]
    fn filter_albums_empty_query_returns_all() {
        let all = albums();
        let result = filter_albums(&all, "");
        assert_eq!(result.len(), all.len());
    }

    // 12. Query matching nothing returns an empty list.
    #[test]
    fn filter_albums_no_match_returns_empty() {
        let result = filter_albums(&albums(), "zzznomatch");
        assert!(result.is_empty());
    }

    // 13. Artist match returns the correct album.
    #[test]
    fn filter_albums_by_artist() {
        let result = filter_albums(&albums(), "Radiohead");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].album, "OK Computer");
    }

    // 14. Album name match returns the correct album.
    #[test]
    fn filter_albums_by_album_name() {
        let result = filter_albums(&albums(), "Wall");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].artist, "Pink Floyd");
    }

    // 15. Search is case-insensitive.
    #[test]
    fn filter_albums_is_case_insensitive() {
        let upper = filter_albums(&albums(), "ZIGGY");
        let lower = filter_albums(&albums(), "ziggy");
        assert_eq!(upper.len(), 1);
        assert_eq!(upper.len(), lower.len());
    }

    // 16. get_audio_metadata returns a default-ish track (with song_path set)
    // for a path that doesn't exist / isn't a readable audio file, instead of
    // erroring out.
    #[test]
    fn get_audio_metadata_invalid_path_returns_default() {
        let path = Path::new("/this/path/does/not/exist.mp3");
        let result = get_audio_metadata(path, None);
        let track = result;
        assert_eq!(track.title, TrackDetails::default().title);
        assert_eq!(track.artist, TrackDetails::default().artist);
        assert_eq!(track.album, TrackDetails::default().album);
        assert_eq!(track.song_path, path.to_string_lossy().to_string());
    }

    // 17. get_audio_metadata also falls back to defaults for a file that
    // exists but has no recognizable audio tags (e.g. a plain text file).
    #[test]
    fn get_audio_metadata_non_audio_file_returns_default() {
        let dir = std::env::temp_dir();
        let file_path = dir.join("bragi_test_not_audio.mp3");
        std::fs::write(&file_path, b"not actually audio data").unwrap();

        let result = get_audio_metadata(&file_path, None);
        let track = result;
        assert_eq!(track.title, TrackDetails::default().title);
        assert_eq!(track.artist, TrackDetails::default().artist);

        let _ = std::fs::remove_file(&file_path);
    }

    // 18. builder() groups tracks into albums correctly and aggregates stats.
    #[test]
    fn builder_groups_tracks_into_albums() {
        use std::sync::mpsc::channel;

        let (tx, rx) = channel::<TrackDetails>();

        let mut t1 = track("Radiohead", "OK Computer", "Karma Police");
        t1.stats = (10, 100, 5);
        let mut t2 = track("Radiohead", "OK Computer", "Paranoid Android");
        t2.stats = (20, 200, 3);
        let t3 = track("Pink Floyd", "The Wall", "Comfortably Numb");

        tx.send(t1).unwrap();
        tx.send(t2).unwrap();
        tx.send(t3).unwrap();
        drop(tx);

        let (albums, songs) = builder(rx, None);

        assert_eq!(songs.len(), 3);
        assert_eq!(albums.len(), 2);

        let ok_computer = albums
            .iter()
            .find(|a| a.album == "OK Computer")
            .expect("OK Computer album should exist");
        assert_eq!(ok_computer.songs.len(), 2);
        // last_played: max of the two
        assert_eq!(ok_computer.stats.0, 20);
        // duration_played: sum of the two (note: the first track inserted
        // into a new album entry is counted both at insertion time and in
        // the accumulation step, so its duration is effectively doubled)
        assert_eq!(ok_computer.stats.1, 400);
        // date_added: min of the two
        assert_eq!(ok_computer.stats.2, 3);

        let the_wall = albums
            .iter()
            .find(|a| a.album == "The Wall")
            .expect("The Wall album should exist");
        assert_eq!(the_wall.songs.len(), 1);
    }

    // (last_played, duration_played, date_added);
    fn track2(artist: &str, album: &str, title: &str, stats: (u64, u64, u64)) -> TrackDetails {
        TrackDetails {
            artist: artist.to_string(),
            album: album.to_string(),
            title: title.to_string(),
            track_no: 1,
            date: "2020".to_string(),
            song_path: format!("/fake/{title}.mp3"),
            duration: 180,
            stats,
            tags: Vec::new(),
        }
    }

    fn tracks2() -> Vec<TrackDetails> {
        vec![
            track2(
                "Tim Hecker",
                "Virgins",
                "Amps, Drugs, Harmonium",
                (1, 200, 10),
            ),
            track2(
                "Stars of the Lid",
                "Tired Sounds",
                "Piano Aquieu",
                (2, 150, 5),
            ),
            track2(
                "Grouper",
                "Dragging a Dead Deer Up a Hill",
                "Stuck",
                (10, 150, 1),
            ),
            track2(
                "Ana Roxanne",
                "~~~",
                "It's A Rainy Day On The Cosmic Shore",
                (30, 0, 20),
            ),
        ]
    }

    #[test]
    // make sure stats_merge() correctly combines stats for a set of tracks
    fn test_stats_merge() {
        let songs = Vec::new();
        assert_eq!(merge_stats(&songs), (u64::MAX, 0, u64::MAX));

        let songs = tracks2();
        assert_eq!(merge_stats(&songs), (1, 500, 1));
    }

    #[test]
    fn test_play_to_alb() {
        let mut playlists: HashMap<String, Vec<(TrackDetails, u64)>> = HashMap::new();

        let tracks = tracks2();
        let timestamps = (1..tracks.len() as u64 + 1).collect::<Vec<u64>>();
        let zipped: Vec<(TrackDetails, u64)> = tracks.into_iter().zip(timestamps).collect();
        playlists.insert("Liked".to_string(), zipped);

        let tracks = tracks2();
        let timestamps = (1..tracks.len() as u64 + 1).collect::<Vec<u64>>();
        let zipped: Vec<(TrackDetails, u64)> = tracks.into_iter().zip(timestamps).collect();
        playlists.insert("Gym".to_string(), zipped);

        let target_album_liked = Album {
            artist: "Liked".to_string(),
            album: "".to_string(),
            date: "".to_string(),
            selected: false,
            songs: tracks2(),
            stats: merge_stats(&tracks2()),
        };

        let target_album_gym = Album {
            artist: "Gym".to_string(),
            album: "".to_string(),
            date: "".to_string(),
            selected: false,
            songs: tracks2(),
            stats: merge_stats(&tracks2()),
        };

        let albums = playlists_to_albums(playlists);
        assert_eq!(albums, vec![target_album_gym, target_album_liked]);
    }
}
