use std::env;
use std::fs;
use std::path::PathBuf;

const APP_ID: &str = "project-reboot-launcher";
const ICON: &[u8] = include_bytes!("../../assets/app-icon.png");

fn data_home() -> PathBuf {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join(".local").join("share"))
}

fn quote(path: &str) -> String {
    format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\"").replace('`', "\\`").replace('$', "\\$"))
}

fn write_if_changed(path: &PathBuf, bytes: &[u8]) -> Result<(), String> {
    if fs::read(path).map(|current| current == bytes).unwrap_or(false) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    fs::write(path, bytes).map_err(|e| format!("could not write {}: {e}", path.display()))
}

pub fn install() -> Result<(), String> {
    let exe = env::current_exe().map_err(|e| format!("could not find the launcher: {e}"))?;
    let icon = data_home().join("icons").join("hicolor").join("256x256").join("apps").join(format!("{APP_ID}.png"));
    write_if_changed(&icon, ICON)?;
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Project Reboot\nComment=Play Project Reboot\nExec={}\nIcon={APP_ID}\nTerminal=false\nCategories=Game;\nStartupWMClass={APP_ID}\n",
        quote(&exe.to_string_lossy())
    );
    write_if_changed(&data_home().join("applications").join(format!("{APP_ID}.desktop")), entry.as_bytes())
}
