use crate::app::App;
use crate::types::Page;
use crate::types::{PlaybackMode, VimMode};
use music::get_song_art;
use ratatui::prelude::Text;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Style},
    symbols,
    widgets::{Block, LineGauge, List, ListItem, ListState, Paragraph, StatefulWidget, Widget},
};
use ratatui_image::{Resize, StatefulImage};
use std::sync::Arc;

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [selection_area, lower_area] =
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);

        let [info_area, album_art_area, progress_bar_area] = Layout::vertical([
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
        ])
        .areas(lower_area);

        let mut selection_state = ListState::default().with_selected(Some(self.cursor));
        let music_preview = Block::bordered().title_top("Now Playing");
        let binding = Arc::clone(&self.playback_mode);
        let state = binding.lock().unwrap();

        // display cover art for a song (if it exists)
        if let Some(selected_track) = &self.playing_song
            && (*state == PlaybackMode::Playing || *state == PlaybackMode::Paused)
        {
            Paragraph::new(Text::from(selected_track))
                .centered()
                .block(music_preview)
                .render(info_area, buf);

            let cover_path = get_song_art(selected_track);

            // the cover art exists
            if let Some(art_bytes) = cover_path {
                let mut cache = self.cover_cache.borrow_mut();

                // does it match the image in the cache?
                let needs_reload = match cache.as_ref() {
                    None => true,
                    Some((cached_art_bytes, _)) => cached_art_bytes != &art_bytes,
                };

                // this is a new cover image
                // replace the one in the cache.
                if needs_reload && let Ok(img) = image::load_from_memory(&art_bytes) {
                    let protocol = self.image_picker.new_resize_protocol(img);
                    *cache = Some((art_bytes, protocol));
                }

                if let Some((_, protocol)) = cache.as_mut() {
                    let [_left, centered, _right] = Layout::horizontal([
                        Constraint::Percentage(40),
                        Constraint::Percentage(20),
                        Constraint::Percentage(40),
                    ])
                    .areas(album_art_area);

                    let image_widget = StatefulImage::default().resize(Resize::Fit(None));
                    StatefulWidget::render(image_widget, centered, buf, protocol);
                }
            }
        }
        std::mem::drop(state);

        let list_title = match self.mode {
            VimMode::Normal => String::from("Playlist"),
            VimMode::Search => format!("Searching: {}", self.user_buff),
            VimMode::Command => format!("cmd: {}", self.user_buff),
            VimMode::Marking => format!("Marking: {}", self.user_buff),
            VimMode::VisualLine => "V-LINE".to_string(),
        };

        let music_selection: List = match self.viewer {
            Page::Albums => {
                let items: Vec<ListItem> = self
                    .albums
                    .iter()
                    .enumerate()
                    .map(|(i, song)| {
                        if self.mode == VimMode::VisualLine
                            && ((i >= self.vline_begin.unwrap() && i <= self.cursor)
                                || (i <= self.vline_begin.unwrap() && i >= self.cursor))
                        {
                            ListItem::new(song).style(Style::default().bg(Color::LightBlue))
                        } else {
                            ListItem::new(song)
                        }
                    })
                    .collect();

                List::new(items)
                    .block(Block::bordered().title_top(list_title))
                    .style(Style::default().fg(Color::White))
                    .highlight_style(Style::new().italic().bold())
                    .highlight_symbol(">>")
            }
            Page::Songs | Page::Search => {
                let items: Vec<ListItem> = self
                    .page_songs
                    .iter()
                    .enumerate()
                    .map(|(i, song)| {
                        if self.mode == VimMode::VisualLine
                            && ((i >= self.vline_begin.unwrap() && i <= self.cursor)
                                || (i <= self.vline_begin.unwrap() && i >= self.cursor))
                        {
                            ListItem::new(song).style(Style::default().bg(Color::LightBlue))
                        } else {
                            ListItem::new(song)
                        }
                    })
                    .collect();

                List::new(items)
                    .block(Block::bordered().title_top(list_title))
                    .style(Style::default().fg(Color::White))
                    .highlight_style(Style::new().italic().bold())
                    .highlight_symbol(">>")
            }
        };

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
                        .min(1.0),
                );
            progress_bar.render(centered_progress_area, buf);
        }

        StatefulWidget::render(music_selection, selection_area, buf, &mut selection_state);
    }
}
