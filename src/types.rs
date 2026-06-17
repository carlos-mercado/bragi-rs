use music::{ TrackDetails };

#[derive(PartialEq)]
pub enum VimMode {
    Search,
    Normal,
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
