use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, UNIX_EPOCH};

use pr_manifest::{FileEntry, Manifest, MANIFEST_FILE, POSTER_FILE, SIGNATURE_FILE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::{Config, FILES_FALLBACK_REF, MANIFEST_PUBLIC_KEY, USER_AGENT};

const STATE_FILE: &str = "installed.json";
const DOWNLOAD_DIR: &str = "download";
const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub enum Progress {
    Checking { done: usize, total: usize },
    Downloading { done: u64, total: u64 },
}

#[derive(Clone, Debug)]
pub struct Release {
    pub manifest: Manifest,
    pub base: String,
}

#[derive(Clone, Debug)]
pub struct Installed {
    pub version: String,
    pub exe: PathBuf,
}

#[derive(Serialize, Deserialize, Default)]
struct State {
    version: String,
    exe: String,
    files: BTreeMap<String, Known>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
struct Known {
    size: u64,
    modified: u128,
    sha256: String,
}

pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .user_agent(USER_AGENT)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build()
}

pub fn describe(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(code, _) => format!("the update server answered {code}"),
        ureq::Error::Transport(t) => format!("could not reach the update server ({})", t.kind()),
    }
}

fn load_state(cfg: &Config) -> State {
    fs::read(cfg.state_dir().join(STATE_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_state(cfg: &Config, state: &State) -> Result<(), String> {
    let dir = cfg.state_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("could not write to {}: {e}", dir.display()))?;
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    let partial = dir.join(format!("{STATE_FILE}.part"));
    fs::write(&partial, bytes).map_err(|e| format!("could not save the install state: {e}"))?;
    fs::rename(&partial, dir.join(STATE_FILE)).map_err(|e| format!("could not save the install state: {e}"))
}

fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to).map_err(|e| format!("could not copy {}: {e}", from.display()))?;
    fs::remove_file(from).map_err(|e| format!("could not remove {}: {e}", from.display()))
}

fn remove_empty_dirs(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                remove_empty_dirs(&entry.path());
            }
        }
    }
    let _ = fs::remove_dir(dir);
}

pub fn move_install(cfg: &Config, target: &Path, progress: &mut dyn FnMut(usize, usize)) -> Result<(), String> {
    let source = cfg.install_dir.clone();
    let from = source.canonicalize().unwrap_or_else(|_| source.clone());
    fs::create_dir_all(target).map_err(|e| format!("could not create {}: {e}", target.display()))?;
    let to = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
    if to.starts_with(&from) || from.starts_with(&to) {
        return Err("Choose a folder that is not inside the current game folder.".into());
    }
    let state = load_state(cfg);
    let mut files: Vec<String> = state.files.keys().cloned().filter(|p| pr_manifest::is_safe_path(p)).collect();
    files.sort();
    let total = files.len() + 1;
    for (index, rel) in files.iter().enumerate() {
        progress(index, total);
        let old = local_path(cfg, rel);
        if old.is_file() {
            let new = rel.split('/').fold(target.to_path_buf(), |path, part| path.join(part));
            move_file(&old, &new)?;
        }
    }
    let old_state = cfg.state_dir();
    let new_state = target.join(".launcher");
    if old_state.is_dir() {
        fs::create_dir_all(&new_state).map_err(|e| format!("could not create {}: {e}", new_state.display()))?;
        if let Ok(entries) = fs::read_dir(&old_state) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    move_file(&entry.path(), &new_state.join(entry.file_name()))?;
                }
            }
        }
    }
    progress(total, total);
    remove_empty_dirs(&source);
    Ok(())
}

pub fn forget_hashes(cfg: &Config) -> Result<(), String> {
    let mut state = load_state(cfg);
    for known in state.files.values_mut() {
        known.modified = 0;
    }
    save_state(cfg, &state)
}

pub fn installed(cfg: &Config) -> Option<Installed> {
    let state = load_state(cfg);
    if state.version.is_empty() || !pr_manifest::is_safe_path(&state.exe) {
        return None;
    }
    let exe = local_path(cfg, &state.exe);
    exe.is_file().then_some(Installed { version: state.version, exe })
}

fn modified(meta: &fs::Metadata) -> u128 {
    meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0)
}

