use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const AUTH_URL: &str = "https://prod.portalworldsgame.com";
pub const GAME_ADDRESS: &str = "prod.portalworldsgame.com:10001";
#[cfg(windows)]
pub const FILES_URL: &str = "https://raw.githubusercontent.com/Project-Reboot/Release/{ref}/PC/";
#[cfg(target_os = "macos")]
pub const FILES_URL: &str = "https://raw.githubusercontent.com/Project-Reboot/Release/{ref}/Mac/";
#[cfg(not(any(windows, target_os = "macos")))]
pub const FILES_URL: &str = "https://raw.githubusercontent.com/Project-Reboot/Release/{ref}/Linux/";
#[cfg(windows)]
pub const PLATFORM: &str = "Windows";
#[cfg(target_os = "macos")]
pub const PLATFORM: &str = "macOS";
#[cfg(not(any(windows, target_os = "macos")))]
pub const PLATFORM: &str = "Linux";
pub const LAUNCHER_LIST_URL: &str = "https://raw.githubusercontent.com/Project-Reboot/Release/{ref}/";
pub const FILES_REF_URL: &str = "https://api.github.com/repos/Project-Reboot/Release/commits/main";
pub const FILES_FALLBACK_REF: &str = "main";
pub const DISCORD_CLIENT_ID: &str = "994736498819731466";
pub const DISCORD_AUTHORIZE_URL: &str = "https://discord.com/oauth2/authorize";
pub const REDIRECT_PORTS: [u16; 5] = [27654, 31827, 38219, 41593, 45761];
#[cfg(windows)]
pub const GAME_PREFS: &str = r"Software\RebootTeam\Project Reboot";
#[cfg(target_os = "macos")]
pub const GAME_PREFS: &str = "unity.RebootTeam.Project Reboot";
#[cfg(not(any(windows, target_os = "macos")))]
pub const GAME_PREFS: &str = "unity3d/RebootTeam/Project Reboot/prefs";
pub const MANIFEST_PUBLIC_KEY: &str = "7bb9fabe872a2838416c25207c0e2dab3f99ec44c4ed4044e93913333e768d69";
pub const DISCORD_URL: &str = "https://discord.gg/F35p8uwFzY";
pub const YOUTUBE_URL: &str = "https://www.youtube.com/@ProjectRebootPW";
pub const GAME_NEWS_URL: &str = "https://raw.githubusercontent.com/Airbronze/RebootProdNews/master/Assets/news_EN.bson";
pub const PLAYLIST_FEED_URL: &str = "https://www.youtube.com/feeds/videos.xml?playlist_id=";
pub const OFFICIAL_PLAYLIST: &str = "PLzbQx-wGxpELpHzldSaLm3gJTxZigcnqV";
pub const FEATURED_PLAYLIST: &str = "PLzbQx-wGxpEI-UaDB8fYuiMfN8C52ycB7";
pub const SUPPORT_URL: &str = "";
pub const HELP_URL: &str = "";
pub const SOURCE_URL: &str = "";
pub const GAME_FOLDER: &str = "Project Reboot";
pub const GAME_COMPANY: &str = "RebootTeam";
pub const GAME_PRODUCT: &str = "Project Reboot";
pub const USER_AGENT: &str = concat!("ProjectRebootLauncher/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug)]
pub struct Config {
    pub auth_url: String,
    pub files_url: String,
    pub files_ref_url: String,
    pub launcher_url: String,
    pub game_address: String,
    pub game_news_url: String,
    pub playlist_feed_url: String,
    pub discord_client_id: String,
    pub discord_authorize_url: String,
    pub redirect_ports: Vec<u16>,
    pub install_dir: PathBuf,
    pub prefs_location: String,
    pub browser: Option<PathBuf>,
}

fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

pub fn data_dir() -> PathBuf {
    if cfg!(windows) {
        env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join("ProjectReboot")
    } else if cfg!(target_os = "macos") {
        home().join("Library").join("Application Support").join("ProjectReboot")
    } else {
        env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home().join(".local").join("share")).join("ProjectReboot")
    }
}

