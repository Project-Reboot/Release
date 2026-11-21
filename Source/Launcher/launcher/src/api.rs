use std::fs;
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::{self, Config};

pub const MAX_POSTS: usize = 60;
pub const MAX_VIDEOS: usize = 15;
const MAX_AVATAR_BYTES: u64 = 256 * 1024;
const MAX_THUMBNAIL_BYTES: u64 = 2 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const GAME_NEWS_IMAGES: &str = "/Airbronze/RebootProdNews/";
const MAX_GAME_NEWS_BYTES: u64 = 1024 * 1024;
const MAX_FEED_BYTES: u64 = 2 * 1024 * 1024;
const MAX_GAME_POSTS: usize = 20;
const MAX_POST_BODY: usize = 16 * 1024;
const MAX_BSON_DEPTH: usize = 8;
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Post {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub image: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct News {
    #[serde(default)]
    posts: Vec<Post>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Video {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub date: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerState {
    Checking,
    Online { sign_in: bool, password: bool },
    MaintenanceSoon { starts: Option<u64>, sign_in: bool, password: bool },
    Maintenance { sign_in: bool, password: bool },
    Offline,
}

impl ServerState {
    pub fn sign_in(&self) -> bool {
        matches!(self, ServerState::Online { sign_in: true, .. } | ServerState::MaintenanceSoon { sign_in: true, .. } | ServerState::Maintenance { sign_in: true, .. })
    }

    pub fn password(&self) -> bool {
        matches!(self, ServerState::Online { password: true, .. } | ServerState::MaintenanceSoon { password: true, .. } | ServerState::Maintenance { password: true, .. })
    }
}

#[derive(Deserialize)]
struct Health {
    #[serde(default)]
    ok: bool,
    #[serde(rename = "signIn", default)]
    sign_in: bool,
    #[serde(rename = "passwordSignIn", default)]
    password_sign_in: bool,
    #[serde(default)]
    register: bool,
    #[serde(default)]
    maintenance: Option<String>,
    #[serde(rename = "maintenanceStarts", default)]
    maintenance_starts: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Playlists {
    #[serde(default)]
    pub official: Vec<Video>,
    #[serde(default)]
    pub featured: Vec<Video>,
}

impl Playlists {
    pub fn is_empty(&self) -> bool {
        self.official.is_empty() && self.featured.is_empty()
    }

    pub fn newest(&self, count: usize) -> Vec<Video> {
        self.official.iter().chain(self.featured.iter()).take(count).cloned().collect()
    }
}

enum Bson {
    Text(String),
    Doc(Vec<(String, Bson)>),
    Other,
}

fn read_i32(bytes: &[u8], at: usize) -> Option<usize> {
    let raw: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    usize::try_from(i32::from_le_bytes(raw)).ok()
}

fn read_bson_doc(bytes: &[u8], at: usize, depth: usize) -> Option<(Vec<(String, Bson)>, usize)> {
    if depth > MAX_BSON_DEPTH {
        return None;
    }
    let size = read_i32(bytes, at)?;
    let end = at.checked_add(size)?;
    if size < 5 || end > bytes.len() || bytes[end - 1] != 0 {
        return None;
    }
    let mut i = at + 4;
    let mut fields = Vec::new();
    while i < end - 1 {
        let kind = bytes[i];
        i += 1;
        let name_end = i + bytes.get(i..end)?.iter().position(|b| *b == 0)?;
        let name = String::from_utf8_lossy(&bytes[i..name_end]).to_string();
        i = name_end + 1;
        let skip = |i: usize, n: usize| i.checked_add(n).filter(|next| *next <= end - 1);
        let value = match kind {
            0x02 => {
                let length = read_i32(bytes, i)?;
                let start = i + 4;
                let stop = start.checked_add(length)?;
                if length == 0 || stop > end - 1 {
                    return None;
                }
                i = stop;
                Bson::Text(String::from_utf8_lossy(&bytes[start..stop - 1]).to_string())
            }
            0x03 | 0x04 => {
                let (doc, next) = read_bson_doc(bytes, i, depth + 1)?;
                if next > end - 1 {
                    return None;
                }
                i = next;
                Bson::Doc(doc)
            }
            0x01 | 0x09 | 0x11 | 0x12 => {
                i = skip(i, 8)?;
                Bson::Other
            }
            0x10 => {
                i = skip(i, 4)?;
                Bson::Other
            }
            0x08 => {
                i = skip(i, 1)?;
                Bson::Other
            }
            0x07 => {
                i = skip(i, 12)?;
                Bson::Other
            }
            0x05 => {
                let length = read_i32(bytes, i)?;
                i = skip(i, length.checked_add(5)?)?;
                Bson::Other
            }
            0x0A => Bson::Other,
            _ => return None,
        };
        fields.push((name, value));
    }
    Some((fields, end))
}

fn parse_game_news(bytes: &[u8]) -> Option<Vec<Post>> {
    let (root, _) = read_bson_doc(bytes, 0, 0)?;
    let items = root.into_iter().find_map(|(name, value)| match value {
        Bson::Doc(items) if name == "items" => Some(items),
        _ => None,
    })?;
    let mut posts = Vec::new();
    for (index, (_, item)) in items.into_iter().enumerate() {
        let Bson::Doc(fields) = item else { continue };
        let get = |key: &str| {
            fields.iter().find_map(|(name, value)| match value {
                Bson::Text(text) if name == key && !text.trim().is_empty() && text.trim() != "-" => Some(text.trim().to_string()),
                _ => None,
            })
        };
        let (Some(title), Some(mut body)) = (get("title"), get("longText")) else { continue };
        body = body.replace("\r\n", "\n");
        if body.len() > MAX_POST_BODY {
            let cut = (0..=MAX_POST_BODY).rev().find(|i| body.is_char_boundary(*i)).unwrap_or(0);
            body.truncate(cut);
        }
        let key = get("analyticsKey").filter(|k| k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')).unwrap_or_else(|| index.to_string());
        posts.push(Post { id: format!("game-{key}"), title, date: String::new(), body, image: get("imageUrl").filter(|link| is_news_image(link)) });
        if posts.len() >= MAX_GAME_POSTS {
            break;
        }
    }
    Some(posts)
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let stop = start + text[start..].find(close)?;
    Some(&text[start..stop])
}

fn xml_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let Some(semi) = tail.find(';').filter(|s| *s <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if entity.starts_with("#x") || entity.starts_with("#X") => u32::from_str_radix(&entity[2..], 16).ok().and_then(char::from_u32),
            _ if entity.starts_with('#') => entity[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_playlist_feed(xml: &str) -> Vec<Video> {
    xml.split("<entry>")
        .skip(1)
        .filter_map(|entry| {
            let id = between(entry, "<yt:videoId>", "</yt:videoId>")?.trim();
            let title = xml_text(between(entry, "<title>", "</title>")?);
            let date = between(entry, "<published>", "</published>").unwrap_or("").trim().to_string();
            (is_video_id(id) && !title.is_empty()).then(|| Video { id: id.to_string(), title, date })
        })
        .take(MAX_VIDEOS)
        .collect()
}

#[derive(Clone, Debug, Deserialize)]
pub struct SignIn {
    pub session: String,
    #[serde(rename = "displayName", default)]
    pub display_name: String,
    pub accounts: Vec<String>,
    #[serde(rename = "canCreate", default)]
    pub can_create: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Registration {
    pub token: String,
    pub account: String,
    #[serde(default)]
    pub remember: String,
    pub password: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Ticket {
    pub token: String,
    pub account: String,
    #[serde(default)]
    pub remember: String,
}

pub enum ResumeError {
    Expired,
    Unreachable(String),
}

pub fn resume(agent: &ureq::Agent, cfg: &Config, remember: &str) -> Result<Ticket, ResumeError> {
    match agent.post(&format!("{}/launcher/resume", cfg.auth_url)).send_json(json!({ "remember": remember })) {
        Ok(response) => response.into_json::<Ticket>().map_err(|_| ResumeError::Expired),
        Err(ureq::Error::Status(429 | 500..=599, _)) => Err(ResumeError::Unreachable("The Project Reboot server is busy. Try again in a minute.".into())),
        Err(ureq::Error::Status(..)) => Err(ResumeError::Expired),
        Err(ureq::Error::Transport(_)) => Err(ResumeError::Unreachable("Could not reach the Project Reboot server. Check your internet connection and try again.".into())),
    }
}

pub fn forget(agent: &ureq::Agent, cfg: &Config, remember: &str) {
    let _ = agent.post(&format!("{}/launcher/forget", cfg.auth_url)).send_json(json!({ "remember": remember }));
}

#[derive(Deserialize)]
struct AvatarAnswer {
    png: String,
    #[serde(default)]
    rank: Option<u8>,
    #[serde(default)]
    name: Option<String>,
}

pub struct Avatar {
    pub png: Vec<u8>,
    pub rank: Option<u8>,
    pub name: Option<String>,
}

pub fn avatar(agent: &ureq::Agent, cfg: &Config, token: &str) -> Option<Avatar> {
    avatar_for(agent, cfg, json!({ "token": token, "scale": 1, "view": "head" }))
}

pub fn remembered_avatar(agent: &ureq::Agent, cfg: &Config, remember: &str) -> Option<Avatar> {
    avatar_for(agent, cfg, json!({ "remember": remember, "scale": 1, "view": "head" }))
}

fn avatar_for(agent: &ureq::Agent, cfg: &Config, body: Value) -> Option<Avatar> {
    use base64::Engine;
    let response = agent.post(&format!("{}/launcher/avatar", cfg.auth_url)).send_json(body).ok()?;
    let answer: AvatarAnswer = serde_json::from_slice(&read_limited(response, MAX_AVATAR_BYTES).ok()?).ok()?;
    let png = base64::engine::general_purpose::STANDARD.decode(answer.png).ok()?;
    let name = answer.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty() && n.len() <= 32);
    Some(Avatar { png, rank: answer.rank, name })
}

pub fn made_account() -> bool {
    cache_path("created-account.txt").is_file()
}

pub fn mark_made_account(account: &str) {
    let path = cache_path("created-account.txt");
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, account);
}

pub fn cached_account() -> Option<String> {
    fs::read_to_string(cache_path("account.txt")).ok().map(|n| n.trim().to_string()).filter(|n| !n.is_empty())
}

pub fn save_account(account: Option<&str>) {
    let path = cache_path("account.txt");
    match account {
        Some(name) => {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(path, name);
        }
        None => {
            let _ = fs::remove_file(path);
        }
    }
}

fn account_file(account: &str) -> String {
    account.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect()
}

fn avatar_path(account: &str) -> PathBuf {
    cache_path(&format!("avatar-{}.png", account_file(account)))
}

fn rank_path(account: &str) -> PathBuf {
    cache_path(&format!("rank-{}.txt", account_file(account)))
}

pub fn cached_rank(account: &str) -> Option<u8> {
    fs::read_to_string(rank_path(account)).ok()?.trim().parse().ok()
}

pub fn save_rank(account: &str, rank: u8) {
    let path = rank_path(account);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, rank.to_string());
}

pub fn cached_avatar(account: &str) -> Option<Vec<u8>> {
    fs::read(avatar_path(account)).ok()
}

pub fn save_avatar(account: &str, png: &[u8]) {
    let path = avatar_path(account);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, png);
}

fn post(agent: &ureq::Agent, url: &str, body: Value) -> Result<Value, String> {
    match agent.post(url).send_json(body) {
        Ok(response) => response.into_json().map_err(|_| "The Project Reboot server sent an answer the launcher did not understand.".to_string()),
        Err(ureq::Error::Status(_, response)) => {
            let message = response
                .into_json::<Value>()
                .ok()
                .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_string))
                .filter(|m| !m.is_empty());
            Err(message.unwrap_or_else(|| "The Project Reboot server refused the sign-in.".to_string()))
        }
        Err(ureq::Error::Transport(_)) => Err("Could not reach the Project Reboot server. Check your internet connection and try again.".to_string()),
    }
}

pub fn sign_in(agent: &ureq::Agent, cfg: &Config, code: &str, redirect_uri: &str, verifier: &str) -> Result<SignIn, String> {
    let value = post(agent, &format!("{}/launcher/signin", cfg.auth_url), json!({ "code": code, "redirectUri": redirect_uri, "codeVerifier": verifier, "allowNew": true }))?;
    serde_json::from_value(value).map_err(|_| "The Project Reboot server sent an answer the launcher did not understand.".to_string())
}

pub fn register(agent: &ureq::Agent, cfg: &Config, session: &str) -> Result<Registration, String> {
    let value = post(agent, &format!("{}/launcher/register", cfg.auth_url), json!({ "session": session }))?;
    let made: Registration = serde_json::from_value(value).map_err(|_| "The Project Reboot server sent an answer the launcher did not understand.".to_string())?;
    if made.account.is_empty() || made.account.len() > 64 || made.password.is_empty() || made.password.len() > 128 {
        return Err("The Project Reboot server sent an answer the launcher did not understand.".to_string());
    }
    Ok(made)
}

pub fn password_sign_in(agent: &ureq::Agent, cfg: &Config, name: &str, password: &str) -> Result<Ticket, String> {
    let value = post(agent, &format!("{}/launcher/login", cfg.auth_url), json!({ "name": name, "password": password }))?;
    serde_json::from_value(value).map_err(|_| "The Project Reboot server sent an answer the launcher did not understand.".to_string())
}

pub fn ticket(agent: &ureq::Agent, cfg: &Config, session: &str, account: &str) -> Result<Ticket, String> {
    let value = post(agent, &format!("{}/launcher/token", cfg.auth_url), json!({ "session": session, "account": account }))?;
    serde_json::from_value(value).map_err(|_| "The Project Reboot server sent an answer the launcher did not understand.".to_string())
}

fn read_limited(response: ureq::Response, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    response.into_reader().take(limit + 1).read_to_end(&mut bytes).map_err(|e| format!("the download was interrupted: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err("the answer was too large".into());
    }
    Ok(bytes)
}

fn parse_news(bytes: &[u8]) -> Option<Vec<Post>> {
    let news: News = serde_json::from_slice(bytes).ok()?;
    Some(news.posts.into_iter().filter(|p| !p.id.is_empty() && !p.title.trim().is_empty()).take(MAX_POSTS).collect())
}

fn cache_path(name: &str) -> PathBuf {
    config::data_dir().join("Launcher").join(name)
}

fn save_cache(name: &str, value: &impl Serialize) {
    let path = cache_path(name);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec(value) {
        let _ = fs::write(path, bytes);
    }
}

pub fn cached_game_news() -> Vec<Post> {
    fs::read(cache_path("game-news.json")).ok().and_then(|bytes| parse_news(&bytes)).unwrap_or_default()
}

pub fn game_news(agent: &ureq::Agent, cfg: &Config) -> Result<Vec<Post>, String> {
    let response = agent.get(&cfg.game_news_url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("the news server answered {code}"),
        ureq::Error::Transport(t) => format!("could not reach the news server ({})", t.kind()),
    })?;
    let posts = parse_game_news(&read_limited(response, MAX_GAME_NEWS_BYTES)?).ok_or_else(|| "the game news could not be read".to_string())?;
    if !posts.is_empty() {
        save_cache("game-news.json", &News { posts: posts.clone() });
    }
    Ok(posts)
}

fn playlist(agent: &ureq::Agent, cfg: &Config, id: &str) -> Result<Vec<Video>, String> {
    let response = agent.get(&format!("{}{id}", cfg.playlist_feed_url)).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("YouTube answered {code}"),
        ureq::Error::Transport(t) => format!("could not reach YouTube ({})", t.kind()),
    })?;
    let bytes = read_limited(response, MAX_FEED_BYTES)?;
    Ok(parse_playlist_feed(&String::from_utf8_lossy(&bytes)))
}

pub fn cached_playlists() -> Playlists {
    fs::read(cache_path("playlists.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

pub fn playlists(agent: &ureq::Agent, cfg: &Config) -> Result<Playlists, String> {
    let (official, featured) = std::thread::scope(|scope| {
        let featured = scope.spawn(|| playlist(agent, cfg, config::FEATURED_PLAYLIST));
        let official = playlist(agent, cfg, config::OFFICIAL_PLAYLIST);
        (official, featured.join().unwrap_or_else(|_| Err("the video list stopped unexpectedly".to_string())))
    });
    if let (Err(e), Err(_)) = (&official, &featured) {
        return Err(e.clone());
    }
    let lists = Playlists { official: official.unwrap_or_default(), featured: featured.unwrap_or_default() };
    if !lists.is_empty() {
        save_cache("playlists.json", &lists);
    }
    Ok(lists)
}

pub fn is_video_id(id: &str) -> bool {
    id.len() == 11 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn watch_url(id: &str) -> Option<String> {
    is_video_id(id).then(|| format!("https://www.youtube.com/watch?v={id}"))
}

pub fn video_thumbnail(agent: &ureq::Agent, id: &str) -> Result<Vec<u8>, String> {
    if !is_video_id(id) {
        return Err("not a YouTube video".into());
    }
    let response = agent.get(&format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg")).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("YouTube answered {code}"),
        ureq::Error::Transport(t) => format!("could not reach YouTube ({})", t.kind()),
    })?;
    read_limited(response, MAX_THUMBNAIL_BYTES)
}

pub fn is_news_image(link: &str) -> bool {
    url::Url::parse(link)
        .map(|u| {
            u.scheme() == "https"
                && u.host_str() == Some("raw.githubusercontent.com")
                && u.path().starts_with(GAME_NEWS_IMAGES)
        })
        .unwrap_or(false)
}

pub fn news_image(agent: &ureq::Agent, link: &str) -> Result<Vec<u8>, String> {
    if !is_news_image(link) {
        return Err("pictures only load from the game news".into());
    }
    let response = agent.get(link).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("the picture server answered {code}"),
        ureq::Error::Transport(t) => format!("could not reach the picture server ({})", t.kind()),
    })?;
    read_limited(response, MAX_IMAGE_BYTES)
}

pub fn short_date(iso: &str) -> String {
    let mut parts = iso.get(..10).unwrap_or("").split('-');
    match (parts.next(), parts.next().and_then(|m| m.parse::<usize>().ok()), parts.next().and_then(|d| d.parse::<u32>().ok())) {
        (Some(year), Some(month @ 1..=12), Some(day)) if year.len() == 4 => format!("{} {day}, {year}", MONTHS[month - 1]),
        _ => String::new(),
    }
}

pub fn plain_line(line: &str) -> &str {
    let trimmed = line.trim_start();
    for marker in ["### ", "## ", "# ", "-# ", "- "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            return rest.trim();
        }
    }
    trimmed.trim_end()
}

static REGISTER_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn register_open() -> bool {
    REGISTER_OPEN.load(std::sync::atomic::Ordering::Relaxed)
}

fn health_allows_register(bytes: &[u8]) -> bool {
    serde_json::from_slice::<Health>(bytes).map(|h| h.ok && h.sign_in && h.register).unwrap_or(false)
}

fn parse_health(bytes: &[u8]) -> Option<ServerState> {
    let health: Health = serde_json::from_slice(bytes).ok()?;
    if !health.ok {
        return None;
    }

    let (sign_in, password) = (health.sign_in, health.password_sign_in);
    Some(match health.maintenance.as_deref() {
        Some("active") => ServerState::Maintenance { sign_in, password },
        Some("scheduled") => ServerState::MaintenanceSoon { starts: health.maintenance_starts.as_deref().and_then(unix_seconds), sign_in, password },
        _ => ServerState::Online { sign_in, password },
    })
}

pub fn unix_seconds(iso: &str) -> Option<u64> {
    let (date, time) = iso.trim().trim_end_matches('Z').split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (year, month, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|p| p.split('.').next().and_then(|v| v.parse::<i64>().ok()));
    let (hour, minute, second) = (t.next()??, t.next()??, t.next().flatten().unwrap_or(0));
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    u64::try_from(days * 86400 + hour * 3600 + minute * 60 + second).ok()
}

pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn game_port_state(address: &str) -> ServerState {
    let reachable = address.to_socket_addrs().map(|addrs| addrs.take(2).any(|a| TcpStream::connect_timeout(&a, Duration::from_secs(3)).is_ok())).unwrap_or(false);
    if reachable {
        ServerState::Online { sign_in: false, password: false }
    } else {
        ServerState::Offline
    }
}

pub fn server_state(cfg: &Config, may_probe_game_port: bool) -> Option<ServerState> {
    let quick = ureq::AgentBuilder::new()
        .user_agent(config::USER_AGENT)
        .timeout_connect(Duration::from_secs(3))
        .timeout_read(Duration::from_secs(4))
        .build();
    if let Ok(response) = quick.get(&format!("{}/launcher/health", cfg.auth_url)).call() {
        if let Ok(bytes) = read_limited(response, 16 * 1024) {
            if let Some(state) = parse_health(&bytes) {
                REGISTER_OPEN.store(health_allows_register(&bytes), std::sync::atomic::Ordering::Relaxed);
                return Some(state);
            }
        }
    }
    may_probe_game_port.then(|| game_port_state(&cfg.game_address))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_read_like_people_write_them() {
        assert_eq!(short_date("2026-11-10T18:00:00Z"), "Nov 10, 2026");
        assert_eq!(short_date("2026-01-02"), "Jan 2, 2026");
        assert_eq!(short_date("nonsense"), "");
        assert_eq!(short_date("2026-13-01T00:00:00Z"), "");
    }

    #[test]
    fn pictures_only_come_from_the_game_news() {
        assert!(!is_news_image("https://cdn.discordapp.com/attachments/1/2/fishing.png"));
        assert!(is_news_image("https://raw.githubusercontent.com/Airbronze/RebootProdNews/master/Assets/newsPRgamechanges.png"));
        assert!(!is_news_image("https://raw.githubusercontent.com/someone/else/master/x.png"));
        assert!(!is_news_image("http://raw.githubusercontent.com/Airbronze/RebootProdNews/master/Assets/x.png"));
        assert!(!is_news_image("https://raw.githubusercontent.com.evil.example/Airbronze/RebootProdNews/x.png"));
        assert!(!is_news_image("https://evil.example/raw.githubusercontent.com/Airbronze/RebootProdNews/x.png"));
    }

    fn bson(fields: &[(&str, Option<&[u8]>, Option<Vec<u8>>)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (name, text, doc) in fields {
            match (text, doc) {
                (Some(text), _) => {
                    body.push(0x02);
                    body.extend_from_slice(name.as_bytes());
                    body.push(0);
                    body.extend_from_slice(&((text.len() + 1) as i32).to_le_bytes());
                    body.extend_from_slice(text);
                    body.push(0);
                }
                (None, Some(doc)) => {
                    body.push(0x03);
                    body.extend_from_slice(name.as_bytes());
                    body.push(0);
                    body.extend_from_slice(doc);
                }
                _ => {}
            }
        }
        let mut out = ((body.len() + 5) as i32).to_le_bytes().to_vec();
        out.extend(body);
        out.push(0);
        out
    }

    #[test]
    fn the_game_news_file_gives_its_text_posts() {
        let link = bson(&[("action", Some(b"0"), None), ("analyticsKey", Some(b"Discord"), None), ("title", Some(b"Join our Discord"), None), ("longText", Some(b"-"), None)]);
        let notes = bson(&[
            ("analyticsKey", Some(b"PatchNotes"), None),
            ("title", Some(b"Patch Notes v1.3.0"), None),
            ("longText", Some(b"Fishing Update!\r\n-Bank Bot"), None),
            ("imageUrl", Some(b"https://raw.githubusercontent.com/Airbronze/RebootProdNews/master/Assets/newsPRgamechanges.png"), None),
        ]);
        let welcome = bson(&[("title", Some(b"New to Project Reboot?"), None), ("longText", Some(b"Nice to meet you!"), None), ("imageUrl", Some(b"https://evil.example/x.png"), None)]);
        let items = bson(&[("0", None, Some(link)), ("1", None, Some(notes)), ("2", None, Some(welcome))]);
        let file = bson(&[("items", None, Some(items))]);
        let posts = parse_game_news(&file).unwrap();
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0], Post {
            id: "game-PatchNotes".into(),
            title: "Patch Notes v1.3.0".into(),
            date: String::new(),
            body: "Fishing Update!\n-Bank Bot".into(),
            image: Some("https://raw.githubusercontent.com/Airbronze/RebootProdNews/master/Assets/newsPRgamechanges.png".into()),
        });
        assert_eq!(posts[1].id, "game-2");
        assert_eq!(posts[1].image, None);
    }

    #[test]
    fn a_broken_news_file_is_refused_not_crashed_on() {
        let good = bson(&[("items", None, Some(bson(&[("0", None, Some(bson(&[("title", Some(b"a"), None)])))])))]);
        for cut in 0..good.len() {
            let _ = parse_game_news(&good[..cut]);
        }
        let mut deep = bson(&[("title", Some(b"x"), None)]);
        for _ in 0..20 {
            deep = bson(&[("items", None, Some(deep))]);
        }
        assert!(parse_game_news(&deep).is_none());
        assert!(parse_game_news(b"not bson at all").is_none());
    }

    #[test]
    fn a_playlist_feed_gives_its_videos() {
        let xml = r#"<?xml version="1.0"?><feed><title>Project Reboot Official Videos</title>
            <entry><id>yt:video:SB4sOBmg_EQ</id><yt:videoId>SB4sOBmg_EQ</yt:videoId><title>Halloween 2026 Update Teaser | Project Reboot</title>
            <published>2026-10-03T16:00:00+00:00</published><media:group><media:title>ignored</media:title></media:group></entry>
            <entry><yt:videoId>bad id!</yt:videoId><title>Broken</title></entry>
            <entry><yt:videoId>qKzncQhhuBQ</yt:videoId><title>Fish &amp; Chips &#8211; &quot;Live&quot;
              Stream</title><published>2026-06-09T17:00:00+00:00</published></entry></feed>"#;
        let videos = parse_playlist_feed(xml);
        assert_eq!(videos.len(), 2);
        assert_eq!(videos[0], Video { id: "SB4sOBmg_EQ".into(), title: "Halloween 2026 Update Teaser | Project Reboot".into(), date: "2026-10-03T16:00:00+00:00".into() });
        assert_eq!(videos[1].title, "Fish & Chips \u{2013} \"Live\" Stream");
        assert_eq!(xml_text("a &unknown; b & c &#x41;"), "a &unknown; b & c A");
    }

    #[test]
    fn saved_news_is_read_and_bad_posts_dropped() {
        let posts = parse_news(br#"{"posts":[{"id":"2","title":"Fishing Update","date":"2026-11-10T18:00:00Z","body":"- New rods","image":null},{"id":"","title":"x"},{"id":"1","title":"  "}]}"#).unwrap();
        assert_eq!(posts, vec![Post { id: "2".into(), title: "Fishing Update".into(), date: "2026-11-10T18:00:00Z".into(), body: "- New rods".into(), image: None }]);
        assert!(parse_news(b"not json").is_none());
    }

    #[test]
    fn only_real_video_ids_make_links() {
        assert_eq!(watch_url("SB4sOBmg_EQ").as_deref(), Some("https://www.youtube.com/watch?v=SB4sOBmg_EQ"));
        assert_eq!(watch_url("javascript:"), None);
        assert_eq!(watch_url("SB4sOBmg_EQ&list=x"), None);
        assert_eq!(watch_url("a/../../b.c"), None);
    }

    #[test]
    fn the_health_check_reads_sign_in_and_maintenance() {
        assert_eq!(parse_health(br#"{"ok":true,"signIn":true,"passwordSignIn":true,"maintenance":"none"}"#), Some(ServerState::Online { sign_in: true, password: true }));
        assert_eq!(parse_health(br#"{"ok":true}"#), Some(ServerState::Online { sign_in: false, password: false }));
        assert!(health_allows_register(br#"{"ok":true,"signIn":true,"register":true}"#));
        assert!(!health_allows_register(br#"{"ok":true,"signIn":false,"register":true}"#));
        assert!(!health_allows_register(br#"{"ok":true,"signIn":true}"#));
        assert_eq!(parse_health(br#"{"ok":true,"signIn":true,"maintenance":"active"}"#), Some(ServerState::Maintenance { sign_in: true, password: false }));
        assert_eq!(
            parse_health(br#"{"ok":true,"maintenance":"scheduled","maintenanceStarts":"2026-11-19T18:30:00Z"}"#),
            Some(ServerState::MaintenanceSoon { starts: Some(1795113000), sign_in: false, password: false })
        );
        assert_eq!(parse_health(br#"{"ok":false}"#), None);
        assert_eq!(parse_health(b"<html>bad gateway</html>"), None);
    }

    #[test]
    fn times_from_the_server_turn_into_unix_seconds() {
        assert_eq!(unix_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_seconds("2026-01-01T00:00:00Z"), Some(1767225600));
        assert_eq!(unix_seconds("2024-02-29T12:34:56Z"), Some(1709210096));
        assert_eq!(unix_seconds("2026-13-01T00:00:00Z"), None);
        assert_eq!(unix_seconds("soon"), None);
    }

    #[test]
    fn the_game_port_tells_online_from_offline() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let open = listener.local_addr().unwrap().to_string();
        assert_eq!(game_port_state(&open), ServerState::Online { sign_in: false, password: false });
        drop(listener);
        assert_eq!(game_port_state(&open), ServerState::Offline);
    }

    #[test]
    fn markers_are_dropped_for_previews() {
        assert_eq!(plain_line("## Fishing"), "Fishing");
        assert_eq!(plain_line("  - New rods"), "New rods");
        assert_eq!(plain_line("-# small"), "small");
        assert_eq!(plain_line("Plain text"), "Plain text");
    }
}