fn local_path(cfg: &Config, rel: &str) -> PathBuf {
    rel.split('/').fold(cfg.install_dir.clone(), |path, part| path.join(part))
}

fn is_current(cfg: &Config, state: &mut State, entry: &FileEntry) -> bool {
    let path = local_path(cfg, &entry.path);
    let Ok(meta) = fs::metadata(&path) else { return false };
    if !meta.is_file() || meta.len() != entry.size {
        return false;
    }
    let stamp = modified(&meta);
    if let Some(known) = state.files.get(&entry.path) {
        if known.size == entry.size && known.modified == stamp && known.sha256 == entry.sha256 {
            return true;
        }
    }
    let Ok(file) = fs::File::open(&path) else { return false };
    match pr_manifest::hash_reader(file) {
        Ok((sha256, size)) if sha256 == entry.sha256 && size == entry.size => {
            state.files.insert(entry.path.clone(), Known { size, modified: stamp, sha256 });
            true
        }
        _ => false,
    }
}

pub(crate) fn fetch_bytes(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let response = agent.get(url).call().map_err(describe)?;
    let mut bytes = Vec::new();
    response.into_reader().take(limit + 1).read_to_end(&mut bytes).map_err(|e| format!("the download was interrupted: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err("the update server sent too much data".into());
    }
    Ok(bytes)
}

pub fn newest_ref(agent: &ureq::Agent, cfg: &Config) -> String {
    agent
        .get(&cfg.files_ref_url)
        .set("Accept", "application/vnd.github.sha")
        .call()
        .ok()
        .and_then(|r| r.into_string().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_else(|| FILES_FALLBACK_REF.to_string())
}

pub fn file_url(base: &str, rel: &str) -> Result<String, String> {
    let mut url = url::Url::parse(base).map_err(|e| format!("the update address is not valid: {e}"))?;
    url.path_segments_mut().map_err(|_| "the update address is not valid".to_string())?.pop_if_empty().extend(rel.split('/'));
    Ok(url.to_string())
}

const NOT_PUBLISHED: &str = "no game build has been published for";

pub fn is_not_published(error: &str) -> bool {
    error.starts_with(NOT_PUBLISHED)
}

pub fn at_ref(url: &str, git_ref: &str) -> String {
    url.replace("{ref}", git_ref)
}

pub(crate) fn fetch_pair(agent: &ureq::Agent, first: (&str, u64), second: (&str, u64)) -> (Result<Vec<u8>, String>, Result<Vec<u8>, String>) {
    std::thread::scope(|scope| {
        let later = scope.spawn(|| fetch_bytes(agent, second.0, second.1));
        let now = fetch_bytes(agent, first.0, first.1);
        (now, later.join().unwrap_or_else(|_| Err("the download stopped unexpectedly".to_string())))
    })
}

pub fn fetch_release(agent: &ureq::Agent, cfg: &Config, git_ref: &str) -> Result<Release, String> {
    let base = at_ref(&cfg.files_url, git_ref);
    let public = pr_manifest::parse_public_key(MANIFEST_PUBLIC_KEY)?;
    let (bytes, signature) = fetch_pair(agent, (&file_url(&base, MANIFEST_FILE)?, MAX_MANIFEST_BYTES), (&file_url(&base, SIGNATURE_FILE)?, 1024));
    let bytes = bytes.map_err(|e| {
        if e.ends_with("answered 404") {
            format!("{NOT_PUBLISHED} {} yet", crate::config::PLATFORM)
        } else {
            e
        }
    })?;
    let signature = signature?;
    let manifest = pr_manifest::verify(&bytes, &String::from_utf8_lossy(&signature), &public)?;
    Ok(Release { manifest, base })
}

enum Attempt {
    Corrupt(String),
    Stopped(String),
}

impl Attempt {
    fn message(self) -> String {
        match self {
            Attempt::Corrupt(message) | Attempt::Stopped(message) => message,
        }
    }
}

fn hash_prefix(path: &Path, length: u64, hasher: &mut Sha256) -> Result<(), String> {
    let file = fs::File::open(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let mut reader = file.take(length);
    let mut buffer = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        let read = reader.read(&mut buffer).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    if total == length {
        Ok(())
    } else {
        Err(format!("could not read {}", path.display()))
    }
}

fn resumes_at(response: &ureq::Response, start: u64, size: u64) -> bool {
    response.status() == 206
        && response
            .header("Content-Range")
            .and_then(|range| range.trim().strip_prefix("bytes "))
            .and_then(|range| range.split_once('/'))
            .map(|(span, total)| span.split('-').next() == Some(start.to_string().as_str()) && (total == "*" || total == size.to_string()))
            .unwrap_or(false)
}

pub fn download_verified(
    agent: &ureq::Agent,
    url: &str,
    sha256: &str,
    size: u64,
    partial: &Path,
    on_received: &mut dyn FnMut(u64),
    cancel: &AtomicBool,
) -> Result<(), String> {
    let kept = fs::metadata(partial).map(|m| m.len()).unwrap_or(0);
    if kept > 0 && kept <= size {
        match download_once(agent, url, sha256, size, partial, kept, on_received, cancel) {
            Err(Attempt::Corrupt(_)) => crate::log::warn("A partly downloaded file did not match, so it is being downloaded again"),
            other => return other.map_err(Attempt::message),
        }
    }
    let _ = fs::remove_file(partial);
    download_once(agent, url, sha256, size, partial, 0, on_received, cancel).map_err(Attempt::message)
}

#[allow(clippy::too_many_arguments)]
fn download_once(
    agent: &ureq::Agent,
    url: &str,
    sha256: &str,
    size: u64,
    partial: &Path,
    kept: u64,
    on_received: &mut dyn FnMut(u64),
    cancel: &AtomicBool,
) -> Result<(), Attempt> {
    let write_error = |e: std::io::Error| Attempt::Stopped(format!("could not write {}: {e}", partial.display()));
    let mut hasher = Sha256::new();
    let mut out = if kept == 0 {
        fs::File::create(partial).map_err(write_error)?
    } else {
        hash_prefix(partial, kept, &mut hasher).map_err(Attempt::Corrupt)?;
        fs::OpenOptions::new().append(true).open(partial).map_err(write_error)?
    };
    let mut received = kept;
    on_received(received);
    if received < size {
        let mut request = agent.get(url);
        if kept > 0 {
            request = request.set("Range", &format!("bytes={kept}-"));
        }
        let response = request.call().map_err(|e| Attempt::Stopped(describe(e)))?;
        if kept > 0 && !resumes_at(&response, kept, size) {
            return Err(Attempt::Corrupt("the update server did not continue the download".into()));
        }
        let mut reader = response.into_reader();
        let mut buffer = vec![0u8; 256 * 1024];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Attempt::Stopped("the update was cancelled".into()));
            }
            let read = reader.read(&mut buffer).map_err(|e| Attempt::Stopped(format!("the download was interrupted: {e}")))?;
            if read == 0 {
                break;
            }
            if received + read as u64 > size {
                drop(out);
                let _ = fs::remove_file(partial);
                return Err(Attempt::Corrupt("the update server sent a file bigger than expected".into()));
            }
            out.write_all(&buffer[..read]).map_err(write_error)?;
            hasher.update(&buffer[..read]);
            received += read as u64;
            on_received(received);
        }
    }
    out.flush().map_err(write_error)?;
    drop(out);
    if received < size {
        return Err(Attempt::Stopped("the download was interrupted; it will carry on from where it stopped next time".into()));
    }
    if hex::encode(hasher.finalize()) == sha256 {
        Ok(())
    } else {
        let _ = fs::remove_file(partial);
        Err(Attempt::Corrupt("a file did not download correctly; please try again".into()))
    }
}