fn unity_root() -> PathBuf {
    if cfg!(windows) {
        env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join("AppData").join("LocalLow")
    } else if cfg!(target_os = "macos") {
        home().join("Library").join("Logs")
    } else {
        env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home().join(".config")).join("unity3d")
    }
}

fn plain_name(value: &str) -> Option<String> {
    let value = value.trim();
    let ok = !value.is_empty() && value != "." && value != ".." && !value.contains(['/', '\\', ':']);
    ok.then(|| value.to_string())
}

pub fn client_log_dir(install_dir: &Path) -> PathBuf {
    let names = fs::read_dir(install_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with("_Data") {
                Some(entry.path().join("app.info"))
            } else if name.ends_with(".app") {
                Some(entry.path().join("Contents").join("Resources").join("Data").join("app.info"))
            } else {
                None
            }
        })
        .find_map(|info| fs::read_to_string(info).ok())
        .and_then(|info| {
            let mut lines = info.lines();
            Some((plain_name(lines.next()?)?, plain_name(lines.next()?)?))
        });
    let (company, product) = names.unwrap_or_else(|| ("RebootTeam".into(), "Project Reboot".into()));
    unity_root().join(company).join(product)
}

pub fn client_log(install_dir: &Path) -> Option<(PathBuf, bool)> {
    let dir = client_log_dir(install_dir);
    let current = dir.join("Player.log");
    if fs::metadata(&current).map(|m| m.len() > 0).unwrap_or(false) {
        return Some((current, false));
    }
    let previous = dir.join("Player-prev.log");
    if previous.is_file() {
        return Some((previous, true));
    }
    current.is_file().then_some((current, false))
}

fn default_prefs_location() -> String {
    if cfg!(windows) || cfg!(target_os = "macos") {
        GAME_PREFS.to_string()
    } else {
        let config = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home().join(".config"));
        config.join(GAME_PREFS).to_string_lossy().to_string()
    }
}

#[derive(Serialize, Deserialize, Default)]
struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    install_dir: Option<PathBuf>,
}

fn settings_path() -> PathBuf {
    data_dir().join("Launcher").join("settings.json")
}

fn load_settings() -> Settings {
    fs::read(settings_path()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

pub fn save_install_dir(dir: &Path) -> Result<(), String> {
    let mut settings = load_settings();
    settings.install_dir = Some(dir.to_path_buf());
    save_settings(&settings)
}

fn save_settings(settings: &Settings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not save the launcher settings: {e}"))?;
    }
    let bytes = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, bytes).map_err(|e| format!("could not save the launcher settings: {e}"))
}

#[cfg(windows)]
pub fn desktop_dir() -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Desktop, SHGetKnownFolderPath};
    unsafe {
        let mut raw: windows_sys::core::PWSTR = std::ptr::null_mut();
        let result = SHGetKnownFolderPath(&FOLDERID_Desktop, 0, std::ptr::null_mut(), &mut raw);
        if result != 0 || raw.is_null() {
            if !raw.is_null() {
                CoTaskMemFree(raw as *const _);
            }
            return None;
        }
        let mut length = 0;
        while *raw.add(length) != 0 {
            length += 1;
        }
        let path = String::from_utf16_lossy(std::slice::from_raw_parts(raw, length));
        CoTaskMemFree(raw as *const _);
        Some(PathBuf::from(path))
    }
}

#[cfg(not(windows))]
pub fn desktop_dir() -> Option<PathBuf> {
    None
}

pub fn legacy_install_dir() -> PathBuf {
    data_dir().join("Game")
}

pub fn needs_setup() -> bool {
    env::var_os("PR_LAUNCHER_INSTALL_DIR").is_none() && load_settings().install_dir.is_none() && !legacy_install_dir().join(".launcher").join("installed.json").is_file()
}

