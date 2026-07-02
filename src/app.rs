use std::fs::File;
use std::{io};
use std::io::BufReader;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::DefaultTerminal;
use ratatui::Frame;

use music::{Album, TrackDetails, build_albums, filter_tracks, get_music_files };
use redb::Database;
use crate::config::config_init;
use crate::types::{MusicStreamEvent, PlaybackMode, VimMode, Page};
use crate::db::*;

/**/
pub struct App {
    pub exit: bool,
    pub cursor: usize,
    pub unfiltered_songs: Vec<TrackDetails>, // this won't change throughout the program.
    pub songs: Vec<TrackDetails>, // this will change during search mode. 
    pub albums: Vec<Album>,
    pub playing_song: Option<TrackDetails>,
    pub playing_song_idx: Option<usize>,
    pub album_selected: Option<Album>,
    pub playlist_selected: Option<Vec<TrackDetails>>,

    pub play_start: Option<Instant>,
    pub elapsed_before_paused: Duration,
    pub sender: Sender<(MusicStreamEvent, usize)>,
    pub receiver: Option<Receiver<(MusicStreamEvent, usize)>>,
    pub playback_mode: Arc<Mutex<PlaybackMode>>,
    pub audio_handle: rodio::MixerDeviceSink,
    pub search_buff: String,
    pub last_key: char,
    pub key_pressed_time: Instant,
    pub playback_event_receiver: Option<Receiver<MusicStreamEvent>>,
    pub db: Option<Database>,

    pub mode: VimMode,
    pub viewer: Page,
}

