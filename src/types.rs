use music::{Album, TrackDetails};

#[derive(Clone)]
pub struct SongPath(pub String);
#[derive(Clone)]
pub struct Label(pub String);

#[derive(PartialEq)]
pub enum VimMode {
    Search,
    Normal,
    Command,
    Marking,
    VisualLine,
}

#[derive(PartialEq)]
pub enum PlaybackMode {
    Paused,
    Playing,
    NotPlaying,
}

pub enum MusicStreamEvent {
    // user pressed selected a new playlist
    // represened as (playlist, song_selected_idx).
    NewPlaylistEvent(Vec<TrackDetails>, usize),
    // user paused, resumed, or completed the song
    // represened as (PlaybackMode, song_selected_idx).
    PlaybackEvent(PlaybackMode, usize),
    // a track played to completion and the next song is now
    // playing. (next_song_info, next_song_idx).
    TrackAutoAdvanced(TrackDetails, usize),
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Page {
    // this is the default page
    Albums,
    // user got to this page via album-selection
    Songs,
    // user got to this page via search
    Search,
}

pub enum DbUpdate {
    LastPlayed(String),
    DurationPlayed(String, u64),
}

pub enum LocalTrackUpdateType {
    DurationPlayed(SongPath, u64),
    LastPlayed(SongPath),
    AddLabel(SongPath, Label),
    WipeLabels(SongPath),
}

#[derive(Clone)]
pub enum MusicItems {
    Albums(Vec<Album>),
    Songs(Vec<TrackDetails>),
}

/*pub enum Item {
    Album(Album),
    Track(TrackDetails),
}*/
