use music::TrackDetails;

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
}

#[derive(PartialEq)]
pub enum PlaybackMode {
    Paused,
    Playing,
    NotPlaying,
}

pub enum MusicStreamEvent {
    //NewSongEvent(TrackDetails),
    NewPlaylistEvent(Vec<TrackDetails>, u64),
    PlaybackEvent(PlaybackMode, u64),
    TrackAutoAdvanced(TrackDetails, u64),
}

#[derive(PartialEq)]
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