pub fn default_install_dir() -> PathBuf {
    let legacy = legacy_install_dir();
    if legacy.join(".launcher").join("installed.json").is_file() {
        return legacy;
    }
    match desktop_dir() {
        Some(desktop) if cfg!(windows) => desktop.join(GAME_FOLDER),
        _ => legacy,
    }
}

pub const PROTON_HELP: &str = "To play now, add the Windows launcher (ProjectRebootLauncher.exe) to Steam as a non-Steam game and turn on Proton in its Compatibility settings.";

#[derive(Debug, PartialEq, Eq)]
pub enum Existing {
    Found(PathBuf),
    WindowsBuild,
    Missing,
}

pub fn existing_install(chosen: &Path, exe_name: &str) -> Existing {
    let candidates = [chosen.to_path_buf(), chosen.join(GAME_FOLDER)];
    if let Some(found) = candidates.iter().find(|dir| dir.join(exe_name).is_file() || dir.join(".launcher").join("installed.json").is_file()) {
        return Existing::Found(found.clone());
    }
    let windows_build = |dir: &PathBuf| dir.join("Project Reboot.exe").is_file() || (dir.join("D3D12").is_dir() && dir.join("Project Reboot_Data").is_dir());
    if !exe_name.ends_with(".exe") && candidates.iter().any(windows_build) {
        return Existing::WindowsBuild;
    }
    Existing::Missing
}

pub fn install_target(chosen: &Path, exe_name: &str) -> PathBuf {
    let is_install = chosen.join(".launcher").join("installed.json").is_file() || chosen.join(exe_name).is_file();
    let is_empty = fs::read_dir(chosen).map(|mut entries| entries.next().is_none()).unwrap_or(true);
    let named = chosen.file_name().map(|name| name.to_string_lossy().eq_ignore_ascii_case(GAME_FOLDER)).unwrap_or(false);
    if is_install || is_empty || named {
        chosen.to_path_buf()
    } else {
        chosen.join(GAME_FOLDER)
    }
}

fn with_slash(mut url: String) -> String {
    if !url.ends_with('/') {
        url.push('/');
    }
    url
}

fn setting(name: &str, default: &str) -> String {
    env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string())
}

impl Config {
    pub fn load() -> Config {
        let install_dir = match env::var_os("PR_LAUNCHER_INSTALL_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => load_settings().install_dir.unwrap_or_else(default_install_dir),
        };
        let files_url = with_slash(setting("PR_LAUNCHER_FILES_URL", FILES_URL));
        Config {
            auth_url: setting("PR_LAUNCHER_AUTH_URL", AUTH_URL).trim_end_matches('/').to_string(),
            files_url,
            files_ref_url: setting("PR_LAUNCHER_FILES_REF_URL", FILES_REF_URL),
            launcher_url: with_slash(setting("PR_LAUNCHER_SELF_URL", LAUNCHER_LIST_URL)),
            game_address: setting("PR_LAUNCHER_GAME_ADDRESS", GAME_ADDRESS),
            game_news_url: setting("PR_LAUNCHER_GAME_NEWS_URL", GAME_NEWS_URL),
            playlist_feed_url: setting("PR_LAUNCHER_PLAYLIST_FEED_URL", PLAYLIST_FEED_URL),
            discord_client_id: setting("PR_LAUNCHER_DISCORD_CLIENT_ID", DISCORD_CLIENT_ID),
            discord_authorize_url: setting("PR_LAUNCHER_DISCORD_AUTHORIZE_URL", DISCORD_AUTHORIZE_URL),
            redirect_ports: env::var("PR_LAUNCHER_REDIRECT_PORT").ok().and_then(|p| p.trim().parse().ok()).map(|p| vec![p]).unwrap_or_else(|| REDIRECT_PORTS.to_vec()),
            install_dir,
            prefs_location: setting(if cfg!(windows) { "PR_LAUNCHER_REGISTRY_KEY" } else { "PR_LAUNCHER_PREFS_FILE" }, &default_prefs_location()),
            browser: env::var_os("PR_LAUNCHER_BROWSER").map(PathBuf::from),
        }
    }

