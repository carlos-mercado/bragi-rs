mod app;
mod app_utils;
mod types;
mod ui;

fn main() -> std::io::Result<()> {
    ratatui::run(|terminal| app::App::new().run(terminal))
}
