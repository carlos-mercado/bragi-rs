/*

use std::fs::{File};
use dirs;
//use serde_json::{Result, Value};


pub fn init() {
    let config_path = dirs::home_dir()
        .expect("Could not find home directory")
        .join(".config/bragi/conf.toml");

    let _config_file = match File::open(&config_path) {
        Ok(file) => file,
        _ => File::create_new(&config_path).unwrap(),
    };
}
*/