    pub fn redirect_uri(port: u16) -> String {
        format!("http://127.0.0.1:{port}/callback")
    }

    pub fn discord_ready(&self) -> bool {
        !self.discord_client_id.is_empty() && self.discord_client_id.bytes().all(|b| b.is_ascii_digit()) && self.discord_client_id.bytes().any(|b| b != b'0')
    }

    pub fn state_dir(&self) -> PathBuf {
        self.install_dir.join(".launcher")
    }

    pub fn remember_install_dir(&self) {
        if env::var_os("PR_LAUNCHER_INSTALL_DIR").is_none() && load_settings().install_dir.is_none() {
            let _ = save_install_dir(&self.install_dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_log_folder_comes_from_app_info_and_ignores_unsafe_names() {
        let install = scratch("client-log");
        fs::create_dir_all(install.join("Game_Data")).unwrap();
        fs::write(install.join("Game_Data").join("app.info"), "Studio\nGame Name").unwrap();
        assert!(client_log_dir(&install).ends_with(Path::new("Studio").join("Game Name")));
        fs::write(install.join("Game_Data").join("app.info"), "..\n../escape").unwrap();
        assert!(client_log_dir(&install).ends_with(Path::new("RebootTeam").join("Project Reboot")));
        let _ = fs::remove_dir_all(&install);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("pr-launcher-target-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_empty_folder_is_used_as_it_is() {
        let dir = scratch("empty");
        assert_eq!(install_target(&dir, "Project Reboot.exe"), dir);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_existing_game_folder_is_used_as_it_is() {
        let dir = scratch("game");
        fs::write(dir.join("Project Reboot.exe"), b"x").unwrap();
        fs::write(dir.join("other.txt"), b"x").unwrap();
        assert_eq!(install_target(&dir, "Project Reboot.exe"), dir);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_busy_folder_gets_a_project_reboot_subfolder() {
        let dir = scratch("busy");
        fs::write(dir.join("notes.txt"), b"x").unwrap();
        assert_eq!(install_target(&dir, "Project Reboot.exe"), dir.join(GAME_FOLDER));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_existing_game_is_found_in_the_folder_or_its_project_reboot_folder() {
        let dir = scratch("existing");
        fs::create_dir_all(dir.join(GAME_FOLDER)).unwrap();
        fs::write(dir.join(GAME_FOLDER).join("Project Reboot.x86_64"), b"x").unwrap();
        assert_eq!(existing_install(&dir, "Project Reboot.x86_64"), Existing::Found(dir.join(GAME_FOLDER)));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_windows_build_is_recognised_when_looking_for_the_linux_one() {
        let dir = scratch("windows-build");
        fs::create_dir_all(dir.join("D3D12")).unwrap();
        fs::create_dir_all(dir.join("Project Reboot_Data")).unwrap();
        assert_eq!(existing_install(&dir, "Project Reboot.x86_64"), Existing::WindowsBuild);
        assert_eq!(existing_install(&dir, "Project Reboot.exe"), Existing::Missing);
        fs::write(dir.join("Project Reboot.exe"), b"x").unwrap();
        assert_eq!(existing_install(&dir, "Project Reboot.exe"), Existing::Found(dir.clone()));
        let empty = scratch("nothing-here");
        assert_eq!(existing_install(&empty, "Project Reboot.x86_64"), Existing::Missing);
        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_dir_all(empty);
    }

    #[cfg(windows)]
    #[test]
    fn windows_reports_a_desktop_folder() {
        let desktop = desktop_dir().unwrap();
        assert!(desktop.is_absolute());
        assert!(desktop.is_dir());
    }
}
