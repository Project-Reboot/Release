use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub const START_TIMEOUT: Duration = Duration::from_secs(90);
pub const RELAUNCH_GRACE: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(750);

#[cfg(windows)]
mod platform {
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};

    fn image_path(pid: u32) -> Option<PathBuf> {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buffer = vec![0u16; 32768];
            let mut length = buffer.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length);
            CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            Some(PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize])))
        }
    }

    fn normalized(path: &Path) -> String {
        let text = path.to_string_lossy().replace('/', "\\").to_lowercase();
        text.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(text)
    }

    fn folder_prefix(path: &Path) -> String {
        let mut prefix = normalized(path);
        if !prefix.ends_with('\\') {
            prefix.push('\\');
        }
        prefix
    }

    pub fn wine_version() -> Option<String> {
        unsafe {
            let ntdll = GetModuleHandleA(c"ntdll.dll".as_ptr().cast());
            if ntdll.is_null() {
                return None;
            }
            let function = GetProcAddress(ntdll, c"wine_get_version".as_ptr().cast())?;
            let get_version: unsafe extern "C" fn() -> *const std::ffi::c_char = std::mem::transmute(function);
            let version = get_version();
            if version.is_null() {
                return Some("unknown".into());
            }
            Some(std::ffi::CStr::from_ptr(version).to_string_lossy().to_string())
        }
    }

    pub fn running_from(dir: &Path) -> Vec<u32> {
        let mut prefixes = vec![folder_prefix(dir)];
        if let Ok(canonical) = dir.canonicalize() {
            let extra = folder_prefix(&canonical);
            if !prefixes.contains(&extra) {
                prefixes.push(extra);
            }
        }
        let mut found = Vec::new();
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return found;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                if entry.th32ProcessID != 0 && entry.th32ProcessID != std::process::id() {
                    if let Some(path) = image_path(entry.th32ProcessID) {
                        let path = normalized(&path);
                        if prefixes.iter().any(|prefix| path.starts_with(prefix)) {
                            found.push(entry.th32ProcessID);
                        }
                    }
                }
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
        }
        found
    }

    pub fn set_executable(_path: &Path) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(not(windows))]
mod platform {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    pub fn wine_version() -> Option<String> {
        None
    }

    fn folder(path: &Path) -> PathBuf {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }

    pub fn running_from(dir: &Path) -> Vec<u32> {
        let folders = [dir.to_path_buf(), folder(dir)];
        let me = std::process::id();
        let mut found = Vec::new();
        let Ok(entries) = fs::read_dir("/proc") else { return found };
        for entry in entries.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()) else { continue };
            if pid == me {
                continue;
            }
            if let Ok(exe) = fs::read_link(entry.path().join("exe")) {
                if folders.iter().any(|folder| exe.starts_with(folder)) {
                    found.push(pid);
                }
            }
        }
        found
    }

    pub fn set_executable(path: &Path) -> Result<(), String> {
        let mut permissions = fs::metadata(path).map_err(|e| format!("could not read {}: {e}", path.display()))?.permissions();
        permissions.set_mode(permissions.mode() | 0o755);
        fs::set_permissions(path, permissions).map_err(|e| format!("could not make {} runnable: {e}", path.display()))
    }
}

pub use platform::{running_from, set_executable, wine_version};

pub fn launch(exe: &Path) -> Result<(), String> {
    let dir = exe.parent().ok_or("the game path has no folder")?;
    set_executable(exe)?;
    Command::new(exe).current_dir(dir).spawn().map(|_| ()).map_err(|e| format!("could not start the game: {e}"))
}

pub enum Ended {
    Closed,
    NeverStarted,
    Cancelled,
}

pub fn wait_until_closed(dir: &Path, cancel: &AtomicBool) -> Ended {
    let started = Instant::now();
    let mut seen = false;
    let mut last_seen = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ended::Cancelled;
        }
        if !running_from(dir).is_empty() {
            seen = true;
            last_seen = Instant::now();
        } else if seen && last_seen.elapsed() >= RELAUNCH_GRACE {
            return Ended::Closed;
        } else if !seen && started.elapsed() >= START_TIMEOUT {
            return Ended::NeverStarted;
        }
        thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn wine_is_not_detected_on_windows() {
        assert_eq!(wine_version(), None);
    }

    #[cfg(windows)]
    #[test]
    fn finds_processes_running_from_a_folder() {
        let me = std::env::current_exe().unwrap();
        let dir = me.parent().unwrap();
        let mut child = Command::new("cmd").args(["/c", "ping -n 3 127.0.0.1 >nul"]).spawn().unwrap();
        assert!(running_from(Path::new(r"C:\Windows\System32")).contains(&child.id()));
        assert!(!running_from(dir).contains(&child.id()));
        let _ = child.kill();
        let _ = child.wait();
    }

    #[cfg(not(windows))]
    #[test]
    fn finds_processes_running_from_a_folder() {
        let sleep = std::fs::canonicalize("/bin/sleep").or_else(|_| std::fs::canonicalize("/usr/bin/sleep")).unwrap();
        let mut child = Command::new(&sleep).arg("3").spawn().unwrap();
        assert!(running_from(sleep.parent().unwrap()).contains(&child.id()));
        assert!(!running_from(Path::new("/nonexistent-folder")).contains(&child.id()));
        let _ = child.kill();
        let _ = child.wait();
    }
}
