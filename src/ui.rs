use crate::app::App;
use crate::types::Page;
use crate::types::{PlaybackMode, VimMode};
use ratatui::prelude::Text;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect, Size},
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

        let [top_area, progress_bar_area] =
            Layout::vertical([Constraint::Percentage(90), Constraint::Percentage(10)])
                .areas(lower_area);

        let mut selection_state = ListState::default().with_selected(Some(self.cursor));
        let music_preview = Block::bordered().title_top("Now Playing");
        let inner_top_area = music_preview.inner(top_area);
        let [info_area, album_art_area] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(inner_top_area);

        let binding = Arc::clone(&self.playback_mode);
        let state = binding.lock().unwrap();

        // display cover art for a song (if it exists)
        if let Some(selected_track) = &self.playing_song
            && (*state == PlaybackMode::Playing || *state == PlaybackMode::Paused)
        {
            music_preview.render(top_area, buf);

            let info_text = Text::from(selected_track.clone());
            let text_height = info_text.lines.len() as u16;
            let info_paragraph = Paragraph::new(info_text).centered();
            let [_top, centered_info, _bottom] = Layout::vertical([
                Constraint::Fill(1),
                Constraint::Length(text_height),
                Constraint::Fill(1),
            ])
            .areas(info_area);
            info_paragraph.render(centered_info, buf);

            if let Some((_bytes, protocol)) = &*self.cover_art.lock().unwrap() {
                let resize = Resize::Fit(None);
                let rendered_size = protocol.read().unwrap().size_for(
                    resize.clone(),
                    Size::new(album_art_area.width, album_art_area.height),
                );

                let centered = Rect {
                    x: album_art_area.x
                        + album_art_area.width.saturating_sub(rendered_size.width) / 2,
                    y: album_art_area.y
                        + album_art_area.height.saturating_sub(rendered_size.height) / 2,
                    width: rendered_size.width.min(album_art_area.width),
                    height: rendered_size.height.min(album_art_area.height),
                };

                let image_widget = StatefulImage::default().resize(resize);
                StatefulWidget::render(
                    image_widget,
                    centered,
                    buf,
                    &mut *protocol.write().unwrap(),
                );
            };
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