fn unpack(packed: &Path, out: &Path, entry: &FileEntry, cancel: &AtomicBool) -> Result<(), String> {
    let source = fs::File::open(packed).map_err(|e| format!("could not read {}: {e}", packed.display()))?;
    let mut decoder = zstd::stream::read::Decoder::new(source).map_err(|e| format!("could not unpack the download: {e}"))?;
    let result = (|| {
        let mut sink = fs::File::create(out).map_err(|e| format!("could not write {}: {e}", out.display()))?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 256 * 1024];
        let mut size = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("the update was cancelled".to_string());
            }
            let read = decoder.read(&mut buffer).map_err(|e| format!("could not unpack the download: {e}"))?;
            if read == 0 {
                break;
            }
            size += read as u64;
            if size > entry.size {
                return Err("the download unpacked to more than expected".to_string());
            }
            hasher.update(&buffer[..read]);
            sink.write_all(&buffer[..read]).map_err(|e| format!("could not write {}: {e}", out.display()))?;
        }
        sink.flush().map_err(|e| format!("could not write {}: {e}", out.display()))?;
        if size == entry.size && hex::encode(hasher.finalize()) == entry.sha256 {
            Ok(())
        } else {
            Err("a file did not unpack correctly; please try again".to_string())
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(out);
    }
    result
}

