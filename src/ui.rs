use music::TrackDetails;
use ratatui::prelude::Text;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect, Margin},
    style::{Color, Style},
    symbols,
    widgets::{Block, LineGauge, List, ListState, Paragraph, StatefulWidget, Widget},
};
use std::sync::Arc;
use crate::app::App;
use crate::types::{PlaybackMode, VimMode};
use crate::types::{Page};

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [selection_area, lower_area] =
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);

        let [info_area, progress_bar_area] =
            Layout::vertical([Constraint::Percentage(90), Constraint::Percentage(10)])
                .areas(lower_area);

        let mut selection_state = ListState::default().with_selected(Some(self.cursor));
        let music_preview = Block::bordered().title_top("Now Playing");

        let binding = Arc::clone(&self.playback_mode);
        let state = binding.lock().unwrap();
        if let Some(selected_track) = &self.playing_song {
            if *state == PlaybackMode::Playing || *state == PlaybackMode::Paused {
                Paragraph::new(Text::from(selected_track))
                    .centered()
                    .block(music_preview)
                    .render(info_area, buf);
            }
        }
        std::mem::drop(state);

        let list_title = match self.mode {
            VimMode::Normal => String::from("Playlist"),
            VimMode::Search => format!("Searching: {}", self.search_buff),
        };

        let music_selection;

        if self.mode == VimMode::Search {
            music_selection = List::new(&self.songs)
                .block(Block::bordered().title_top(list_title))
                .style(ratatui::style::Style::default().fg(Color::White))
                .highlight_style(Style::new().italic().bold())
                .highlight_symbol(">>");

        }
        else {
            match self.viewer {
                Page::AlbumsView => {
                    music_selection = List::new(&self.albums)
                        .block(Block::bordered().title_top(list_title))
                        .style(ratatui::style::Style::default().fg(Color::White))
                        .highlight_style(Style::new().italic().bold())
                        .highlight_symbol(">>");
                }
                Page::SongsView => {
                    let songs: Vec<TrackDetails> = self.album_selected.as_ref().unwrap().songs.clone();

                    music_selection = List::new(&songs)
                        .block(Block::bordered().title_top(list_title))
                        .style(ratatui::style::Style::default().fg(Color::White))
                        .highlight_style(Style::new().italic().bold())
                        .highlight_symbol(">>");
                }
                Page::SearchView => {
                    let songs: Vec<TrackDetails> = self.playlist_selected.as_ref().unwrap().clone();

                    music_selection = List::new(&songs)
                        .block(Block::bordered().title_top(list_title))
                        .style(ratatui::style::Style::default().fg(Color::White))
                        .highlight_style(Style::new().italic().bold())
                        .highlight_symbol(">>");

                }
            }
        }

        let binding = Arc::clone(&self.playback_mode);
        let playback_state = binding.lock().unwrap();
        if *playback_state == PlaybackMode::Playing || *playback_state == PlaybackMode::Paused {
            std::mem::drop(playback_state);
            let centered_progress_area = progress_bar_area.inner(Margin {
                horizontal: 4,
                vertical: 0,
            });
            let progress_bar = LineGauge::default()
                .filled_style(Style::new().cyan().on_black().bold())
                .unfilled_style(Style::new().dark_gray().on_black())
                .label("")
                .filled_symbol(symbols::line::THICK_HORIZONTAL)
                .unfilled_symbol(symbols::line::THICK_HORIZONTAL)
                .ratio(
                    (self.get_time_elapsed().as_secs_f64()
                        / self.playing_song.as_ref().unwrap().duration as f64)
                        .min(1.0)
                );
            progress_bar.render(centered_progress_area, buf);
        }

        StatefulWidget::render(music_selection, selection_area, buf, &mut selection_state);
    }
}
