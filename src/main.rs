mod app;
mod config;
mod types;
mod ui;

fn main() -> std::io::Result<()> {
    ratatui::run(|terminal| app::App::new().run(terminal))
}
