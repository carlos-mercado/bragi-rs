use music::{ TrackDetails };

#[derive(PartialEq)]
pub enum VimMode {
    Search,
    Normal,
    Command
}

#[derive(PartialEq)]
pub enum PlaybackMode {
    Paused,
    Playing,
    NotPlaying,
}

pub enum MusicStreamEvent {
    //NewSongEvent(TrackDetails),
    NewPlaylistEvent(Vec<TrackDetails>),
    PlaybackEvent(PlaybackMode),
    TrackAutoAdvanced(TrackDetails),
}

pub enum Page {
    AlbumsView,
    SongsView,
    SearchView
}

pub enum DbUpdate {
    LastPlayed(String),
    DurationPlayed(String, u64),
}
