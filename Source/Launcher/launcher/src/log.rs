use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_BYTES: u64 = 1024 * 1024;
static LOCK: Mutex<()> = Mutex::new(());

pub fn dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("PR_LAUNCHER_LOG_DIR") {
        return PathBuf::from(dir);
    }
    crate::config::data_dir().join("Launcher")
}

pub fn path() -> PathBuf {
    dir().join("launcher.log")
}

#[cfg(windows)]
fn timestamp() -> String {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

#[cfg(not(windows))]
fn timestamp() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as libc::time_t).unwrap_or(0);
    let mut t: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&now, &mut t);
    }
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.tm_year + 1900, t.tm_mon + 1, t.tm_mday, t.tm_hour, t.tm_min, t.tm_sec)
}

fn write(level: &str, message: &str) {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let file = path();
    if fs::create_dir_all(dir()).is_err() {
        return;
    }
    if fs::metadata(&file).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = fs::rename(&file, dir().join("launcher.old.log"));
    }
    if let Ok(mut out) = OpenOptions::new().create(true).append(true).open(&file) {
        let line = message.replace(['\r', '\n'], " ");
        let _ = writeln!(out, "[{}] {:<5} {}", timestamp(), level, line);
    }
}

pub fn info(message: impl AsRef<str>) {
    write("INFO", message.as_ref());
}

pub fn warn(message: impl AsRef<str>) {
    write("WARN", message.as_ref());
}

pub fn error(message: impl AsRef<str>) {
    write("ERROR", message.as_ref());
}

pub fn tail(max_bytes: u64) -> String {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    tail_file(&path(), max_bytes)
}

pub fn tail_file(path: &std::path::Path, max_bytes: u64) -> String {
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    let length = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = length.saturating_sub(max_bytes);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes).to_string();
    if start > 0 {
        text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or(text)
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_have_a_timestamp_and_stay_on_one_line() {
        let dir = std::env::temp_dir().join(format!("pr-launcher-log-test-{}", std::process::id()));
        std::env::set_var("PR_LAUNCHER_LOG_DIR", &dir);
        write("TEST", "first\nsecond");
        let tail = tail(4096);
        let last = tail.lines().last().unwrap_or("");
        assert!(last.starts_with('['));
        assert!(last.contains("TEST  first second"));
        let _ = fs::remove_dir_all(dir);
    }
}