fn fetch_entry(agent: &ureq::Agent, release: &Release, entry: &FileEntry, download_dir: &Path, on_received: &mut dyn FnMut(u64), cancel: &AtomicBool) -> Result<PathBuf, String> {
    let partial = download_dir.join(format!("{}.part", entry.sha256));
    if let Some(packed) = &entry.packed {
        let packed_partial = download_dir.join(format!("{}.zst.part", packed.sha256));
        match download_verified(agent, &file_url(&release.base, &entry.packed_path())?, &packed.sha256, packed.size, &packed_partial, on_received, cancel) {
            Ok(()) => {
                let unpacked = unpack(&packed_partial, &partial, entry, cancel);
                let _ = fs::remove_file(&packed_partial);
                return unpacked.map(|_| partial);
            }
            Err(e) if e.ends_with("answered 404") => crate::log::warn(format!("No packed copy of {} on the server; downloading it whole", entry.path)),
            Err(e) => return Err(e),
        }
    }
    download_verified(agent, &file_url(&release.base, &entry.path)?, &entry.sha256, entry.size, &partial, on_received, cancel)?;
    Ok(partial)
}

pub fn poster(agent: &ureq::Agent, cfg: &Config, release: &Release, cancel: &AtomicBool) -> Option<Vec<u8>> {
    let entry = release.manifest.poster.as_ref()?;
    let dir = cfg.state_dir();
    let cached = dir.join(format!("poster-{}.png", entry.sha256));
    if let Ok(bytes) = fs::read(&cached) {
        if bytes.len() as u64 == entry.size && pr_manifest::sha256_hex(&bytes) == entry.sha256 {
            return Some(bytes);
        }
    }
    fs::create_dir_all(&dir).ok()?;
    if let Ok(old) = fs::read_dir(&dir) {
        for file in old.flatten() {
            let name = file.file_name().to_string_lossy().to_string();
            if name.starts_with("poster-") && name.ends_with(".png") {
                let _ = fs::remove_file(file.path());
            }
        }
    }
    let url = file_url(&release.base, POSTER_FILE).ok()?;
    download_verified(agent, &url, &entry.sha256, entry.size, &cached, &mut |_| {}, cancel).ok()?;
    fs::read(&cached).ok()
}

