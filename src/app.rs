use std::fs::File;
use std::io;
use std::io::BufReader;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use std::collections::{BTreeSet};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::DefaultTerminal;
use ratatui::Frame;

use music::{TrackDetails, Album, build_albums, filter_tracks, get_music_files};
use crate::types::{MusicStreamEvent, PlaybackMode, VimMode};

pub struct App {
    pub counter: u32,
    pub exit: bool,
    pub songs: Vec<TrackDetails>,
    pub albums: Vec<Album>,
    pub song_selected: Option<TrackDetails>,
    pub album_selected: Option<BTreeSet<TrackDetails>>,
    pub play_start: Option<Instant>,
    pub elapsed_before_paused: Duration,
    pub sender: Sender<MusicStreamEvent>,
    pub receiver: Option<Receiver<MusicStreamEvent>>,
    pub mode: VimMode,
    pub playback_mode: Arc<Mutex<PlaybackMode>>,
    pub audio_handle: rodio::MixerDeviceSink,
    pub search_buff: String,
    pub unfiltered_songs: Vec<TrackDetails>,
    pub last_key: char,
    pub key_pressed_time: Instant,
}

impl App {
    pub fn new() -> App {
        let mut songs_vec: Vec<TrackDetails> = vec![];
        get_music_files(Path::new("/home/carlos/Music"), &mut songs_vec).unwrap();
        songs_vec.sort();
        let songs_vec_clone = songs_vec.clone();
        let albums = build_albums(&songs_vec_clone);
        let audio_handle = rodio::DeviceSinkBuilder::open_default_sink()
            .expect("Could not find default audio stream");
        let (sender, receiver): (Sender<MusicStreamEvent>, Receiver<MusicStreamEvent>) = channel();
        let play_start = None;
        let elapsed_before_paused = Duration::from_secs(0);
        let playback_mode = Arc::new(Mutex::new(PlaybackMode::NotPlaying));

        let mut app = App {
            counter: 0,
            exit: false,
            songs: songs_vec,
            albums,
            song_selected: None,
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
                KeyCode::Char('/') => self.mode = VimMode::Search,
                KeyCode::Char('g') => {
                    let timeout = Duration::from_millis(300);
                    if self.last_key == 'g' && self.key_pressed_time.elapsed() < timeout {
                        self.counter = 0;
                        self.last_key = ' ';
                    } else {
                        self.last_key = 'g';
                        self.key_pressed_time = Instant::now();
                    }
                }
                KeyCode::Char('G') => {
                    if self.album_selected == None {
                        self.counter = (self.albums.len() - 1) as u32;
                    }
                    else {
                        self.counter = (self.album_selected.clone().unwrap().len() - 1) as u32;
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
                                .send(MusicStreamEvent::PlaybackEvent(PlaybackMode::Paused))
                                .expect("Could not send through channel");
                            self.elapsed_before_paused += self.play_start.unwrap().elapsed();
                        }
                        PlaybackMode::Paused => {
                            *state = PlaybackMode::Playing;
                            self.sender
                                .send(MusicStreamEvent::PlaybackEvent(PlaybackMode::Playing))
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
                }
                KeyCode::Enter => {
                    if self.songs.is_empty() || self.albums.is_empty() { return; }

                    if self.album_selected == None {
                        self.album_selected = Some(self.albums[self.counter as usize].clone().songs);
                        self.counter = 0;
                    }
                    else {
                        // user chose a song
                        let binding = Arc::clone(&self.playback_mode);
                        let mut state = binding.lock().unwrap();
                        self.song_selected = self.album_selected
                            .as_ref()
                            .and_then(|set| set
                                .iter()
                                .nth(self.counter as usize)
                                .cloned()
                            );

                        let selected = self.song_selected.clone();
                        self.sender
                            .send(MusicStreamEvent::NewSongEvent(selected.unwrap()))
                            .expect("Could not send through channel");
                        *state = PlaybackMode::Playing;
                        self.play_start = Some(Instant::now());
                        self.elapsed_before_paused = Duration::from_secs(0);
                    }

                }
                _ => {}
            }
        } else {
            match key_event.code {
                KeyCode::Char(c) => {
                    self.search_buff.push(c);
                    self.songs = self.filter_songs();
                    self.counter = 0;
                }
                KeyCode::Backspace => {
                    if self.search_buff.is_empty() {
                        return;
                    }
                    self.search_buff.pop();
                    self.songs = self.filter_songs();
                    self.counter = 0;
                }
                KeyCode::Enter => {
                    self.search_buff.clear();
                    self.mode = VimMode::Normal;
                    self.album_selected = Some(self.songs
                        .iter()
                        .cloned()
                        .collect::<BTreeSet<_>>()
                        );
                }
                KeyCode::Esc => {
                    self.counter = 0;
                    self.search_buff.clear();
                    self.mode = VimMode::Normal;
                    self.songs = self.unfiltered_songs.clone();
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
        if self.album_selected == None {
            self.counter = std::cmp::min((self.albums.len() - 1) as u32, self.counter + 1);
        }
        else {
            self.counter = std::cmp::min((self.songs.len() - 1) as u32, self.counter + 1);
        }
    }

    fn decrement_counter(&mut self) {
        self.counter = std::cmp::max(0_i32, self.counter as i32 - 1) as u32;
    }

    pub fn get_time_elapsed(&self) -> Duration {
        self.elapsed_before_paused + self.play_start.unwrap_or(Instant::now()).elapsed()
    }

    fn playback(&mut self) {
        let Some(receiver) = self.receiver.take() else {
            return;
        };
        let mixer = self.audio_handle.mixer().clone();
        let binding = Arc::clone(&self.playback_mode);

        let _thread_handle = thread::spawn(move || {
            let mut current_track = match receiver.recv() {
                Ok(MusicStreamEvent::NewSongEvent(track_info)) => track_info,
                Ok(MusicStreamEvent::PlaybackEvent(_)) => return,
                Err(_) => return,
            };

            loop {
                let song_path = current_track.song_path.clone();
                let file = BufReader::new(File::open(song_path).unwrap());
                let mut song_time_remaining = Duration::from_secs(current_track.duration);
                let player = rodio::play(&mixer, file).unwrap();

                let mut is_paused = false;
                loop {
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
                        Ok(MusicStreamEvent::NewSongEvent(new_track_info)) => {
                            std::mem::drop(player);
                            current_track = new_track_info;
                            break;
                        }
                        Ok(MusicStreamEvent::PlaybackEvent(mode)) => match mode {
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
                        Err(RecvTimeoutError::Timeout) => {
                            // played song to it's conclusion
                            // clean up app state
                            let mut state = binding.lock().unwrap();
                            *state = PlaybackMode::NotPlaying;
                        }
                        Err(_) => {}
                    }
                }
            }
        });
    }
}
// self.elapsed_before_paused + self.play_start.unwrap_or(Instant::now()).elapsed()
