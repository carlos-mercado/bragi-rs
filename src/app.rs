use crate::types::{
    DbUpdate, Label, LocalTrackUpdateType, MusicStreamEvent, Page, PlaybackMode, SongPath, VimMode,
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use lru::LruCache;
use music::config::config_init;
use music::{Album, TrackDetails, filter_albums, filter_tracks};
use music::{builder, db::*, divide_and_conquer, get_song_art};
use ratatui::DefaultTerminal;
use ratatui::Frame;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use redb::Database;
use std::cmp::Reverse;
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

// (bytes, protocol)
pub type AlbumArtInfo = (Vec<u8>, Arc<RwLock<StatefulProtocol>>);

// Clamp the cursor after moving "down" a list of `len` items.
// Never moves past the last valid index; a `len` of 0 stays at 0.
fn clamp_cursor_increment(cursor: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    std::cmp::min(len - 1, cursor + 1)
}

// Clamp the cursor after moving "up" a list. Never goes below 0.
fn clamp_cursor_decrement(cursor: usize) -> usize {
    cursor.saturating_sub(1)
}

// Returns the next playlist index, or `None` if `current` is already
// at (or past) the last playable index.
fn next_index(current: usize, len: usize) -> Option<usize> {
    if len == 0 || current + 1 >= len {
        return None;
    }
    Some(current + 1)
}

// Returns the previous playlist index, or `None` if `current` is
// already at index 0.
fn prev_index(current: usize) -> Option<usize> {
    if current == 0 {
        None
    } else {
        Some(current - 1)
    }
}

// Parses a user-entered command buffer of the form "<cmd> <arg>" into
// its two whitespace-separated parts. Returns `None` if the input isn't
// exactly two space-separated tokens.
fn parse_command_args(input: &str) -> Option<(&str, &str)> {
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
fn with_matching_song_and_album_song<F>(
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

fn apply_duration_played(
    all_songs: &mut [TrackDetails],
    albums: &mut [Album],
    song_path: &str,
    time: u64,
) {
    with_matching_song_and_album_song(all_songs, albums, song_path, |s| s.stats.1 += time);
}

fn apply_last_played(
    all_songs: &mut [TrackDetails],
    albums: &mut [Album],
    song_path: &str,
    timestamp: u64,
) {
    with_matching_song_and_album_song(all_songs, albums, song_path, |s| s.stats.0 = timestamp);
}

fn apply_wipe_labels(all_songs: &mut [TrackDetails], albums: &mut [Album], song_path: &str) {
    with_matching_song_and_album_song(all_songs, albums, song_path, |s| s.tags.clear());
}

fn apply_add_label(
    all_songs: &mut [TrackDetails],
    albums: &mut [Album],
    song_path: &str,
    label: &str,
) {
    with_matching_song_and_album_song(all_songs, albums, song_path, |s| {
        s.tags.push(label.to_string())
    });
}

pub struct App {
    pub exit: bool,
    pub cursor: usize,
    pub mode: VimMode,
    pub viewer: Page,

    // all_songs are the songs that are produced
    // after running all_songs_unfiltered
    // through some (possibly none) search filters
    pub all_songs: Vec<TrackDetails>,
    pub all_songs_unfiltered: Vec<TrackDetails>,
    pub page_songs: Vec<TrackDetails>,
    pub page_songs_unfiltered: Vec<TrackDetails>,
    pub playing_song: Option<TrackDetails>,
    pub playing_song_idx: Option<usize>,
    pub page_albums: Vec<Album>,
    pub albums_unfiltered: Vec<Album>,
    pub album_selected: Option<Album>,
    pub albums_cursor: usize,

    pub song_queue: Option<Vec<TrackDetails>>,
    pub play_start: Option<Instant>,
    pub elapsed_before_paused: Duration,
    pub playback_sender: Sender<MusicStreamEvent>,
    pub playback_receiver: Option<Receiver<MusicStreamEvent>>,
    pub playback_mode: Arc<Mutex<PlaybackMode>>,
    pub audio_handle: rodio::MixerDeviceSink,
    pub user_buff: String,
    pub last_key: char,
    pub key_pressed_time: Instant,
    pub event_receiver: Option<Receiver<MusicStreamEvent>>,
    pub db: Option<Arc<Database>>,
    pub yank_buff: Vec<TrackDetails>,
    pub vline_begin: Option<usize>,
    pub image_picker: Picker,

    pub cover_art: Arc<Mutex<Option<AlbumArtInfo>>>,
    // Album => (art_bytes, hash)
    pub cache: Arc<Mutex<LruCache<Album, AlbumArtInfo>>>,
}

impl App {
    pub fn new() -> App {
        let config = config_init();
        let db = db_setup();
        let fixed_db = db.map(Arc::new);
        let (init_sender, init_receiver) = channel();
        let builder_handle = thread::spawn(move || builder(init_receiver));
        let _resolved = divide_and_conquer(
            fixed_db.clone(),
            Arc::new(init_sender.clone()),
            Path::new(&config.music_path),
        );
        drop(init_sender);
        let (mut albums_unfiltered, mut songs_vec) = builder_handle.join().unwrap();
        songs_vec.sort();
        albums_unfiltered.sort();
        let page_albums = albums_unfiltered.clone();
        let all_songs_unfiltered = songs_vec.clone();

        let songs_vec_clone = songs_vec.clone();
        let audio_handle = rodio::DeviceSinkBuilder::open_default_sink()
            .expect("Could not find default audio stream");
        let (playback_sender, receiver) = channel();
        let play_start = None;
        let elapsed_before_paused = Duration::from_secs(0);
        let playback_mode = Arc::new(Mutex::new(PlaybackMode::NotPlaying));
        let image_picker = Picker::from_query_stdio().expect("Could not create image picker");
        let cover_art = Arc::new(Mutex::new(None));
        let cache = Arc::new(Mutex::new(LruCache::new(NonZeroUsize::new(10).unwrap())));

        let mut app = App {
            cursor: 0,
            exit: false,
            page_songs: songs_vec,
            page_songs_unfiltered: Vec::new(),
            albums_unfiltered,
            page_albums,
            playing_song: None,
            playing_song_idx: None,
            album_selected: None,
            play_start,
            elapsed_before_paused,
            playback_sender,
            playback_receiver: Some(receiver),
            mode: VimMode::Normal,
            playback_mode,
            audio_handle,
            user_buff: String::new(),
            all_songs: songs_vec_clone,
            last_key: ' ',
            key_pressed_time: Instant::now(),
            event_receiver: None,
            db: fixed_db,
            song_queue: None,
            all_songs_unfiltered,
            vline_begin: None,
            viewer: Page::Albums,
            yank_buff: Vec::new(),
            albums_cursor: 0,
            image_picker,
            cover_art,
            cache,
        };

        app.playback();
        app
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }

    fn handle_events(&mut self) -> io::Result<()> {
        let Some(ref rx) = self.event_receiver else {
            return Ok(());
        };

        match rx.try_recv() {
            Ok(MusicStreamEvent::TrackAutoAdvanced(track_details, _i)) => {
                self.play_start = Some(Instant::now());
                self.elapsed_before_paused = Duration::from_secs(0);
                self.playing_song = Some(track_details);
                self.playing_song_idx = Some(self.playing_song_idx.unwrap() + 1);
                // the previous song played to completion, add the duration of the
                // track to the track statistics
                let prev_song =
                    self.song_queue.as_ref().unwrap()[&self.playing_song_idx.unwrap() - 1].clone();

                self.update_db(DbUpdate::DurationPlayed(
                    prev_song.song_path.clone(),
                    prev_song.duration,
                ));
                self.update_song_label_local(LocalTrackUpdateType::DurationPlayed(
                    SongPath(prev_song.song_path),
                    prev_song.duration,
                ));
                self.update_curr_song_art();
            }
            Ok(MusicStreamEvent::NewPlaylistEvent(queue, i)) => {
                // user chose a song
                //
                // if there was a previously playing song
                // update it's stats appropriately
                if let Some(prev_song) = self.playing_song.clone() {
                    let dur_played = self.get_time_elapsed().as_secs_f64() as u64;
                    self.update_song_label_local(LocalTrackUpdateType::DurationPlayed(
                        SongPath(prev_song.song_path.clone()),
                        dur_played,
                    ));
                    self.update_db(DbUpdate::DurationPlayed(
                        prev_song.song_path.clone(),
                        dur_played,
                    ));
                }

                self.playing_song_idx = Some(i);
                self.playing_song = Some(queue[i].clone());
                self.song_queue = Some(queue);

                let binding = Arc::clone(&self.playback_mode);
                let mut state = binding.lock().unwrap();
                *state = PlaybackMode::Playing;

                self.update_db(DbUpdate::LastPlayed(
                    self.playing_song
                        .clone()
                        .expect("a song should have been playing by now")
                        .song_path,
                ));
                self.update_song_label_local(LocalTrackUpdateType::LastPlayed(SongPath(
                    self.playing_song
                        .clone()
                        .expect("a song should have been playing by now")
                        .song_path,
                )));

                self.play_start = Some(Instant::now());
                self.elapsed_before_paused = Duration::from_secs(0);
                self.update_curr_song_art();
            }
            Ok(MusicStreamEvent::PlaybackEvent(_, _)) => unimplemented!(),
            Err(_) => {}
        };

        if event::poll(Duration::from_millis(500))? {
            match event::read()? {
                Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                    self.handle_key_event(key_event)
                }
                _ => {}
            };
        }
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match self.mode {
            VimMode::Normal => match key_event.code {
                KeyCode::Char('q') => self.exit(),
                KeyCode::Char('j') => self.increment_counter(),
                KeyCode::Char('k') => self.decrement_counter(),
                KeyCode::Char('h') => self.prev_song(),
                KeyCode::Char('l') => self.next_song(),
                KeyCode::Char('/') => {
                    self.mode = VimMode::Search;
                }
                KeyCode::Char(':') => self.mode = VimMode::Command,
                KeyCode::Char('V') => {
                    self.mode = VimMode::VisualLine;
                    self.vline_begin = Some(self.cursor);
                }
                KeyCode::Char('m') => {
                    self.mode = {
                        if self.viewer != Page::Albums {
                            VimMode::Marking
                        } else {
                            VimMode::Normal
                        }
                    }
                }
                KeyCode::Char('p') => match self.viewer {
                    Page::Albums => {
                        unimplemented!()
                    }
                    Page::Songs | Page::Search => {
                        if self.page_songs.is_empty() {
                            self.page_songs = self.yank_buff.clone();
                            return;
                        }

                        self.page_songs
                            .splice(self.cursor + 1..self.cursor + 1, self.yank_buff.clone());
                    }
                },
                KeyCode::Char('g') => {
                    let timeout = Duration::from_millis(300);
                    if self.last_key == 'g' && self.key_pressed_time.elapsed() < timeout {
                        self.cursor = 0;
                        self.albums_cursor = self.cursor;
                        self.last_key = ' ';
                    } else {
                        self.last_key = 'g';
                        self.key_pressed_time = Instant::now();
                    }
                }
                KeyCode::Char('y') => {
                    let timeout = Duration::from_millis(300);
                    if self.last_key == 'y' && self.key_pressed_time.elapsed() < timeout {
                        match self.viewer {
                            Page::Search | Page::Songs => {
                                self.yank_buff = vec![self.page_songs[self.cursor].clone()];
                            }
                            Page::Albums => {
                                self.yank_buff = self.page_albums[self.cursor].songs.clone();
                            }
                        }
                        self.last_key = ' ';
                    } else {
                        self.last_key = 'y';
                        self.key_pressed_time = Instant::now();
                    }
                }
                KeyCode::Char('d') => {
                    let timeout = Duration::from_millis(300);
                    if self.last_key == 'd'
                        && self.key_pressed_time.elapsed() < timeout
                        && !self.page_songs.is_empty()
                    {
                        match self.viewer {
                            Page::Search | Page::Songs => {
                                self.yank_buff = vec![self.page_songs[self.cursor].clone()];
                                self.page_songs.remove(self.cursor);
                            }
                            Page::Albums => {
                                self.yank_buff = self.page_albums[self.cursor].songs.clone();
                                self.page_albums.remove(self.cursor);
                            }
                        }

                        self.decrement_counter();
                        self.last_key = ' ';
                    } else {
                        self.last_key = 'd';
                        self.key_pressed_time = Instant::now();
                    }
                }
                KeyCode::Char('G') => match self.viewer {
                    Page::Albums => {
                        self.cursor = self.page_albums.len() - 1;
                        self.albums_cursor = self.cursor;
                    }
                    Page::Songs | Page::Search => {
                        self.cursor = self.page_songs.len() - 1;
                    }
                },
                KeyCode::Char(' ') => {
                    let binding = Arc::clone(&self.playback_mode);
                    let mut state = binding.lock().unwrap();
                    match *state {
                        PlaybackMode::NotPlaying => {}
                        PlaybackMode::Playing => {
                            *state = PlaybackMode::Paused;
                            self.playback_sender
                                .send(MusicStreamEvent::PlaybackEvent(PlaybackMode::Paused, 0))
                                .expect("Could not send through channel");
                            self.elapsed_before_paused += self.play_start.unwrap().elapsed();
                            self.play_start = None;
                        }
                        PlaybackMode::Paused => {
                            *state = PlaybackMode::Playing;
                            self.playback_sender
                                .send(MusicStreamEvent::PlaybackEvent(PlaybackMode::Playing, 0))
                                .expect("Could not send through channel");
                            self.play_start = Some(Instant::now());
                        }
                    }
                }
                KeyCode::Esc => {
                    self.page_albums = self.albums_unfiltered.clone();
                    self.cursor = self.albums_cursor;
                    self.user_buff.clear();
                    self.viewer = Page::Albums;
                    self.album_selected = None;
                }
                KeyCode::Enter => {
                    if self.page_songs.is_empty() || self.page_albums.is_empty() {
                        return;
                    }

                    match self.viewer {
                        Page::Albums => {
                            self.album_selected = Some(self.page_albums[self.cursor].clone());
                            self.viewer = Page::Songs;
                            self.page_songs = self.album_selected.as_ref().unwrap().songs.clone();
                            self.page_songs_unfiltered = self.page_songs.clone();
                            self.preload(self.album_selected.clone().unwrap());
                            self.cursor = 0;
                        }
                        Page::Songs | Page::Search => {
                            // the user picked a song
                            self.playback_sender
                                .send(MusicStreamEvent::NewPlaylistEvent(
                                    self.page_songs.clone(),
                                    self.cursor,
                                ))
                                .expect("Could not send through channel");
                        }
                    }
                }
                _ => {}
            },
            VimMode::Search => match key_event.code {
                KeyCode::Char(c) => {
                    self.user_buff.push(c);
                    if self.user_buff.starts_with('*') {
                        // if a search query starts with this
                        // seach the list of all songs in the
                        // library
                        self.viewer = Page::Search;
                    }

                    match self.viewer {
                        Page::Albums => {
                            self.page_albums = self.filter_albums();
                        }
                        Page::Songs | Page::Search => {
                            self.page_songs = self.filter_songs();
                        }
                    };
                    self.cursor = 0;
                }
                KeyCode::Backspace => {
                    if self.user_buff.is_empty() {
                        return;
                    }
                    self.user_buff.pop();

                    match self.viewer {
                        Page::Albums => {
                            self.page_albums = self.filter_albums();
                        }
                        Page::Songs | Page::Search => {
                            self.page_songs = self.filter_songs();
                        }
                    };
                    self.cursor = 0;
                }
                KeyCode::Enter => {
                    self.cursor = 0;
                    self.user_buff.clear();
                    self.mode = VimMode::Normal;
                }
                KeyCode::Esc => {
                    self.cursor = 0;
                    self.user_buff.clear();
                    self.mode = VimMode::Normal;

                    match self.viewer {
                        Page::Albums => {
                            self.page_albums = self.albums_unfiltered.clone();
                        }
                        Page::Songs => {
                            self.page_songs = self.page_songs_unfiltered.clone();
                        }
                        Page::Search => {
                            self.page_songs = self.all_songs_unfiltered.clone();
                        }
                    }
                }
                _ => {}
            },
            VimMode::Command => match key_event.code {
                KeyCode::Char(c) => {
                    self.user_buff.push(c);
                    self.cursor = 0;
                }
                KeyCode::Backspace => {
                    if self.user_buff.is_empty() {
                        return;
                    }
                    self.user_buff.pop();
                    self.cursor = 0;
                }
                KeyCode::Enter => {
                    self.parse_command();
                    self.user_buff.clear();
                    self.mode = VimMode::Normal;
                }
                KeyCode::Esc => {
                    self.cursor = 0;
                    self.user_buff.clear();
                    self.mode = VimMode::Normal;
                }
                _ => {}
            },
            VimMode::Marking => match key_event.code {
                KeyCode::Char(c) => {
                    self.user_buff.push(c);
                }
                KeyCode::Backspace => {
                    if self.user_buff.is_empty() {
                        return;
                    }
                    self.user_buff.pop();
                }
                KeyCode::Enter => {
                    if let Some(db) = &self.db {
                        let song_path = match self.viewer {
                            Page::Songs | Page::Search => {
                                let song_path = self.page_songs[self.cursor].song_path.clone();
                                add_playlist_labels(db, &song_path, &self.user_buff).unwrap();
                                song_path
                            }
                            _ => {
                                self.mode = VimMode::Normal;
                                self.user_buff.clear();
                                return;
                            }
                        };

                        let user_buff_clone = self.user_buff.clone();
                        let update_type = LocalTrackUpdateType::AddLabel(
                            SongPath(song_path.clone()),
                            Label(user_buff_clone),
                        );
                        self.update_song_label_local(update_type);
                    }
                    self.mode = VimMode::Normal;
                    self.user_buff.clear();
                }
                KeyCode::Esc => {
                    self.user_buff.clear();
                    self.mode = VimMode::Normal;
                }
                KeyCode::Delete => {
                    if let Some(db) = &self.db {
                        let song_path = match self.viewer {
                            Page::Songs | Page::Search => {
                                let song_path = self.page_songs[self.cursor].song_path.clone();
                                wipe_playlist_labels(db, &song_path).unwrap();
                                song_path
                            }
                            Page::Albums => {
                                self.user_buff.clear();
                                self.mode = VimMode::Normal;
                                return;
                            }
                        };

                        self.update_song_label_local(LocalTrackUpdateType::WipeLabels(
                            crate::types::SongPath(song_path),
                        ));
                    }
                    self.user_buff.clear();
                    self.mode = VimMode::Normal;
                }
                _ => {}
            },
            VimMode::VisualLine => match key_event.code {
                KeyCode::Char('j') => {
                    self.increment_counter();
                }
                KeyCode::Char('k') => {
                    self.decrement_counter();
                }
                KeyCode::Char('d') => {
                    // yank the songs from vline_begin to now
                    match self.viewer {
                        Page::Albums => {
                            unimplemented!()
                        }
                        Page::Songs | Page::Search => {
                            if let Some(begin) = self.vline_begin {
                                self.yank_buff.clear();
                                self.yank_buff.extend_from_slice(
                                    &self.page_songs[std::cmp::min(begin, self.cursor)
                                        ..std::cmp::max(self.cursor + 1, begin)],
                                );
                                self.page_songs.drain(
                                    std::cmp::min(begin, self.cursor)
                                        ..std::cmp::max(self.cursor + 1, begin),
                                );
                                self.cursor = std::cmp::min(begin, self.cursor);
                            }
                        }
                    }

                    self.mode = VimMode::Normal;
                }
                KeyCode::Char('y') => {
                    // yank the songs from vline_begin to now
                    match self.viewer {
                        Page::Albums => {
                            unimplemented!()
                        }
                        Page::Songs | Page::Search => {
                            if let Some(begin) = self.vline_begin {
                                self.yank_buff.clear();
                                self.yank_buff.extend_from_slice(
                                    &self.page_songs[std::cmp::min(begin, self.cursor)
                                        ..std::cmp::max(self.cursor + 1, begin)],
                                );
                            }
                        }
                    }

                    self.mode = VimMode::Normal;
                }
                KeyCode::Enter => {
                    self.parse_command();
                    self.user_buff.clear();
                    self.mode = VimMode::Normal;
                    self.viewer = Page::Albums;
                }
                KeyCode::Esc => {
                    self.user_buff.clear();
                    self.mode = VimMode::Normal;
                }
                _ => {}
            },
        };
    }

    fn parse_command(&mut self) {
        let buff_clone = self.user_buff.clone();
        let Some((cmd, arg)) = parse_command_args(&buff_clone) else {
            return;
        };
        self.run_command(cmd, arg);
    }

    fn run_command(&mut self, cmd: &str, arg: &str) {
        if cmd == "sort" {
            match arg {
                "last_played" => match self.viewer {
                    Page::Albums => self.page_albums.sort_by_key(|a| Reverse(a.stats.0)),
                    Page::Songs | Page::Search => {
                        self.page_songs.sort_by_key(|s| Reverse(s.stats.0))
                    }
                },
                "duration_played" => match self.viewer {
                    Page::Albums => self.page_albums.sort_by_key(|a| Reverse(a.stats.1)),
                    Page::Songs | Page::Search => {
                        self.page_songs.sort_by_key(|s| Reverse(s.stats.1))
                    }
                },
                "date_added" => match self.viewer {
                    Page::Albums => self.page_albums.sort_by_key(|a| Reverse(a.stats.2)),
                    Page::Songs | Page::Search => {
                        self.page_songs.sort_by_key(|s| Reverse(s.stats.2))
                    }
                },
                _ => {
                    self.page_albums.sort();
                }
            }
        }
    }

    fn filter_songs(&self) -> Vec<TrackDetails> {
        match self.viewer {
            Page::Songs => filter_tracks(&self.page_songs_unfiltered, &self.user_buff),
            Page::Search => {
                let real_buff = &self.user_buff[1..];
                filter_tracks(&self.all_songs_unfiltered, real_buff)
            }
            _ => Vec::new(),
        }
    }

    fn filter_albums(&self) -> Vec<Album> {
        filter_albums(&self.albums_unfiltered, &self.user_buff)
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    fn increment_counter(&mut self) {
        match self.viewer {
            Page::Albums => {
                self.cursor = clamp_cursor_increment(self.cursor, self.page_albums.len());
                self.albums_cursor = self.cursor;
            }
            Page::Songs | Page::Search => {
                self.cursor = clamp_cursor_increment(self.cursor, self.page_songs.len());
            }
        }
    }

    fn decrement_counter(&mut self) {
        if self.viewer == Page::Albums {
            self.albums_cursor = clamp_cursor_decrement(self.cursor);
        }
        self.cursor = clamp_cursor_decrement(self.cursor);
    }

    fn next_song(&mut self) {
        if self.song_queue.is_none() {
            return;
        }
        let len = self.song_queue.as_ref().unwrap().len();
        let Some(new_idx) = next_index(self.playing_song_idx.unwrap(), len) else {
            return;
        };

        self.playing_song_idx = Some(new_idx);
        let next_song: Option<TrackDetails> =
            self.song_queue.as_ref().unwrap().get(new_idx).cloned();

        self.playing_song = next_song;

        self.playback_sender
            .send(MusicStreamEvent::NewPlaylistEvent(
                self.song_queue.clone().unwrap(),
                new_idx,
            ))
            .expect("Could not send through channel");
    }

    fn prev_song(&mut self) {
        if self.song_queue.is_none() {
            return;
        }
        let Some(new_idx) = prev_index(self.playing_song_idx.unwrap()) else {
            return;
        };

        self.playing_song_idx = Some(new_idx);
        let prev_song: Option<TrackDetails> =
            self.song_queue.as_ref().unwrap().get(new_idx).cloned();

        self.playing_song = prev_song;

        self.playback_sender
            .send(MusicStreamEvent::NewPlaylistEvent(
                self.song_queue.clone().unwrap(),
                new_idx,
            ))
            .expect("Could not send through channel");
    }

    pub fn get_time_elapsed(&self) -> Duration {
        self.elapsed_before_paused + self.play_start.unwrap_or(Instant::now()).elapsed()
    }

    fn update_song_label_local(&mut self, update_type: LocalTrackUpdateType) {
        let path = match &update_type {
            LocalTrackUpdateType::DurationPlayed(s, _) => s,
            LocalTrackUpdateType::WipeLabels(s) => s,
            LocalTrackUpdateType::AddLabel(s, _) => s,
            LocalTrackUpdateType::LastPlayed(s) => s,
        };
        let song_path_str = path.0.clone();

        let (album_name, artist_name) = {
            let all_songs_ref = self
                .all_songs_unfiltered
                .iter()
                .find(|song| song.song_path == path.0)
                .unwrap();
            (all_songs_ref.album.clone(), all_songs_ref.artist.clone())
        };

        match update_type {
            LocalTrackUpdateType::DurationPlayed(song_path, time) => {
                apply_duration_played(
                    &mut self.all_songs_unfiltered,
                    &mut self.albums_unfiltered,
                    &song_path.0,
                    time,
                );

                match self.viewer {
                    Page::Search | Page::Songs => {
                        let song_ref = self
                            .page_songs
                            .iter_mut()
                            .find(|song| song.song_path == song_path.0);

                        if let Some(song_ref) = song_ref {
                            song_ref.stats.1 += time;
                        }
                    }
                    _ => {}
                }

                self.albums_unfiltered
                    .iter_mut()
                    .find(|alb| alb.album == album_name && alb.artist == artist_name)
                    .unwrap()
                    .stats
                    .1 += time;
            }
            LocalTrackUpdateType::WipeLabels(_) => {
                match self.viewer {
                    Page::Songs | Page::Search => self.page_songs[self.cursor].tags.clear(),
                    _ => {}
                }
                apply_wipe_labels(
                    &mut self.all_songs_unfiltered,
                    &mut self.albums_unfiltered,
                    &song_path_str,
                );
            }
            LocalTrackUpdateType::AddLabel(_, label) => {
                match self.viewer {
                    Page::Songs | Page::Search => {
                        self.page_songs[self.cursor]
                            .tags
                            .push(self.user_buff.clone());
                    }
                    _ => {}
                }
                apply_add_label(
                    &mut self.all_songs_unfiltered,
                    &mut self.albums_unfiltered,
                    &song_path_str,
                    &label.0,
                );
            }
            LocalTrackUpdateType::LastPlayed(_) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                apply_last_played(
                    &mut self.all_songs_unfiltered,
                    &mut self.albums_unfiltered,
                    &song_path_str,
                    now,
                );

                match self.viewer {
                    Page::Search | Page::Songs => self.page_songs[self.cursor].stats.0 = now,
                    _ => {}
                }

                self.albums_unfiltered
                    .iter_mut()
                    .find(|alb| alb.album == album_name && alb.artist == artist_name)
                    .unwrap()
                    .stats
                    .0 = now;
            }
        }
    }

    fn update_db(&mut self, update_type: DbUpdate) {
        let Some(database) = &self.db else {
            return;
        };

        match update_type {
            DbUpdate::DurationPlayed(song_path, duration) => {
                if let Err(e) = update_duration_played(database, &song_path, duration) {
                    panic!("Failed to update: {e}");
                }
            }
            DbUpdate::LastPlayed(song_path) => {
                if let Err(e) = update_last_played(database, &song_path) {
                    panic!("Failed to update: {e}");
                }
            }
        }
    }

    fn playback(&mut self) {
        let (track_sender, track_receiver) = channel::<MusicStreamEvent>();
        self.event_receiver = Some(track_receiver);
        let Some(receiver) = self.playback_receiver.take() else {
            return;
        };

        let mixer = self.audio_handle.mixer().clone();
        let playback_mode = Arc::clone(&self.playback_mode);
        let _thread_handle = thread::spawn(move || {
            let (mut playlist, mut track_no) = match receiver.recv() {
                Ok(MusicStreamEvent::NewPlaylistEvent(playlist, track_no)) => {
                    track_sender
                        .send(MusicStreamEvent::NewPlaylistEvent(
                            playlist.clone(),
                            track_no,
                        ))
                        .ok();
                    (playlist, track_no)
                }
                _ => return,
            };

            'song_loop: loop {
                let current_track = playlist[track_no].clone();
                let song_path = current_track.song_path;
                let file = File::open(song_path).unwrap();
                let decoder = rodio::Decoder::try_from(file).unwrap();
                let mut song_time_remaining = Duration::from_secs(current_track.duration);
                let player = rodio::Player::connect_new(&mixer);
                player.append(decoder);
                let mut is_paused = false;

                '_playback_loop: loop {
                    let now = Instant::now();

                    let event = if is_paused {
                        match receiver.recv() {
                            Ok(e) => Ok(e),
                            Err(_) => Err(RecvTimeoutError::Disconnected),
                        }
                    } else {
                        receiver.recv_timeout(song_time_remaining)
                    };

                    match event {
                        Ok(MusicStreamEvent::NewPlaylistEvent(new_playlist, new_idx)) => {
                            track_sender
                                .send(MusicStreamEvent::NewPlaylistEvent(
                                    new_playlist.clone(),
                                    new_idx,
                                ))
                                .ok();

                            std::mem::drop(player);
                            playlist = new_playlist;
                            track_no = new_idx;
                            continue 'song_loop;
                        }
                        Ok(MusicStreamEvent::PlaybackEvent(mode, _)) => match mode {
                            PlaybackMode::Paused => {
                                player.pause();
                                is_paused = true;
                                song_time_remaining =
                                    song_time_remaining.saturating_sub(now.elapsed());
                            }
                            PlaybackMode::Playing => {
                                player.play();
                                is_paused = false;
                            }
                            _ => {}
                        },
                        Ok(MusicStreamEvent::TrackAutoAdvanced(_, _)) => {
                            unimplemented!();
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            // played playlist to it's conclusion
                            // clean up app state
                            if track_no == playlist.len() - 1 {
                                let mut state = playback_mode.lock().unwrap();
                                *state = PlaybackMode::NotPlaying;
                            }
                            // if we are not at the last song
                            // in the playlist, play the next song.
                            else {
                                std::mem::drop(player);
                                track_no += 1;
                                track_sender
                                    .send(MusicStreamEvent::TrackAutoAdvanced(
                                        playlist[track_no].clone(),
                                        track_no,
                                    ))
                                    .ok();
                                continue 'song_loop;
                            }
                        }
                        Err(_) => {}
                    }
                }
            }
        });
    }

    fn _hash_art(&self, song: &Vec<u8>) -> u64 {
        let mut s = DefaultHasher::new();
        song.hash(&mut s);
        s.finish()
    }

    fn preload(&mut self, album: Album) {
        let picker = self.image_picker.clone();
        let cache_handle = Arc::clone(&self.cache);

        let _handle = thread::spawn(move || {
            if album.songs.is_empty() || cache_handle.lock().unwrap().contains(&album) {
                return;
            }
            let Some(new_song_art) = get_song_art(&album.songs[0]) else {
                return;
            };

            if let Ok(img) = image::load_from_memory(&new_song_art) {
                let protocol = picker.new_resize_protocol(img);
                let rc_ptr = Arc::new(RwLock::new(protocol));
                cache_handle
                    .lock()
                    .unwrap()
                    .put(album, (new_song_art, rc_ptr.clone()));
            }
        });
    }

    fn update_curr_song_art(&mut self) {
        let possible_curr_album = self.album_selected.clone();
        let picker = self.image_picker.clone();
        let playing_song = self.playing_song.clone();
        let cover_art_handle = Arc::clone(&self.cover_art);
        let cache_handle = Arc::clone(&self.cache);
        thread::spawn(move || {
            // before we try to find the song art, is it in the cache?
            if possible_curr_album.is_some()
                && let Some((bytes, protocol)) = cache_handle
                    .lock()
                    .unwrap()
                    .get(possible_curr_album.as_ref().unwrap())
            {
                // it's in the cache
                *cover_art_handle.lock().unwrap() = Some((bytes.clone(), (*protocol).clone()));
                return;
            }

            // couldn't find the art in the cache
            let Some(new_song_art) = get_song_art(playing_song.as_ref().unwrap()) else {
                return;
            };
            if let Ok(img) = image::load_from_memory(&new_song_art) {
                let protocol = picker.new_resize_protocol(img);
                let rc_ptr = Arc::new(RwLock::new(protocol));
                *cover_art_handle.lock().unwrap() = Some((new_song_art.clone(), rc_ptr.clone()));
                if let Some(album) = possible_curr_album.clone() {
                    cache_handle
                        .lock()
                        .unwrap()
                        .put(album, (new_song_art, rc_ptr.clone()));
                }
            }
        });
    }
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