pub fn update(agent: &ureq::Agent, cfg: &Config, release: &Release, progress: &mut dyn FnMut(Progress), cancel: &AtomicBool) -> Result<Installed, String> {
    let manifest = &release.manifest;
    fs::create_dir_all(&cfg.install_dir).map_err(|e| format!("could not create {}: {e}", cfg.install_dir.display()))?;
    let mut state = load_state(cfg);
    let previous: Vec<String> = state.files.keys().cloned().collect();

    let mut needed = Vec::new();
    for (index, entry) in manifest.files.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err("the update was cancelled".into());
        }
        progress(Progress::Checking { done: index, total: manifest.files.len() });
        if !is_current(cfg, &mut state, entry) {
            needed.push(entry.clone());
        }
    }

    let total: u64 = needed.iter().map(FileEntry::transfer_size).sum();
    let mut done = 0u64;
    let download_dir = cfg.state_dir().join(DOWNLOAD_DIR);
    if !needed.is_empty() {
        fs::create_dir_all(&download_dir).map_err(|e| format!("could not write to {}: {e}", download_dir.display()))?;
        progress(Progress::Downloading { done: 0, total });
    }
    for entry in &needed {
        let before = done;
        let partial = fetch_entry(agent, release, entry, &download_dir, &mut |received| progress(Progress::Downloading { done: (before + received).min(total), total }), cancel)
            .map_err(|e| format!("{}: {e}", entry.path))?;
        done = (before + entry.transfer_size()).min(total);
        let target = local_path(cfg, &entry.path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        fs::rename(&partial, &target).map_err(|e| format!("could not install {}: {e}", entry.path))?;
        let meta = fs::metadata(&target).map_err(|e| e.to_string())?;
        state.files.insert(entry.path.clone(), Known { size: entry.size, modified: modified(&meta), sha256: entry.sha256.clone() });
        save_state(cfg, &state)?;
    }

    let wanted: HashSet<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    for old in previous.iter().filter(|p| !wanted.contains(p.as_str())) {
        if pr_manifest::is_safe_path(old) {
            let _ = fs::remove_file(local_path(cfg, old));
        }
        state.files.remove(old);
    }
    state.version = manifest.version.clone();
    state.exe = manifest.exe.clone();
    save_state(cfg, &state)?;
    crate::game::set_executable(&local_path(cfg, &manifest.exe))?;
    let _ = fs::remove_dir_all(&download_dir);
    Ok(Installed { version: manifest.version.clone(), exe: local_path(cfg, &manifest.exe) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    struct Files {
        base: String,
        asked: Arc<Mutex<Vec<(String, Option<String>)>>>,
    }

    impl Files {
        fn asked(&self) -> Vec<(String, Option<String>)> {
            std::mem::take(&mut *self.asked.lock().unwrap())
        }
    }

    fn serve(files: Vec<(&str, Vec<u8>)>) -> Files {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}/files/", listener.local_addr().unwrap().port());
        let files: BTreeMap<String, Vec<u8>> = files.into_iter().map(|(path, bytes)| (format!("/files/{path}"), bytes)).collect();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let log = asked.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                if reader.read_line(&mut first).is_err() {
                    continue;
                }
                let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
                let mut range = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        if name.eq_ignore_ascii_case("range") {
                            range = Some(value.trim().to_string());
                        }
                    }
                }
                log.lock().unwrap().push((path.clone(), range.clone()));
                let mut stream = stream;
                let Some(bytes) = files.get(&path) else {
                    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                };
                let start = range.as_deref().and_then(|r| r.strip_prefix("bytes=")).and_then(|r| r.trim_end_matches('-').parse::<usize>().ok()).filter(|s| *s < bytes.len());
                let head = match start {
                    Some(start) => format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        bytes.len() - start,
                        start,
                        bytes.len() - 1,
                        bytes.len()
                    ),
                    None => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()),
                };
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&bytes[start.unwrap_or(0)..]);
            }
        });
        Files { base, asked }
    }

    fn release(base: &str, version: &str, files: &[(&str, &[u8])]) -> Release {
        Release {
            manifest: Manifest {
                version: version.into(),
                exe: "game.exe".into(),
                files: files.iter().map(|(path, bytes)| FileEntry { path: (*path).into(), size: bytes.len() as u64, sha256: pr_manifest::sha256_hex(bytes), packed: None }).collect(),
                notes: None,
                poster: None,
            },
            base: base.into(),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pr-launcher-update-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn run(cfg: &Config, release: &Release) -> Result<Installed, String> {
        update(&agent(), cfg, release, &mut |_| {}, &AtomicBool::new(false))
    }

    fn big(seed: u8) -> Vec<u8> {
        (0..300_000u32).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
    }

    #[test]
    fn a_signed_launcher_list_is_read_and_only_a_newer_one_updates() {
        use pr_manifest::{LauncherBuild, LauncherList, LINUX_LAUNCHER_PATH, WINDOWS_LAUNCHER_PATH};
        let key = pr_manifest::SigningKey::from_bytes(&[5u8; 32]);
        let public = pr_manifest::parse_public_key(&pr_manifest::public_key_hex(&key)).unwrap();
        let list = LauncherList {
            version: "999.0.0".into(),
            windows: Some(LauncherBuild { file: WINDOWS_LAUNCHER_PATH.into(), size: 3, sha256: pr_manifest::sha256_hex(b"win") }),
            linux: Some(LauncherBuild { file: LINUX_LAUNCHER_PATH.into(), size: 3, sha256: pr_manifest::sha256_hex(b"lin") }),
        };
        let bytes = serde_json::to_vec(&list).unwrap();
        let signature = pr_manifest::sign_launcher_list(&bytes, &key);
        let server = serve(vec![("launcher.json", bytes), ("launcher.json.sig", signature.into_bytes())]);
        let release = crate::selfupdate::fetch_list_from(&agent(), &server.base, &public).unwrap();
        assert_eq!(release.list, list);
        let expected = if cfg!(windows) { WINDOWS_LAUNCHER_PATH } else { LINUX_LAUNCHER_PATH };
        assert_eq!(crate::selfupdate::newer(&release.list).map(|b| b.file.as_str()), Some(expected));
        let same = LauncherList { version: crate::selfupdate::CURRENT_VERSION.into(), ..list.clone() };
        assert!(crate::selfupdate::newer(&same).is_none());
        let stranger = pr_manifest::parse_public_key(&pr_manifest::public_key_hex(&pr_manifest::SigningKey::from_bytes(&[6u8; 32]))).unwrap();
        assert!(crate::selfupdate::fetch_list_from(&agent(), &server.base, &stranger).is_err());
        assert_eq!(file_url("https://example.test/Release/abc/", WINDOWS_LAUNCHER_PATH).unwrap(), "https://example.test/Release/abc/Windows%20Launcher/ProjectRebootLauncher.exe");
    }

    fn packed(bytes: &[u8]) -> Vec<u8> {
        zstd::stream::encode_all(bytes, 3).unwrap()
    }

    fn with_packing(mut rel: Release, path: &str, zst: &[u8]) -> Release {
        let entry = rel.manifest.files.iter_mut().find(|f| f.path == path).unwrap();
        entry.packed = Some(pr_manifest::Packed { size: zst.len() as u64, sha256: pr_manifest::sha256_hex(zst) });
        rel
    }

    #[test]
    fn a_packed_file_is_downloaded_small_and_unpacked() {
        let dir = scratch("packed");
        let data = big(9);
        let zst = packed(&data);
        assert!(zst.len() < data.len());
        let files = [("game.exe", &b"exe"[..]), ("Data/level.bin", &data[..])];
        let server = serve(vec![("game.exe", b"exe".to_vec()), ("Data/level.bin.zst", zst.clone())]);
        let cfg = config_for(&dir);
        let rel = with_packing(release(&server.base, "1.0.0", &files), "Data/level.bin", &zst);
        let mut last = 0;
        update(&agent(), &cfg, &rel, &mut |p| if let Progress::Downloading { done, total } = p { assert!(done <= total); last = done }, &AtomicBool::new(false)).unwrap();
        let asked: Vec<String> = server.asked().into_iter().map(|(path, _)| path).collect();
        assert!(asked.contains(&"/files/Data/level.bin.zst".to_string()));
        assert!(!asked.contains(&"/files/Data/level.bin".to_string()));
        assert_eq!(fs::read(dir.join("Data").join("level.bin")).unwrap(), data);
        assert_eq!(last, rel.manifest.transfer_size());
        assert!(!cfg.state_dir().join(DOWNLOAD_DIR).exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_packed_copy_falls_back_to_the_whole_file() {
        let dir = scratch("packed-missing");
        let data = big(4);
        let zst = packed(&data);
        let files = [("game.exe", &data[..])];
        let server = serve(vec![("game.exe", data.clone())]);
        let cfg = config_for(&dir);
        run(&cfg, &with_packing(release(&server.base, "1.0.0", &files), "game.exe", &zst)).unwrap();
        let asked: Vec<String> = server.asked().into_iter().map(|(path, _)| path).collect();
        assert_eq!(asked, vec!["/files/game.exe.zst".to_string(), "/files/game.exe".to_string()]);
        assert_eq!(fs::read(dir.join("game.exe")).unwrap(), data);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_packed_copy_that_unpacks_wrong_installs_nothing() {
        let dir = scratch("packed-wrong");
        let data = big(5);
        let impostor = packed(&big(6));
        let files = [("game.exe", &data[..])];
        let server = serve(vec![("game.exe.zst", impostor.clone())]);
        let cfg = config_for(&dir);
        let error = run(&cfg, &with_packing(release(&server.base, "1.0.0", &files), "game.exe", &impostor)).unwrap_err();
        assert!(error.contains("did not unpack correctly"), "{error}");
        assert!(!dir.join("game.exe").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_game_is_downloaded_in_full() {
        let dir = scratch("fresh");
        let files = [("game.exe", &b"game v1"[..]), ("Data/level.bin", &b"level v1"[..])];
        let server = serve(files.iter().map(|(p, b)| (*p, b.to_vec())).collect());
        let cfg = config_for(&dir);
        let installed = run(&cfg, &release(&server.base, "1.0.0", &files)).unwrap();
        assert_eq!(installed.version, "1.0.0");
        assert_eq!(fs::read(dir.join("Data").join("level.bin")).unwrap(), b"level v1");
        assert_eq!(server.asked().len(), 2);
        assert!(!cfg.state_dir().join(DOWNLOAD_DIR).exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn files_that_already_match_are_never_downloaded() {
        let dir = scratch("present");
        let files = [("game.exe", &b"game v1"[..]), ("Data/level.bin", &b"level v1"[..])];
        fs::create_dir_all(dir.join("Data")).unwrap();
        fs::write(dir.join("game.exe"), b"game v1").unwrap();
        fs::write(dir.join("Data").join("level.bin"), b"level v1").unwrap();
        let server = serve(files.iter().map(|(p, b)| (*p, b.to_vec())).collect());
        let cfg = config_for(&dir);
        run(&cfg, &release(&server.base, "1.0.0", &files)).unwrap();
        assert!(server.asked().is_empty());
        assert_eq!(installed(&cfg).map(|i| i.version).as_deref(), Some("1.0.0"));
        run(&cfg, &release(&server.base, "1.0.0", &files)).unwrap();
        assert!(server.asked().is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_update_downloads_only_what_changed() {
        let dir = scratch("changed");
        let v1 = [("game.exe", &b"game v1"[..]), ("Data/level.bin", &b"level v1"[..]), ("Data/old.bin", &b"old"[..])];
        let v2 = [("game.exe", &b"game v1"[..]), ("Data/level.bin", &b"level v2!"[..])];
        let server = serve(vec![("game.exe", b"game v1".to_vec()), ("Data/level.bin", b"level v2!".to_vec()), ("Data/old.bin", b"old".to_vec())]);
        let cfg = config_for(&dir);
        fs::create_dir_all(dir.join("Data")).unwrap();
        for (path, bytes) in v1 {
            fs::write(local_path(&cfg, path), bytes).unwrap();
        }
        run(&cfg, &release(&server.base, "1.0.0", &v1)).unwrap();
        assert!(server.asked().is_empty());
        run(&cfg, &release(&server.base, "1.1.0", &v2)).unwrap();
        let asked: Vec<String> = server.asked().into_iter().map(|(path, _)| path).collect();
        assert_eq!(asked, vec!["/files/Data/level.bin".to_string()]);
        assert_eq!(fs::read(dir.join("Data").join("level.bin")).unwrap(), b"level v2!");
        assert!(!dir.join("Data").join("old.bin").exists());
        assert_eq!(installed(&cfg).map(|i| i.version).as_deref(), Some("1.1.0"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_interrupted_download_carries_on_from_where_it_stopped() {
        let dir = scratch("resume");
        let data = big(7);
        let files = [("game.exe", &data[..])];
        let server = serve(vec![("game.exe", data.clone())]);
        let cfg = config_for(&dir);
        let rel = release(&server.base, "1.0.0", &files);
        let download = cfg.state_dir().join(DOWNLOAD_DIR);
        fs::create_dir_all(&download).unwrap();
        fs::write(download.join(format!("{}.part", rel.manifest.files[0].sha256)), &data[..120_000]).unwrap();
        let mut seen = Vec::new();
        update(&agent(), &cfg, &rel, &mut |p| if let Progress::Downloading { done, .. } = p { seen.push(done) }, &AtomicBool::new(false)).unwrap();
        assert_eq!(server.asked(), vec![("/files/game.exe".to_string(), Some("bytes=120000-".to_string()))]);
        assert_eq!(fs::read(dir.join("game.exe")).unwrap(), data);
        assert!(seen.iter().all(|done| *done <= data.len() as u64));
        assert_eq!(seen.last().copied(), Some(data.len() as u64));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_damaged_partial_download_is_thrown_away_and_fetched_again() {
        let dir = scratch("damaged");
        let data = big(3);
        let files = [("game.exe", &data[..])];
        let server = serve(vec![("game.exe", data.clone())]);
        let cfg = config_for(&dir);
        let rel = release(&server.base, "1.0.0", &files);
        let download = cfg.state_dir().join(DOWNLOAD_DIR);
        fs::create_dir_all(&download).unwrap();
        fs::write(download.join(format!("{}.part", rel.manifest.files[0].sha256)), vec![0u8; 50_000]).unwrap();
        run(&cfg, &rel).unwrap();
        let asked = server.asked();
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[1].1, None);
        assert_eq!(fs::read(dir.join("game.exe")).unwrap(), data);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_changed_local_file_is_replaced() {
        let dir = scratch("edited");
        let files = [("game.exe", &b"game v1"[..])];
        let server = serve(vec![("game.exe", b"game v1".to_vec())]);
        let cfg = config_for(&dir);
        run(&cfg, &release(&server.base, "1.0.0", &files)).unwrap();
        server.asked();
        fs::write(dir.join("game.exe"), b"game XX").unwrap();
        run(&cfg, &release(&server.base, "1.0.0", &files)).unwrap();
        assert_eq!(server.asked().len(), 1);
        assert_eq!(fs::read(dir.join("game.exe")).unwrap(), b"game v1");
        let _ = fs::remove_dir_all(dir);
    }

    fn config_for(dir: &Path) -> Config {
        Config {
            auth_url: String::new(),
            files_url: String::new(),
            files_ref_url: String::new(),
            launcher_url: String::new(),
            game_address: String::new(),
            game_news_url: String::new(),
            playlist_feed_url: String::new(),
            discord_client_id: String::new(),
            discord_authorize_url: String::new(),
            redirect_ports: Vec::new(),
            install_dir: dir.to_path_buf(),
            prefs_location: String::new(),
            browser: None,
        }
    }

    #[test]
    fn moving_an_install_takes_only_the_game_files_and_its_state() {
        let root = std::env::temp_dir().join(format!("pr-launcher-move-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let old = root.join("old");
        let new = root.join("new");
        fs::create_dir_all(old.join("Project Reboot_Data")).unwrap();
        fs::write(old.join("Project Reboot.exe"), b"exe").unwrap();
        fs::write(old.join("Project Reboot_Data").join("data.unity3d"), b"data").unwrap();
        fs::write(old.join("my notes.txt"), b"keep me").unwrap();
        let cfg = config_for(&old);
        let mut state = State { version: "1.3.0".into(), exe: "Project Reboot.exe".into(), files: BTreeMap::new() };
        for (path, bytes) in [("Project Reboot.exe", &b"exe"[..]), ("Project Reboot_Data/data.unity3d", &b"data"[..])] {
            state.files.insert(path.into(), Known { size: bytes.len() as u64, modified: 0, sha256: pr_manifest::sha256_hex(bytes) });
        }
        save_state(&cfg, &state).unwrap();
        let mut steps = 0;
        move_install(&cfg, &new, &mut |_, _| steps += 1).unwrap();
        assert!(steps > 0);
        assert_eq!(fs::read(new.join("Project Reboot.exe")).unwrap(), b"exe");
        assert_eq!(fs::read(new.join("Project Reboot_Data").join("data.unity3d")).unwrap(), b"data");
        assert!(new.join(".launcher").join(STATE_FILE).is_file());
        assert_eq!(installed(&config_for(&new)).map(|i| i.version).as_deref(), Some("1.3.0"));
        assert!(!old.join("Project Reboot.exe").exists());
        assert!(!old.join("Project Reboot_Data").exists());
        assert_eq!(fs::read(old.join("my notes.txt")).unwrap(), b"keep me");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn moving_into_the_game_folder_itself_is_refused() {
        let root = std::env::temp_dir().join(format!("pr-launcher-move-inside-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let cfg = config_for(&root);
        assert!(move_install(&cfg, &root.join("inside"), &mut |_, _| {}).is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
