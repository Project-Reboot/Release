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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Normal,
    Warning,
    Error,
    Crash,
}

const CRASH_MARKERS: [&str; 14] = [
    "crash!!!",
    "received signal sig",
    "sigsegv",
    "sigabrt",
    "sigbus",
    "sigill",
    "native crash",
    "outputting stack trace",
    "fatal error",
    "segmentation fault",
    "core dumped",
    "unhandled exception",
    "panicked at",
    "access violation",
];

fn has_word(lower: &str, word: &str) -> bool {
    lower.match_indices(word).any(|(at, _)| {
        let before = lower[..at].chars().next_back();
        let after = lower[at + word.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_alphabetic()) && !after.is_some_and(|c| c.is_ascii_alphabetic())
    })
}

fn is_stack_frame(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return false;
    }
    if line.starts_with([' ', '\t']) {
        return true;
    }
    if ["at ", "(wrapper", "Rethrow as", "--- End of", "0x"].iter().any(|p| trimmed.starts_with(p)) {
        return true;
    }
    if trimmed.starts_with('#') && trimmed[1..].starts_with(|c: char| c.is_ascii_digit()) {
        return true;
    }
    match (trimmed.find(':'), trimmed.find(" (")) {
        (Some(colon), Some(paren)) => colon < paren && !trimmed[..paren].contains(' ') && trimmed.ends_with(')'),
        _ => false,
    }
}

fn own_severity(line: &str, client: bool) -> Severity {
    let lower = line.to_ascii_lowercase();
    if CRASH_MARKERS.iter().any(|m| lower.contains(m)) {
        return Severity::Crash;
    }
    if client {
        if lower.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| w.ends_with("exception")) || has_word(&lower, "error") {
            return Severity::Error;
        }
        if has_word(&lower, "warning") {
            return Severity::Warning;
        }
    } else if line.contains("] ERROR") {
        return Severity::Error;
    } else if line.contains("] WARN") {
        return Severity::Warning;
    }
    Severity::Normal
}

pub fn classify(lines: &[&str], client: bool) -> Vec<Severity> {
    let mut out = Vec::with_capacity(lines.len());
    let mut carry = Severity::Normal;
    for line in lines {
        let own = own_severity(line, client);
        let severity = if own == Severity::Normal && matches!(carry, Severity::Error | Severity::Crash) && is_stack_frame(line) { carry } else { own };
        carry = if line.trim().is_empty() { Severity::Normal } else if own == Severity::Normal { if is_stack_frame(line) { carry } else { Severity::Normal } } else { own };
        out.push(severity);
    }
    out
}

pub fn count(marks: &[Severity]) -> (usize, usize) {
    let (mut errors, mut crashes) = (0, 0);
    let mut last_crash: Option<usize> = None;
    for (i, mark) in marks.iter().enumerate() {
        let starts = i == 0 || marks[i - 1] != *mark;
        match mark {
            Severity::Error if starts => errors += 1,
            Severity::Crash => {
                if last_crash.map_or(true, |at| i - at > 12) {
                    crashes += 1;
                }
                last_crash = Some(i);
            }
            _ => {}
        }
    }
    (errors, crashes)
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

    #[test]
    fn errors_crashes_and_their_stack_traces_are_marked() {
        let client = [
            "Loading player data",
            "NullReferenceException: Object reference not set to an instance of an object",
            "  at PlayerController.Update () [0x00000] in <abc>:0 ",
            "UnityEngine.Debug:LogError (object)",
            "Fishing:Cast ()",
            "",
            "Some other line",
            "Warning: the sound is too loud",
            "Crash!!!",
            "#0 0x00007ff6 in UnityPlayer",
            "errorCode not an error word",
            "Tile errors do not count",
        ];
        let marks = classify(&client, true);
        assert_eq!(
            marks,
            vec![
                Severity::Normal,
                Severity::Error,
                Severity::Error,
                Severity::Error,
                Severity::Error,
                Severity::Normal,
                Severity::Normal,
                Severity::Warning,
                Severity::Crash,
                Severity::Crash,
                Severity::Error,
                Severity::Normal,
            ]
        );
        let launcher = ["[2026-11-20 10:00:00] INFO  Started", "[2026-11-20 10:00:01] WARN  Slow", "[2026-11-20 10:00:02] ERROR Broke", "thread 'main' panicked at src/ui.rs:1:1"];
        assert_eq!(classify(&launcher, false), vec![Severity::Normal, Severity::Warning, Severity::Error, Severity::Crash]);
    }

    #[test]
    fn one_crash_report_counts_once() {
        let unity = ["Crash!!!", "SymInit: Symbol-SearchPath: '.'", "========== OUTPUTTING STACK TRACE ==================", "0x00007FF6 (UnityPlayer) UnityMain", "NullReferenceException: boom", "  at A.B () [0x0] in <x>:0"];
        assert_eq!(count(&classify(&unity, true)), (1, 1));
        let mut two = vec!["Crash!!!"];
        two.extend(std::iter::repeat("normal line").take(20));
        two.push("Crash!!!");
        assert_eq!(count(&classify(&two, true)), (0, 2));
    }
}
