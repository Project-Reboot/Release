use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use pr_manifest::{LauncherBuild, LauncherList, LAUNCHER_LIST_FILE, LAUNCHER_LIST_SIGNATURE};

use crate::config::{Config, MANIFEST_PUBLIC_KEY};
use crate::update;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(suffix);
    exe.with_file_name(name)
}

pub fn remove_leftovers() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = fs::remove_file(sibling(&exe, ".old"));
        let _ = fs::remove_file(sibling(&exe, ".new"));
    }
}

const MAX_LIST_BYTES: u64 = 64 * 1024;

pub struct LauncherRelease {
    pub list: LauncherList,
    pub base: String,
}

pub fn fetch_list(agent: &ureq::Agent, cfg: &Config, git_ref: &str) -> Result<LauncherRelease, String> {
    fetch_list_from(agent, &update::at_ref(&cfg.launcher_url, git_ref), &pr_manifest::parse_public_key(MANIFEST_PUBLIC_KEY)?)
}

pub(crate) fn fetch_list_from(agent: &ureq::Agent, base: &str, public: &pr_manifest::VerifyingKey) -> Result<LauncherRelease, String> {
    let base = base.to_string();
    let (bytes, signature) = update::fetch_pair(agent, (&update::file_url(&base, LAUNCHER_LIST_FILE)?, MAX_LIST_BYTES), (&update::file_url(&base, LAUNCHER_LIST_SIGNATURE)?, 1024));
    let (bytes, signature) = (bytes?, signature?);
    let list = pr_manifest::verify_launcher_list(&bytes, &String::from_utf8_lossy(&signature), public)?;
    Ok(LauncherRelease { list, base })
}

pub fn newer(list: &LauncherList) -> Option<&LauncherBuild> {
    if pr_manifest::is_newer(&list.version, CURRENT_VERSION) {
        list.for_this_system()
    } else {
        None
    }
}

pub fn install_and_restart(agent: &ureq::Agent, release: &LauncherRelease, build: &LauncherBuild, on_received: &mut dyn FnMut(u64), cancel: &AtomicBool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("could not find the launcher: {e}"))?;
    let fresh = sibling(&exe, ".new");
    let old = sibling(&exe, ".old");
    update::download_verified(agent, &update::file_url(&release.base, &build.file)?, &build.sha256, build.size, &fresh, on_received, cancel)
        .map_err(|e| format!("could not download launcher {}: {e}", release.list.version))?;
    crate::game::set_executable(&fresh)?;
    let _ = fs::remove_file(&old);
    if let Err(e) = fs::rename(&exe, &old) {
        let _ = fs::remove_file(&fresh);
        return Err(format!("could not replace the launcher (is its folder read-only?): {e}"));
    }
    if let Err(e) = fs::rename(&fresh, &exe) {
        let _ = fs::rename(&old, &exe);
        let _ = fs::remove_file(&fresh);
        return Err(format!("could not replace the launcher: {e}"));
    }
    Command::new(&exe)
        .args(std::env::args_os().skip(1))
        .spawn()
        .map_err(|e| format!("the launcher was updated but could not restart; please open it again: {e}"))?;
    Ok(())
}
