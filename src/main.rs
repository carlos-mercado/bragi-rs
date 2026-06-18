mod app;
mod types;
mod ui;
mod config;
mod db;

fn main() -> std::io::Result<()> {
    ratatui::run(|terminal| app::App::new().run(terminal))
}