impl App {
    pub fn new() -> App {
        let config = config_init();
        let db = db_setup();

        let mut songs_vec: Vec<TrackDetails> = vec![];
        get_music_files(Path::new(&config.music_path), &mut songs_vec, &db).unwrap();
        songs_vec.sort();
        let songs_vec_clone = songs_vec.clone();
        let albums = build_albums(&songs_vec_clone);
        let audio_handle = rodio::DeviceSinkBuilder::open_default_sink()
            .expect("Could not find default audio stream");
        let (sender, receiver): (Sender<(MusicStreamEvent, usize)>, Receiver<(MusicStreamEvent, usize)>) = channel();
        let play_start = None;
        let elapsed_before_paused = Duration::from_secs(0);
        let playback_mode = Arc::new(Mutex::new(PlaybackMode::NotPlaying));

        let mut app = App {
            cursor: 0,
            exit: false,
            songs: songs_vec,
            albums,
            playing_song: None,
            playing_song_idx: None,
            album_selected: None,
            play_start,
            elapsed_before_paused,
            sender,
            receiver: Some(receiver),
            mode: VimMode::Normal,
            playback_mode,
            audio_handle,
            search_buff: String::new(),
            unfiltered_songs: songs_vec_clone,
            last_key: ' ',
            key_pressed_time: Instant::now(),
            playback_event_receiver: None,
            db,
            playlist_selected: None,
            viewer: Page::AlbumsView,
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
        if let Some(ref rx) = self.playback_event_receiver {
            if let Ok(MusicStreamEvent::TrackAutoAdvanced(i)) = rx.try_recv() {
                self.play_start = Some(Instant::now());
                self.elapsed_before_paused = Duration::from_secs(0);
                self.playing_song = Some(i);
                self.playing_song_idx = Some(self.playing_song_idx.unwrap() + 1);
            }
        }

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
        if self.mode == VimMode::Normal {
            match key_event.code {
                KeyCode::Char('q') => self.exit(),
                KeyCode::Char('j') => self.increment_counter(),
                KeyCode::Char('k') => self.decrement_counter(),
                KeyCode::Char('h') => self.prev_song(),
                KeyCode::Char('l') => self.next_song(),
                KeyCode::Char('/') => self.mode = VimMode::Search,
                KeyCode::Char('g') => {
                    let timeout = Duration::from_millis(300);
                    if self.last_key == 'g' && self.key_pressed_time.elapsed() < timeout {
                        self.cursor = 0;
                        self.last_key = ' ';
                    } else {
                        self.last_key = 'g';
                        self.key_pressed_time = Instant::now();
                    }
                }
                KeyCode::Char('G') => {
                    if self.album_selected == None {
                        self.cursor = self.albums.len() - 1;
                    }
                    else {
                        self.cursor = self.album_selected.as_ref().unwrap().songs.clone().len() - 1;
                    }
                }
                KeyCode::Char('p') => {
                    let binding = Arc::clone(&self.playback_mode);
                    let mut state = binding.lock().unwrap();
                    match *state {
                        PlaybackMode::NotPlaying => return,
                        PlaybackMode::Playing => {
                            *state = PlaybackMode::Paused;
                            self.sender
                                .send(( MusicStreamEvent::PlaybackEvent(PlaybackMode::Paused), 0 ))
                                .expect("Could not send through channel");
                            self.elapsed_before_paused += self.play_start.unwrap().elapsed();
                        }
                        PlaybackMode::Paused => {
                            *state = PlaybackMode::Playing;
                            self.sender
                                .send(( MusicStreamEvent::PlaybackEvent(PlaybackMode::Playing), 0 ))
                                .expect("Could not send through channel");
                            self.play_start = Some(Instant::now());
                        }
                    }
                }
                KeyCode::Esc => {
                    self.search_buff.clear();
                    self.mode = VimMode::Normal;
                    self.songs = self.unfiltered_songs.clone();
                    self.album_selected = None;
                    self.viewer = Page::AlbumsView;
                }
                KeyCode::Enter => {
                    if self.songs.is_empty() || self.albums.is_empty() { return; }

                    match self.viewer {
                        Page::AlbumsView => {
                            self.album_selected = Some(self.albums[self.cursor].clone());
                            self.viewer = Page::SongsView;
                            self.cursor = 0;
                        },
                        _ => {
                            // user chose a song
                            self.playing_song_idx = Some(self.cursor) ;
                            let binding = Arc::clone(&self.playback_mode);
                            let mut state = binding.lock().unwrap();
                            self.playlist_selected = match &self.album_selected {
                                Some(album) => Some( album.songs.clone() ),
                                None => Some( self.songs.clone() ),
                            };

                            self.playing_song = Some( self.playlist_selected.as_ref().unwrap()[self.playing_song_idx.unwrap()].clone() );
                            self.sender
                                .send(( MusicStreamEvent::NewPlaylistEvent(self.playlist_selected.as_ref().unwrap().clone()), self.cursor))
                                .expect("Could not send through channel");
                            *state = PlaybackMode::Playing;

                            // update last_played time
                            if let Some(database) = &self.db {
                                let path = self.playing_song.as_ref().unwrap().song_path.as_str();
                                if let Err(e) = update(database, path) {
                                    eprintln!("Failed to update: {e}");
                                }
                            }

                            self.play_start = Some(Instant::now());
                            self.elapsed_before_paused = Duration::from_secs(0);
                        }
                    }
                }
                _ => {}
            }
        } else {
            match key_event.code {
                KeyCode::Char(c) => {
                    self.search_buff.push(c);
                    self.songs = self.filter_songs();
                    self.cursor = 0;
                }
                KeyCode::Backspace => {
                    if self.search_buff.is_empty() { return; }

                    self.search_buff.pop();
                    self.songs = self.filter_songs();
                    self.cursor = 0;
                }
                KeyCode::Enter => {
                    self.search_buff.clear();
                    self.mode = VimMode::Normal;
                    self.playlist_selected = Some(self.songs
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                    );
                    self.viewer = Page::SearchView;
                }
                KeyCode::Esc => {
                    self.cursor = 0;
                    self.search_buff.clear();
                    self.mode = VimMode::Normal;
                    self.songs = self.unfiltered_songs.clone();
                    self.viewer = Page::AlbumsView;
                }
                _ => {}
            }
        }
    }

    fn filter_songs(&self) -> Vec<TrackDetails> {
        filter_tracks(&self.unfiltered_songs, &self.search_buff)
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    fn increment_counter(&mut self) {
        match self.viewer {
            Page::AlbumsView => {
                self.cursor = std::cmp::min(self.albums.len() - 1, self.cursor + 1);
            },
            Page::SongsView => {
                self.cursor = std::cmp::min(self.album_selected.as_ref().unwrap().songs.len() - 1, self.cursor + 1);
            },
            Page::SearchView => {
                self.cursor = std::cmp::min(self.playlist_selected.as_ref().unwrap().len() - 1, self.cursor + 1);
            },
        }
    }

    fn decrement_counter(&mut self) {
        self.cursor = std::cmp::max(0_i32, self.cursor as i32 - 1) as usize;
    }

    fn next_song(&mut self) {
        if self.playlist_selected == None {
            return;
        }

        let len = self.playlist_selected.as_ref().unwrap().len();
        if self.playing_song_idx.unwrap() + 1 >= len {
            return;
        }

        self.playing_song_idx = Some(self.playing_song_idx.unwrap() + 1);
        let next_song : Option<TrackDetails> = self.playlist_selected
                            .as_ref()
                            .unwrap()
                            .get(self.playing_song_idx.unwrap())
                            .cloned();
        self.playing_song = next_song;

        self.sender
            .send(( MusicStreamEvent::NewPlaylistEvent(self.playlist_selected.clone().unwrap()), self.playing_song_idx.unwrap()))
            .expect("Could not send through channel");  
    }

    fn prev_song(&mut self) {
        if self.album_selected == None { return; }
        if self.playing_song == None { return; }
        if self.playing_song_idx == None { return; }
        if self.playing_song_idx.unwrap() as i32 - 1 < 0 {
            return; 
        }

        self.playing_song_idx = Some(self.playing_song_idx.unwrap() - 1);
        let prev_song : Option<TrackDetails> = self.playlist_selected
                            .as_ref()
                            .unwrap()
                            .get(self.playing_song_idx.unwrap())
                            .cloned();

        self.playing_song = prev_song;

        self.sender
            .send(( MusicStreamEvent::NewPlaylistEvent(self.playlist_selected.clone().unwrap()), self.playing_song_idx.unwrap()))
            .expect("Could not send through channel");  
    }

    pub fn get_time_elapsed(&self) -> Duration {
        self.elapsed_before_paused + self.play_start.unwrap_or(Instant::now()).elapsed()
    }

    fn playback(&mut self) {
        let (track_sender, track_receiver) = channel::<MusicStreamEvent>();
        self.playback_event_receiver = Some(track_receiver);

        let Some(receiver) = self.receiver.take() else {
            return;
        };
        let mixer = self.audio_handle.mixer().clone();
        let playback_mode = Arc::clone(&self.playback_mode);

        let _thread_handle = thread::spawn(move || {
            let ( mut playlist, mut track_no ) = match receiver.recv() {
                Ok( 
                    ( MusicStreamEvent::NewPlaylistEvent(playlist), track_no ) 
                ) => ( playlist, track_no ),
                _ => return,
            };

            'song_loop : loop {
                let current_track = playlist[track_no].clone();
                let song_path = current_track.song_path;
                let file = BufReader::new(File::open(song_path).unwrap());
                let mut song_time_remaining = Duration::from_secs(current_track.duration);
                let player = rodio::play(&mixer, file).unwrap();
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
                        Ok((MusicStreamEvent::NewPlaylistEvent(new_playlist), new_idx)) => {
                            std::mem::drop(player);
                            playlist = new_playlist;
                            track_no = new_idx;
                            continue 'song_loop;
                        },
                        Ok((MusicStreamEvent::PlaybackEvent(mode), _)) => match mode {
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
                        Ok((MusicStreamEvent::TrackAutoAdvanced(_), _)) => {
                            todo!()
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            // played playlist to it's conclusion
                            // clean up app state
                            if track_no == playlist.len() - 1 {
                                let mut state = playback_mode.lock().unwrap();
                                *state = PlaybackMode::NotPlaying;
                            }
                            // if there is we are not at the last song
                            // in the playlist, play the next song.
                            else {
                                std::mem::drop(player);
                                track_no += 1;
                                track_sender.send(MusicStreamEvent::TrackAutoAdvanced(playlist[track_no].clone())).ok();
                                continue 'song_loop;
                            }
                        },
                        Err(_) => {},
                    }
                }
            }
        });
    }
}
// self.elapsed_before_paused + self.play_start.unwrap_or(Instant::now()).elapsed()
