pub mod api;
pub mod config;
pub mod cover;
#[cfg(target_os = "linux")]
pub mod desktop;
pub mod discord;
pub mod game;
pub mod gameicon;
pub mod log;
pub mod prefs;
pub mod remember;
pub mod selfupdate;
#[cfg(windows)]
mod tray;
pub mod ui;
pub mod update;
