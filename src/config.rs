use dirs;
use serde::{Deserialize};


#[derive(Deserialize, Debug)]
pub struct Config {
    pub music_path: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            music_path: dirs::home_dir()
                .expect("could not find home dir")
                .join("Music")
                .to_string_lossy()
                .to_string(),
        }
    }
}

pub fn config_init() -> Config {
    let config_path = dirs::home_dir()
        .expect("Could not find home directory")
        .join(".config/bragi/conf.toml");

    std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}
