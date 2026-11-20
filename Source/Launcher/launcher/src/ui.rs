use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Id, Mesh, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, UiBuilder, Vec2, ViewportCommand,
};

use crate::api::{self, SignIn, Ticket};
use crate::config::{self, Config};
use crate::discord;
use crate::game::{self, Ended};
use crate::gameicon;
use crate::log;
use crate::prefs;
use crate::remember;
use crate::selfupdate::{self, CURRENT_VERSION};
use crate::update::{self, Installed, Progress};

pub const WIDTH: f32 = 1180.0;
pub const HEIGHT: f32 = 720.0;

const SIDEBAR: f32 = 240.0;
const HEADER: f32 = 108.0;
const HERO: f32 = 364.0;
const STRIP: f32 = 14.0;
const SAFE: f32 = 20.0;

const WINDOW: Color32 = Color32::from_rgb(0, 0, 0);
const SIDE: Color32 = Color32::from_rgb(14, 14, 14);
const MAIN: Color32 = Color32::from_rgb(14, 8, 48);
const NAVY_BOTTOM: Color32 = Color32::from_rgb(25, 20, 77);
const HEADER_BG: Color32 = Color32::from_rgb(27, 27, 27);
const TITLE_GREY: Color32 = Color32::from_rgb(140, 140, 140);
const TAB_GREY: Color32 = Color32::from_rgb(122, 122, 122);
const SIDE_HOVER: Color32 = Color32::from_rgb(30, 30, 30);
const SIDE_SELECTED: Color32 = Color32::from_rgb(46, 46, 46);
const CARD_HOVER: Color32 = Color32::from_rgb(70, 70, 70);
const EDGE: Color32 = Color32::from_rgb(43, 43, 43);
const EDGE_STRONG: Color32 = Color32::from_rgb(12, 12, 12);
const SELECTED: Color32 = Color32::from_rgb(80, 80, 80);
const FIELD: Color32 = Color32::from_rgb(26, 26, 26);
const ROW: Color32 = Color32::from_rgb(40, 40, 40);
const ROW_HOVER: Color32 = Color32::from_rgb(62, 62, 62);
const INK: Color32 = Color32::from_rgb(0, 0, 0);
const GREEN: Color32 = Color32::from_rgb(29, 186, 68);
const GREEN_HOVER: Color32 = Color32::from_rgb(70, 220, 105);
const GREEN_EDGE: Color32 = Color32::from_rgb(20, 110, 40);
const TRACK: Color32 = Color32::from_rgb(18, 18, 18);
const ACCENT: Color32 = Color32::from_rgb(255, 120, 240);
const MAGENTA: Color32 = Color32::from_rgb(255, 21, 233);
const TEXT: Color32 = Color32::from_rgb(245, 245, 250);
const MUTED: Color32 = Color32::from_rgb(205, 205, 215);
const FAINT: Color32 = Color32::from_rgb(145, 145, 160);
const WARN: Color32 = Color32::from_rgb(255, 205, 96);
const ERROR: Color32 = Color32::from_rgb(255, 120, 112);
const CLOSE_RED: Color32 = Color32::from_rgb(196, 43, 28);
const YOUTUBE_RED: Color32 = Color32::from_rgb(204, 32, 32);

const BOLD: &str = "bold";
const SEMI: &str = "semibold";
const HEAVY: &str = "heavy";
const BLOCKY: &str = "blocktopia";

#[derive(Clone, Copy)]
enum Skin {
    Blue,
    Gray,
    Green,
    Red,
}

struct Skins {
    buttons: [egui::TextureId; 4],
    tables: [egui::TextureId; 4],
}

static SKINS: std::sync::OnceLock<Skins> = std::sync::OnceLock::new();

fn skins() -> &'static Skins {
    SKINS.get().expect("frames are loaded with the art")
}

impl Skin {
    fn texture(self) -> egui::TextureId {
        skins().buttons[self as usize]
    }
}

fn rank_table(rank: Option<u8>) -> egui::TextureId {
    let index = match rank {
        Some(1) => 0,
        Some(3) => 2,
        Some(2) => 3,
        _ => 1,
    };
    skins().tables[index]
}

fn rank_label(rank: Option<u8>) -> Option<&'static str> {
    match rank {
        Some(1) => Some("Moderator"),
        Some(2) => Some("Admin"),
        Some(3) => Some("Influencer"),
        _ => None,
    }
}

enum Msg {
    Server(api::ServerState),
    PlayDecision(api::ServerState),
    SetupUnavailable(String),
    Playlists(api::Playlists),
    GameNews(Vec<api::Post>),
    VideoThumb(String, egui::ColorImage),
    NewsImage(String, egui::ColorImage),
    SetupSize(u64),
    Moved(Result<std::path::PathBuf, String>),
    SignInLink(String),
    Status(String, Option<f32>),
    Release { version: String, notes: Option<String> },
    Poster(egui::ColorImage),
    Updated(Result<Installed, String>),
    Restarting,
    SelfUpdateFailed(String),
    SignedIn(Result<SignIn, String>),
    Ticket(Result<Ticket, String>),
    Resumed(Result<Ticket, api::ResumeError>),
    PasswordSignedIn(Result<Ticket, String>, String),
    GameEnded(Ended),
    Avatar(Option<String>, api::Avatar),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Page {
    Play,
    Notes,
    Videos,
    Logs,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Retry {
    Update,
    Ready,
    SignIn,
}

enum Phase {
    Updating { text: String, progress: Option<f32> },
    Ready,
    SigningIn { text: String },
    Choose { session: String, accounts: Vec<String> },
    SignIn { name: String, password: String, error: Option<String>, discord: bool },
    Playing { account: String, token: String },
    Failed { message: String, retry: Retry },
}

struct Art {
    logo: TextureHandle,
    icon: TextureHandle,
    avatar: TextureHandle,
    discord: TextureHandle,
    youtube_icon: TextureHandle,
    youtube: TextureHandle,
    whats_new: TextureHandle,
    settings: TextureHandle,
    console: TextureHandle,
    team: TextureHandle,
    poster: TextureHandle,
    _frames: Vec<TextureHandle>,
}

impl Art {
    fn load(ctx: &egui::Context) -> Art {
        let t = |name: &str, bytes: &[u8]| load_texture(ctx, name, bytes);
        let frames = vec![
            t("button-blue", include_bytes!("../../assets/ui/button-blue.png")),
            t("button-gray", include_bytes!("../../assets/ui/button-gray.png")),
            t("button-green", include_bytes!("../../assets/ui/button-green.png")),
            t("button-red", include_bytes!("../../assets/ui/button-red.png")),
            t("table-blue", include_bytes!("../../assets/ui/table-blue.png")),
            t("table-gray", include_bytes!("../../assets/ui/table-gray.png")),
            t("table-green", include_bytes!("../../assets/ui/table-green.png")),
            t("table-purple", include_bytes!("../../assets/ui/table-purple.png")),
        ];
        let id = |i: usize| frames[i].id();
        let _ = SKINS.set(Skins { buttons: [id(0), id(1), id(2), id(3)], tables: [id(4), id(5), id(6), id(7)] });
        Art {
            logo: t("logo", include_bytes!("../../assets/ui/game-logo.png")),
            icon: t("icon", include_bytes!("../../assets/ui/game-icon.png")),
            avatar: t("avatar", include_bytes!("../../assets/ui/avatar.png")),
            discord: t("discord", include_bytes!("../../assets/ui/icon-discord.png")),
            youtube_icon: t("youtube-icon", include_bytes!("../../assets/ui/icon-youtube.png")),
            youtube: t("youtube-banner", include_bytes!("../../assets/ui/youtube-banner.jpg")),
            whats_new: t("whats-new", include_bytes!("../../assets/ui/icon-whats-new.png")),
            settings: t("settings", include_bytes!("../../assets/ui/icon-settings.png")),
            console: t("console", include_bytes!("../../assets/ui/icon-console.png")),
            team: t("team", include_bytes!("../../assets/ui/reboot-team.png")),
            poster: t("poster", include_bytes!("../../assets/ui/poster-default.jpg")),
            _frames: frames,
        }
    }
}

pub struct LauncherApp {
    cfg: Config,
    agent: ureq::Agent,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    phase: Phase,
    page: Page,
    page_since: f64,
    installed: Option<Installed>,
    latest_version: Option<String>,
    notes: Option<String>,
    notice: Option<String>,
    discord_name: Option<String>,
    account_name: Option<String>,
    rank: Option<u8>,
    avatar: Option<(String, TextureHandle, Vec2)>,
    cover: crate::cover::Cover,
    avatar_checked: f64,
    cancel: Arc<AtomicBool>,
    art: Art,
    poster: Option<TextureHandle>,
    log_text: String,
    client_log: bool,
    game_icon: Option<(std::path::PathBuf, Option<std::time::SystemTime>, TextureHandle)>,
    log_checked: f64,
    log_seen: Option<(std::path::PathBuf, u64, Option<std::time::SystemTime>)>,
    sign_in_link: Option<String>,
    wine: Option<String>,
    news_selected: Option<String>,
    news_images: HashMap<String, TextureHandle>,
    news_asked: HashSet<String>,
    news_checked: Option<f64>,
    playlists: api::Playlists,
    game_news: Vec<api::Post>,
    video_thumbs: HashMap<String, TextureHandle>,
    thumbs_asked: HashSet<String>,
    videos_checked: Option<f64>,
    server: api::ServerState,
    server_checked: f64,
    setup: bool,
    play_after_update: bool,
    setup_size: Option<u64>,
    setup_unavailable: Option<String>,
    setup_error: Option<String>,
    launcher_login: bool,
    ctx: egui::Context,
}

fn decode_picture(bytes: &[u8], max: u32) -> Option<egui::ColorImage> {
    let picture = image::load_from_memory(bytes).ok()?.thumbnail(max, max).to_rgba8();
    let size = [picture.width() as usize, picture.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, picture.as_raw()))
}

fn load_texture(ctx: &egui::Context, name: &str, bytes: &[u8]) -> TextureHandle {
    let image = image::load_from_memory(bytes).expect("built-in image").to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    ctx.load_texture(name, egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()), TextureOptions::LINEAR)
}

pub fn window_icon() -> egui::IconData {
    let image = image::load_from_memory(include_bytes!("../../assets/app-icon.png")).expect("built-in icon").to_rgba8();
    egui::IconData { width: image.width(), height: image.height(), rgba: image.into_raw() }
}

fn font_candidates() -> [Vec<std::path::PathBuf>; 4] {
    if cfg!(windows) {
        let dir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into()).join("Fonts");
        [vec![dir.join("segoeui.ttf")], vec![dir.join("segoeuib.ttf")], vec![dir.join("seguisb.ttf"), dir.join("segoeuib.ttf")], vec![dir.join("seguibl.ttf"), dir.join("segoeuib.ttf")]]
    } else {
        let roots = ["/usr/share/fonts/truetype", "/usr/share/fonts", "/usr/local/share/fonts"];
        let pick = |names: &[&str]| -> Vec<std::path::PathBuf> {
            roots.iter().flat_map(|root| names.iter().map(move |name| std::path::Path::new(root).join(name))).collect()
        };
        [
            pick(&["noto/NotoSans-Regular.ttf", "google-noto/NotoSans-Regular.ttf", "dejavu/DejaVuSans.ttf", "TTF/DejaVuSans.ttf"]),
            pick(&["noto/NotoSans-Bold.ttf", "google-noto/NotoSans-Bold.ttf", "dejavu/DejaVuSans-Bold.ttf", "TTF/DejaVuSans-Bold.ttf"]),
            pick(&["noto/NotoSans-SemiBold.ttf", "google-noto/NotoSans-SemiBold.ttf", "noto/NotoSans-Bold.ttf", "dejavu/DejaVuSans-Bold.ttf", "TTF/DejaVuSans-Bold.ttf"]),
            pick(&["noto/NotoSans-Black.ttf", "google-noto/NotoSans-Black.ttf", "noto/NotoSans-ExtraBold.ttf", "noto/NotoSans-Bold.ttf", "dejavu/DejaVuSans-Bold.ttf", "TTF/DejaVuSans-Bold.ttf"]),
        ]
    }
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let [regular_files, bold_files, semi_files, heavy_files] = font_candidates();
    if let Some(bytes) = regular_files.iter().find_map(|path| std::fs::read(path).ok()) {
        fonts.font_data.insert("system".into(), Arc::new(FontData::from_owned(bytes)));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "system".into());
    }
    for (family, files) in [(BOLD, bold_files), (SEMI, semi_files), (HEAVY, heavy_files)] {
        let mut chain = fallbacks.clone();
        if let Some(bytes) = files.iter().find_map(|path| std::fs::read(path).ok()) {
            let name = format!("system-{family}");
            fonts.font_data.insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
            chain.insert(0, name);
        }
        fonts.families.insert(FontFamily::Name(family.into()), chain);
    }
    let mut chain = Vec::new();
    #[cfg(button_font)]
    {
        fonts.font_data.insert(BLOCKY.into(), Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/blocktopia.ttf"))));
        chain.push(BLOCKY.to_string());
    }
    chain.extend(fonts.families.get(&FontFamily::Name(HEAVY.into())).cloned().unwrap_or_default());
    fonts.families.insert(FontFamily::Name(BLOCKY.into()), chain);
    ctx.set_fonts(fonts);
}

fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD.into()))
}

fn semi(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMI.into()))
}

fn heavy(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(HEAVY.into()))
}

fn blocky(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BLOCKY.into()))
}

fn style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = MAIN;
    visuals.extreme_bg_color = WINDOW;
    visuals.selection.bg_fill = GREEN_EDGE;
    visuals.override_text_color = Some(TEXT);
    ctx.set_visuals(visuals);
    ctx.style_mut(|s| s.spacing.scroll = egui::style::ScrollStyle::thin());
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

impl LauncherApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> LauncherApp {
        let loading = std::time::Instant::now();
        let (tx, rx) = channel();
        let ctx = cc.egui_ctx.clone();
        install_fonts(&ctx);
        style(&ctx);
        let cfg = Config::load();
        let mut app = LauncherApp {
            installed: update::installed(&cfg),
            cfg,
            agent: update::agent(),
            tx,
            rx,
            phase: Phase::Ready,
            page: Page::Play,
            page_since: 0.0,
            latest_version: None,
            notes: None,
            notice: None,
            discord_name: None,
            account_name: remember::load().map(|r| r.account).or_else(api::cached_account),
            rank: None,
            avatar: None,
            cover: crate::cover::Cover::new(&ctx),
            avatar_checked: 0.0,
            cancel: Arc::new(AtomicBool::new(false)),
            art: Art::load(&ctx),
            poster: None,
            log_text: String::new(),
            client_log: false,
            game_icon: None,
            log_checked: 0.0,
            log_seen: None,
            sign_in_link: None,
            wine: game::wine_version(),
            news_selected: None,
            news_images: HashMap::new(),
            news_asked: HashSet::new(),
            news_checked: None,
            playlists: api::cached_playlists(),
            game_news: api::cached_game_news(),
            video_thumbs: HashMap::new(),
            thumbs_asked: HashSet::new(),
            videos_checked: None,
            server: api::ServerState::Checking,
            server_checked: 0.0,
            setup: config::needs_setup(),
            play_after_update: false,
            setup_size: None,
            setup_unavailable: None,
            setup_error: None,
            launcher_login: false,
            ctx,
        };
        if let Some(account) = app.account_name.clone() {
            app.rank = api::cached_rank(&account);
            if let Some(png) = api::cached_avatar(&account) {
                app.show_avatar(&account, &png);
            }
        }
        match remember::load() {
            Some(saved) => {
                let (cfg, agent) = (app.cfg.clone(), app.agent.clone());
                app.spawn(move |tx| {
                    if let Some(avatar) = api::remembered_avatar(&agent, &cfg, &saved.remember) {
                        let _ = tx.send(Msg::Avatar(Some(saved.account), avatar));
                    }
                });
            }
            None => app.fetch_game_login_avatar(),
        }
        log::info(format!("Launcher {CURRENT_VERSION} started (loaded in {} ms)", loading.elapsed().as_millis()));
        if !app.setup {
            app.cfg.remember_install_dir();
        }
        if let Some(version) = &app.wine {
            log::info(format!("Running under Wine {version} (Linux, Proton or Steam Deck)"));
        }
        #[cfg(not(windows))]
        if let Err(e) = crate::desktop::install() {
            log::warn(format!("Could not add the launcher to the app menu: {e}"));
        }
        selfupdate::remove_leftovers();
        app.fetch_news();
        app.fetch_videos();
        app.check_server(true);
        if let Some(newest) = app.posts().first().cloned() {
            app.want_image(&newest);
        }
        if app.setup {
            log::info("First start: asking where to keep the game");
            app.phase = Phase::Updating { text: "Choose where to keep the game".into(), progress: None };
            app.fetch_setup_size();
        } else {
            app.start_update();
        }
        app
    }

    fn go(&mut self, page: Page) {
        if page == Page::Logs {
            self.log_text = self.read_log(96 * 1024);
        }
        if page == Page::Notes && self.news_checked.map(|at| self.ctx.input(|i| i.time) - at > 300.0).unwrap_or(false) {
            self.fetch_news();
        }
        if page == Page::Videos && self.videos_checked.map(|at| self.ctx.input(|i| i.time) - at > 600.0).unwrap_or(false) {
            self.fetch_videos();
        }
        if self.page != page {
            self.page = page;
            self.page_since = self.ctx.input(|i| i.time);
        }
    }

    fn spawn(&self, job: impl FnOnce(&Sender<Msg>) + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            job(&tx);
            ctx.request_repaint();
        });
    }

    fn show_avatar(&mut self, account: &str, png: &[u8]) -> bool {
        let Ok(picture) = image::load_from_memory(png) else {
            return false;
        };
        let picture = picture.to_rgba8();
        let size = [picture.width() as usize, picture.height() as usize];
        let (mut sum_x, mut sum_y, mut weight) = (0.0f32, 0.0f32, 0.0f32);
        for (x, y, pixel) in picture.enumerate_pixels() {
            let alpha = pixel[3] as f32;
            sum_x += (x as f32 + 0.5) * alpha;
            sum_y += (y as f32 + 0.5) * alpha;
            weight += alpha;
        }
        let focus = if weight > 0.0 { Vec2::new(sum_x / weight / size[0] as f32, sum_y / weight / size[1] as f32) } else { Vec2::splat(0.5) };
        let texture = self.ctx.load_texture(format!("avatar-{account}"), egui::ColorImage::from_rgba_unmultiplied(size, picture.as_raw()), TextureOptions::NEAREST);
        self.avatar = Some((account.to_string(), texture, focus));
        true
    }

    fn fetch_avatar(&mut self, account: Option<String>, token: String) {
        if token.is_empty() {
            return;
        }
        self.avatar_checked = self.ctx.input(|i| i.time);
        let (cfg, agent) = (self.cfg.clone(), self.agent.clone());
        self.spawn(move |tx| {
            if let Some(avatar) = api::avatar(&agent, &cfg, &token) {
                let _ = tx.send(Msg::Avatar(account, avatar));
            }
        });
    }

    fn fetch_game_login_avatar(&mut self) {
        if remember::load().is_some() {
            return;
        }
        if let Some(token) = prefs::read_token(&self.cfg.prefs_location).filter(|t| !t.is_empty()) {
            self.fetch_avatar(None, token);
        }
    }

    fn fresh_cancel(&mut self) -> Arc<AtomicBool> {
        self.cancel = Arc::new(AtomicBool::new(false));
        self.cancel.clone()
    }

    fn busy(&self) -> bool {
        matches!(self.phase, Phase::Updating { .. } | Phase::SigningIn { .. } | Phase::Playing { .. } | Phase::Choose { .. } | Phase::SignIn { .. })
    }

    fn start_update(&mut self) {
        self.phase = Phase::Updating { text: "Checking for updates".into(), progress: None };
        let cfg = self.cfg.clone();
        let agent = self.agent.clone();
        let cancel = self.fresh_cancel();
        let ctx = self.ctx.clone();
        self.spawn(move |tx| {
            if !game::running_from(&cfg.install_dir).is_empty() {
                let installed = update::installed(&cfg).ok_or_else(|| "Project Reboot is running. Close it and try again.".to_string());
                let _ = tx.send(Msg::Updated(installed));
                return;
            }
            log::info("Checking for updates");
            let checking = std::time::Instant::now();
            let git_ref = update::newest_ref(&agent, &cfg);
            let (launchers, release) = std::thread::scope(|scope| {
                let launchers = scope.spawn(|| selfupdate::fetch_list(&agent, &cfg, &git_ref));
                let release = update::fetch_release(&agent, &cfg, &git_ref);
                (launchers.join().unwrap_or_else(|_| Err("the launcher update check stopped unexpectedly".to_string())), release)
            });
            match launchers {
                Ok(launchers) => {
                    if let Some(build) = selfupdate::newer(&launchers.list).cloned() {
                        let label = format!("Updating the launcher to {}", launchers.list.version);
                        log::info(&label);
                        let _ = tx.send(Msg::Status(label.clone(), Some(0.0)));
                        let total = build.size.max(1);
                        let progress_tx = tx.clone();
                        let progress_ctx = ctx.clone();
                        let result = selfupdate::install_and_restart(&agent, &launchers, &build, &mut |received| {
                            let _ = progress_tx.send(Msg::Status(label.clone(), Some(received as f32 / total as f32)));
                            progress_ctx.request_repaint();
                        }, &cancel);
                        match result {
                            Ok(()) => {
                                log::info("Launcher updated, restarting");
                                let _ = tx.send(Msg::Restarting);
                                return;
                            }
                            Err(e) => {
                                log::error(format!("Launcher self-update failed: {e}"));
                                let _ = tx.send(Msg::SelfUpdateFailed(e));
                            }
                        }
                    }
                }
                Err(e) => log::warn(format!("Launcher update check failed: {e}")),
            }
            let release = match release {
                Ok(release) => release,
                Err(e) => {
                    log::warn(format!("Update check failed: {e}"));
                    let _ = tx.send(Msg::Updated(Err(e)));
                    return;
                }
            };
            log::info(format!("Latest release is {} (update lists fetched in {} ms)", release.manifest.version, checking.elapsed().as_millis()));
            let _ = tx.send(Msg::Release { version: release.manifest.version.clone(), notes: release.manifest.notes.clone() });
            if let Some(picture) = update::poster(&agent, &cfg, &release, &cancel).and_then(|bytes| decode_picture(&bytes, 1024)) {
                let _ = tx.send(Msg::Poster(picture));
                ctx.request_repaint();
            }
            let progress_tx = tx.clone();
            let progress_ctx = ctx.clone();
            let first_install = update::installed(&cfg).is_none();
            let result = update::update(&agent, &cfg, &release, &mut |p| {
                let (text, fraction) = match p {
                    Progress::Checking { done, total } => (format!("Checking game files {}/{}", done + 1, total), Some(done as f32 / total.max(1) as f32)),
                    Progress::Downloading { done, total } => (
                        format!(
                            "{} {:.0} of {:.0} MB",
                            if first_install { "Installing Project Reboot:" } else { "Downloading the update:" },
                            done as f64 / 1_048_576.0,
                            total as f64 / 1_048_576.0
                        ),
                        Some(done as f32 / total.max(1) as f32),
                    ),
                };
                let _ = progress_tx.send(Msg::Status(text, fraction));
                progress_ctx.request_repaint();
            }, &cancel);
            match &result {
                Ok(installed) => log::info(format!("Game files are up to date (version {}, checked in {} ms)", installed.version, checking.elapsed().as_millis())),
                Err(e) => log::error(format!("Game update failed: {e}")),
            }
            let _ = tx.send(Msg::Updated(result));
        });
    }

    fn start_sign_in(&mut self) {
        if !game::running_from(&self.cfg.install_dir).is_empty() {
            self.phase = Phase::Failed { message: "Project Reboot is already running.".into(), retry: Retry::Ready };
            return;
        }
        self.notice = None;
        log::info("Starting Discord sign-in");
        self.phase = Phase::SigningIn { text: "Waiting for Discord in your browser".into() };
        let cfg = self.cfg.clone();
        let agent = self.agent.clone();
        let cancel = self.fresh_cancel();
        self.sign_in_link = None;
        self.spawn(move |tx| {
            let link_tx = tx.clone();
            let result = discord::sign_in(&cfg, &cancel, &move |link| {
                let _ = link_tx.send(Msg::SignInLink(link));
            })
            .and_then(|c| api::sign_in(&agent, &cfg, &c.code, &c.redirect_uri, &c.verifier));
            if !cancel.load(Ordering::Relaxed) {
                let _ = tx.send(Msg::SignedIn(result));
            }
        });
    }

    fn start_remembered_or_sign_in(&mut self) {
        let Some(saved) = remember::load() else {
            self.ask_sign_in(String::new(), None);
            return;
        };
        self.phase = Phase::SigningIn { text: format!("Logging in as {}", saved.account) };
        let cfg = self.cfg.clone();
        let agent = self.agent.clone();
        self.spawn(move |tx| {
            let _ = tx.send(Msg::Resumed(api::resume(&agent, &cfg, &saved.remember)));
        });
    }

    fn ask_sign_in(&mut self, name: String, error: Option<String>) {
        let discord = self.server.sign_in() && self.cfg.discord_ready();
        if !self.server.password() {
            self.start_sign_in();
            return;
        }
        self.phase = Phase::SignIn { name, password: String::new(), error, discord };
    }

    fn start_password_sign_in(&mut self, name: String, password: String) {
        log::info(format!("Signing in as {name} with a password"));
        self.phase = Phase::SigningIn { text: format!("Logging in as {name}") };
        let cfg = self.cfg.clone();
        let agent = self.agent.clone();
        self.spawn(move |tx| {
            let _ = tx.send(Msg::PasswordSignedIn(api::password_sign_in(&agent, &cfg, &name, &password), name));
        });
    }

    fn request_ticket(&mut self, session: String, account: String) {
        self.phase = Phase::SigningIn { text: format!("Logging in as {account}") };
        let cfg = self.cfg.clone();
        let agent = self.agent.clone();
        self.spawn(move |tx| {
            let _ = tx.send(Msg::Ticket(api::ticket(&agent, &cfg, &session, &account)));
        });
    }

    fn start_game(&mut self, ticket: Ticket) {
        let Some(installed) = self.installed.clone() else {
            self.phase = Phase::Failed { message: "The game is not installed yet.".into(), retry: Retry::Update };
            return;
        };
        self.launcher_login = true;
        if let Err(e) = prefs::write_login(&self.cfg.prefs_location, &ticket.token).and_then(|_| game::launch(&installed.exe)) {
            log::error(format!("Could not start the game: {e}"));
            let _ = prefs::clear_login(&self.cfg.prefs_location);
            self.phase = Phase::Failed { message: e, retry: Retry::Ready };
            return;
        }
        log::info(format!("Started the game (version {})", installed.version));
        self.phase = Phase::Playing { account: ticket.account, token: ticket.token };
        self.ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        let dir = self.cfg.install_dir.clone();
        let cancel = self.fresh_cancel();
        self.spawn(move |tx| {
            let _ = tx.send(Msg::GameEnded(game::wait_until_closed(&dir, &cancel)));
        });
    }

    fn check_server(&mut self, may_probe_game_port: bool) {
        self.server_checked = self.ctx.input(|i| i.time);
        let cfg = self.cfg.clone();
        self.spawn(move |tx| {
            if let Some(state) = api::server_state(&cfg, may_probe_game_port) {
                let _ = tx.send(Msg::Server(state));
            }
        });
    }

    fn start_play(&mut self) {
        if !game::running_from(&self.cfg.install_dir).is_empty() {
            self.phase = Phase::Failed { message: "Project Reboot is already running.".into(), retry: Retry::Ready };
            return;
        }
        self.phase = Phase::SigningIn { text: "Checking the server".into() };
        self.server_checked = self.ctx.input(|i| i.time);
        let cfg = self.cfg.clone();
        self.spawn(move |tx| {
            let state = api::server_state(&cfg, true).unwrap_or(api::ServerState::Offline);
            let _ = tx.send(Msg::PlayDecision(state));
        });
    }

    fn start_plain_game(&mut self) {
        let Some(installed) = self.installed.clone() else {
            self.phase = Phase::Failed { message: "The game is not installed yet.".into(), retry: Retry::Update };
            return;
        };
        self.launcher_login = false;
        if let Err(e) = game::launch(&installed.exe) {
            log::error(format!("Could not start the game: {e}"));
            self.phase = Phase::Failed { message: e, retry: Retry::Ready };
            return;
        }
        log::info(format!("Started the game without launcher sign-in (version {})", installed.version));
        self.phase = Phase::Playing { account: String::new(), token: String::new() };
        self.ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        let dir = self.cfg.install_dir.clone();
        let cancel = self.fresh_cancel();
        self.spawn(move |tx| {
            let _ = tx.send(Msg::GameEnded(game::wait_until_closed(&dir, &cancel)));
        });
    }

    fn fetch_news(&mut self) {
        self.news_checked = Some(self.ctx.input(|i| i.time));
        let game_cfg = self.cfg.clone();
        let game_agent = self.agent.clone();
        self.spawn(move |tx| match api::game_news(&game_agent, &game_cfg) {
            Ok(posts) => {
                log::info(format!("Loaded {} game news posts", posts.len()));
                let _ = tx.send(Msg::GameNews(posts));
            }
            Err(e) => log::warn(format!("Could not load the game news: {e}")),
        });
    }

    fn fetch_videos(&mut self) {
        self.videos_checked = Some(self.ctx.input(|i| i.time));
        let cfg = self.cfg.clone();
        let agent = self.agent.clone();
        self.spawn(move |tx| match api::playlists(&agent, &cfg) {
            Ok(lists) => {
                log::info(format!("Loaded {} official and {} featured videos from YouTube", lists.official.len(), lists.featured.len()));
                let _ = tx.send(Msg::Playlists(lists));
            }
            Err(e) => log::warn(format!("Could not load the YouTube videos: {e}")),
        });
    }

    fn want_thumb(&mut self, id: &str) {
        if !api::is_video_id(id) || !self.thumbs_asked.insert(id.to_string()) {
            return;
        }
        let agent = self.agent.clone();
        let id = id.to_string();
        self.spawn(move |tx| match api::video_thumbnail(&agent, &id).map(|bytes| decode_picture(&bytes, 640)) {
            Ok(Some(picture)) => {
                let _ = tx.send(Msg::VideoThumb(id, picture));
            }
            Ok(None) => log::warn("A video picture could not be read"),
            Err(e) => log::warn(format!("Could not load a video picture: {e}")),
        });
    }

    fn video_card(&mut self, ui: &mut egui::Ui, rect: Rect, video: &api::Video, section: &str) {
        self.want_thumb(&video.id);
        let (response, hover) = hit(ui, rect, &format!("video-{section}-{}", video.id), &video.title, true);
        let painter = ui.painter().clone();
        let lift = hover * 3.0;
        let frame = rect.translate(Vec2::new(0.0, -lift));
        card(&painter, frame, hover);
        let thumb = Rect::from_min_size(frame.min + Vec2::splat(8.0), Vec2::new(frame.width() - 16.0, (frame.width() - 16.0) * 9.0 / 16.0));
        match self.video_thumbs.get(&video.id) {
            Some(texture) => cover(&painter.with_clip_rect(thumb), texture, thumb, 0.5),
            None => {
                painter.rect_filled(thumb, CornerRadius::same(6), INK);
            }
        }
        painter.rect_stroke(thumb, CornerRadius::same(6), Stroke::new(1.0, mix(EDGE, EDGE_STRONG, hover)), StrokeKind::Outside);
        let badge = Rect::from_center_size(thumb.center(), Vec2::new(54.0, 38.0) * (1.0 + hover * 0.08));
        painter.rect_filled(badge, CornerRadius::same(10), mix(Color32::from_rgba_unmultiplied(20, 20, 20, 200), YOUTUBE_RED, 0.35 + hover * 0.65));
        let c = badge.center();
        painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(-7.0, -10.0), c + Vec2::new(11.0, 0.0), c + Vec2::new(-7.0, 10.0)], Color32::WHITE, Stroke::NONE));
        let inner = Rect::from_min_max(Pos2::new(frame.left() + 14.0, thumb.bottom() + 10.0), Pos2::new(frame.right() - 14.0, frame.bottom() - 10.0));
        let clip = painter.with_clip_rect(Rect::from_min_max(inner.min, Pos2::new(inner.right(), inner.top() + 40.0)));
        let title = painter.layout(video.title.clone(), semi(14.5), TEXT, inner.width());
        clip.galley(inner.min, title, TEXT);
        let date = api::short_date(&video.date);
        text(&painter, Pos2::new(inner.left(), frame.bottom() - 12.0), Align2::LEFT_BOTTOM, if date.is_empty() { "YouTube".to_string() } else { format!("YouTube  ·  {date}") }, regular(12.0), mix(FAINT, MUTED, hover));
        if response.clicked() {
            if let Some(link) = api::watch_url(&video.id) {
                log::info("Opening a video on YouTube");
                let _ = open::that_detached(link);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn video_row(&mut self, ui: &mut egui::Ui, left: f32, top: f32, width: f32, videos: &[api::Video], columns: usize, section: &str) -> f32 {
        let gap = 16.0;
        let card_width = (width - gap * (columns as f32 - 1.0)) / columns as f32;
        let card_height = (card_width - 16.0) * 9.0 / 16.0 + 16.0 + 74.0;
        let mut bottom = top;
        for (index, video) in videos.iter().enumerate() {
            let column = index % columns;
            let row = index / columns;
            let rect = Rect::from_min_size(Pos2::new(left + column as f32 * (card_width + gap), top + row as f32 * (card_height + gap)), Vec2::new(card_width, card_height));
            self.video_card(ui, rect, video, section);
            bottom = rect.bottom();
        }
        bottom
    }

    fn videos_page(&mut self, ui: &mut egui::Ui, origin: Pos2, width: f32) -> f32 {
        let painter = ui.painter().clone();
        let inner_width = (width - 80.0).min(960.0);
        let left = origin.x + (width - inner_width) / 2.0;
        let banner = Rect::from_min_size(Pos2::new(left, origin.y + PAD), Vec2::new(inner_width, (inner_width * 254.0 / 1400.0).max(150.0)));
        cover(&painter.with_clip_rect(banner), &self.art.youtube, banner, 0.5);
        vertical_fade(&painter, Rect::from_min_max(Pos2::new(banner.left(), banner.center().y), banner.max), Color32::TRANSPARENT, Color32::from_rgba_unmultiplied(4, 10, 30, 220));
        painter.rect_stroke(banner, CornerRadius::same(10), Stroke::new(1.0, EDGE_STRONG), StrokeKind::Outside);
        text(&painter, Pos2::new(banner.left() + PAD, banner.bottom() - 46.0), Align2::LEFT_CENTER, "PROJECT REBOOT ON YOUTUBE", bold(11.5), ACCENT);
        text(&painter, Pos2::new(banner.left() + PAD, banner.bottom() - 24.0), Align2::LEFT_CENTER, "Trailers, update teasers and live streams", semi(17.0), TEXT);
        let label = "Open channel";
        let button_size = Vec2::new(button_width(&painter, label) + 8.0, BUTTON_H);
        let place = Rect::from_min_size(Pos2::new(banner.right() - PAD - button_size.x, banner.bottom() - 18.0 - button_size.y), button_size);
        if button(ui, place, "open-youtube", label, Skin::Red, true) {
            let _ = open::that_detached(config::YOUTUBE_URL);
        }
        let mut y = banner.bottom() + GAP + 4.0;
        if self.playlists.is_empty() {
            text(&painter, Pos2::new(left + inner_width / 2.0, y + 60.0), Align2::CENTER_CENTER, "The videos could not be loaded right now. Check your internet connection.", regular(14.5), MUTED);
            return y + 120.0 - origin.y;
        }
        let sections = [("official", "OFFICIAL VIDEOS", self.playlists.official.clone()), ("featured", "COMMUNITY FEATURED", self.playlists.featured.clone())];
        for (key, heading, videos) in sections {
            if videos.is_empty() {
                continue;
            }
            text(&painter, Pos2::new(left, y + 10.0), Align2::LEFT_CENTER, heading, bold(13.0), ACCENT);
            y = self.video_row(ui, left, y + 32.0, inner_width, &videos, 3, key) + GAP + 12.0;
        }
        y + PAD - origin.y
    }

    fn want_image(&mut self, post: &api::Post) {
        let Some(link) = post.image.clone() else { return };
        if !api::is_news_image(&link) || !self.news_asked.insert(post.id.clone()) {
            return;
        }
        let agent = self.agent.clone();
        let id = post.id.clone();
        self.spawn(move |tx| match api::news_image(&agent, &link).map(|bytes| decode_picture(&bytes, 1280)) {
            Ok(Some(picture)) => {
                let _ = tx.send(Msg::NewsImage(id, picture));
            }
            Ok(None) => log::warn("A patch notes picture could not be read"),
            Err(e) => log::warn(format!("Could not load a patch notes picture: {e}")),
        });
    }

    fn posts(&self) -> Vec<api::Post> {
        let posts: Vec<api::Post> = self.game_news.clone();
        if !posts.is_empty() {
            return posts;
        }
        match &self.notes {
            Some(notes) => vec![api::Post { id: "release".into(), title: format!("Project Reboot {}", self.version_text()), date: String::new(), body: notes.clone(), image: None }],
            None => Vec::new(),
        }
    }

    fn post_picture(&self, post: &api::Post) -> Option<TextureHandle> {
        self.news_images.get(&post.id).cloned().or_else(|| if post.id == "release" || (post.image.is_none() && self.posts().first().map(|p| p.id == post.id).unwrap_or(false)) { self.poster.clone() } else { None })
    }

    fn fetch_setup_size(&mut self) {
        let cfg = self.cfg.clone();
        let agent = self.agent.clone();
        self.spawn(move |tx| match update::fetch_release(&agent, &cfg, &update::newest_ref(&agent, &cfg)) {
            Ok(release) => {
                let _ = tx.send(Msg::SetupSize(release.manifest.transfer_size()));
                let _ = tx.send(Msg::Release { version: release.manifest.version.clone(), notes: release.manifest.notes.clone() });
            }
            Err(e) => {
                log::warn(format!("Could not check the download size: {e}"));
                let _ = tx.send(Msg::SetupUnavailable(e));
            }
        });
    }

    fn game_exe_name(&self) -> String {
        self.installed
            .as_ref()
            .and_then(|i| i.exe.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| if cfg!(windows) { "Project Reboot.exe".into() } else { "Project Reboot.x86_64".into() })
    }

    fn pick_folder(&self, title: &str) -> Option<std::path::PathBuf> {
        let start = self.cfg.install_dir.parent().map(|p| p.to_path_buf()).filter(|p| p.is_dir()).or_else(config::desktop_dir);
        let mut dialog = rfd::FileDialog::new().set_title(title);
        if let Some(start) = start {
            dialog = dialog.set_directory(start);
        }
        dialog.pick_folder()
    }

    fn finish_setup(&mut self, target: std::path::PathBuf) {
        if let Err(e) = config::save_install_dir(&target) {
            self.setup_error = Some(e);
            return;
        }
        log::info("Game folder chosen on first start");
        self.setup = false;
        self.setup_error = None;
        self.cfg.install_dir = target;
        self.installed = update::installed(&self.cfg);
        self.start_update();
    }

    fn change_folder(&mut self) {
        let Some(chosen) = self.pick_folder("Choose where to keep Project Reboot") else { return };
        let target = config::install_target(&chosen, &self.game_exe_name());
        if target == self.cfg.install_dir {
            return;
        }
        if let Err(e) = config::save_install_dir(&target) {
            self.phase = Phase::Failed { message: e, retry: Retry::Ready };
            return;
        }
        log::info("Changing the game folder");
        self.go(Page::Play);
        if self.installed.is_none() {
            self.cfg.install_dir = target;
            self.start_update();
            return;
        }
        self.phase = Phase::Updating { text: "Moving game files".into(), progress: Some(0.0) };
        let cfg = self.cfg.clone();
        let ctx = self.ctx.clone();
        self.spawn(move |tx| {
            let progress_tx = tx.clone();
            let result = update::move_install(&cfg, &target, &mut |done, total| {
                let _ = progress_tx.send(Msg::Status("Moving game files".into(), Some(done as f32 / total.max(1) as f32)));
                ctx.request_repaint();
            });
            if result.is_err() {
                let _ = config::save_install_dir(&cfg.install_dir);
            }
            let _ = tx.send(Msg::Moved(result.map(|_| target)));
        });
    }

    fn verify_files(&mut self) {
        log::info("Verifying game files");
        if let Err(e) = update::forget_hashes(&self.cfg) {
            self.notice = Some(e);
        }
        self.go(Page::Play);
        self.start_update();
    }

    fn handle(&mut self, msg: Msg) {
        if !matches!(msg, Msg::SignInLink(_)) && !matches!(self.phase, Phase::SigningIn { .. }) {
            self.sign_in_link = None;
        }
        match msg {
            Msg::Server(state) => {
                if state != self.server {
                    log::info(format!("Server status: {}", server_label(&state)));
                }
                self.server = state;
            }
            Msg::PlayDecision(state) => {
                self.server = state.clone();
                if !matches!(self.phase, Phase::SigningIn { .. }) {
                    return;
                }
                match state {
                    api::ServerState::Offline => {
                        log::warn("Play pressed but the server is not answering");
                        self.phase = Phase::Failed { message: "The Project Reboot server isn't answering right now. It may be restarting, so try again in a minute.".into(), retry: Retry::Ready };
                    }
                    state if (state.sign_in() && self.cfg.discord_ready()) || state.password() => self.start_remembered_or_sign_in(),
                    _ => self.start_plain_game(),
                }
            }
            Msg::SetupUnavailable(reason) => self.setup_unavailable = Some(reason),
            Msg::Playlists(lists) => {
                if !lists.is_empty() {
                    self.playlists = lists;
                }
            }
            Msg::GameNews(posts) => {
                if !posts.is_empty() {
                    self.game_news = posts;
                    if let Some(newest) = self.posts().first().cloned() {
                        self.want_image(&newest);
                    }
                }
            }
            Msg::VideoThumb(id, picture) => {
                let texture = self.ctx.load_texture(format!("video-{id}"), picture, TextureOptions::LINEAR);
                self.video_thumbs.insert(id, texture);
            }
            Msg::NewsImage(id, picture) => {
                let texture = self.ctx.load_texture(format!("news-{id}"), picture, TextureOptions::LINEAR);
                self.news_images.insert(id, texture);
            }
            Msg::SetupSize(size) => self.setup_size = Some(size),
            Msg::Avatar(expected, avatar) => {
                let Some(account) = expected.clone().or_else(|| avatar.name.clone()) else { return };
                if expected.is_some() && self.account_name.as_deref() != Some(account.as_str()) {
                    return;
                }
                if expected.is_none() {
                    if self.account_name.as_deref() != Some(account.as_str()) {
                        self.rank = api::cached_rank(&account);
                    }
                    self.account_name = Some(account.clone());
                    api::save_account(Some(&account));
                }
                if self.show_avatar(&account, &avatar.png) {
                    api::save_avatar(&account, &avatar.png);
                }
                if let Some(rank) = avatar.rank {
                    self.rank = Some(rank);
                    api::save_rank(&account, rank);
                }
            }
            Msg::Moved(Ok(target)) => {
                log::info("Moved the game to its new folder");
                self.cfg.install_dir = target;
                self.installed = update::installed(&self.cfg);
                self.notice = Some(format!("Project Reboot now lives in {}", self.cfg.install_dir.display()));
                self.start_update();
            }
            Msg::Moved(Err(e)) => {
                log::error(format!("Could not move the game: {e}"));
                self.phase = Phase::Failed { message: format!("Could not move the game: {e}"), retry: Retry::Ready };
            }
            Msg::SignInLink(link) => self.sign_in_link = Some(link),
            Msg::Status(text, progress) => {
                if let Phase::Updating { .. } = self.phase {
                    self.phase = Phase::Updating { text, progress };
                }
            }
            Msg::Release { version, notes } => {
                self.latest_version = Some(version);
                self.notes = notes;
            }
            Msg::Poster(picture) => self.poster = Some(self.ctx.load_texture("release-poster", picture, TextureOptions::LINEAR)),
            Msg::Restarting => self.ctx.send_viewport_cmd(ViewportCommand::Close),
            Msg::SelfUpdateFailed(e) => self.notice = Some(format!("The launcher could not update itself ({e}). See Logs for details.")),
            Msg::Updated(Ok(installed)) => {
                self.installed = Some(installed);
                self.phase = Phase::Ready;
                if std::mem::take(&mut self.play_after_update) {
                    self.start_play();
                }
            }
            Msg::Updated(Err(e)) => match update::installed(&self.cfg) {
                Some(installed) => {
                    self.notice = Some(format!("Could not check for updates ({e}). You can still play version {}.", installed.version));
                    self.installed = Some(installed);
                    self.phase = Phase::Ready;
                    if std::mem::take(&mut self.play_after_update) {
                        self.start_play();
                    }
                }
                None => {
                    self.play_after_update = false;
                    let message = if update::is_not_published(&e) && !cfg!(windows) {
                        format!("The Linux version of Project Reboot isn't out yet. {}", config::PROTON_HELP)
                    } else {
                        format!("Could not download Project Reboot: {e}")
                    };
                    self.phase = Phase::Failed { message, retry: Retry::Update };
                }
            },
            Msg::SignedIn(Ok(sign_in)) => {
                log::info(format!("Signed in with Discord ({} linked account(s))", sign_in.accounts.len()));
                self.discord_name = Some(sign_in.display_name.clone()).filter(|n| !n.is_empty());
                match sign_in.accounts.as_slice() {
                    [only] => self.request_ticket(sign_in.session, only.clone()),
                    [] => self.phase = Phase::Failed { message: "No Project Reboot account is linked to this Discord account.".into(), retry: Retry::SignIn },
                    _ => self.phase = Phase::Choose { session: sign_in.session, accounts: sign_in.accounts },
                }
            }
            Msg::SignedIn(Err(e)) | Msg::Ticket(Err(e)) => {
                log::error(format!("Sign-in failed: {e}"));
                self.phase = Phase::Failed { message: e, retry: Retry::SignIn };
            }
            Msg::Ticket(Ok(ticket)) | Msg::Resumed(Ok(ticket)) | Msg::PasswordSignedIn(Ok(ticket), _) => {
                if let Err(e) = remember::save(&ticket.account, &ticket.remember) {
                    log::warn(format!("Could not keep this PC signed in: {e}"));
                }
                if self.account_name.as_deref() != Some(ticket.account.as_str()) {
                    self.rank = api::cached_rank(&ticket.account);
                }
                self.account_name = Some(ticket.account.clone()).filter(|a| !a.is_empty());
                api::save_account(self.account_name.as_deref());
                if self.avatar.as_ref().is_some_and(|(owner, _, _)| *owner != ticket.account) {
                    self.avatar = None;
                }
                self.fetch_avatar(Some(ticket.account.clone()), ticket.token.clone());
                self.start_game(ticket)
            }
            Msg::Resumed(Err(api::ResumeError::Expired)) => {
                log::info("The saved sign-in has expired; signing in again");
                remember::clear();
                api::save_account(None);
                self.account_name = None;
                self.rank = None;
                self.avatar = None;
                self.ask_sign_in(String::new(), None);
            }
            Msg::PasswordSignedIn(Err(e), name) => {
                log::warn(format!("Password sign-in failed: {e}"));
                self.ask_sign_in(name, Some(e));
            }
            Msg::Resumed(Err(api::ResumeError::Unreachable(e))) => {
                log::error(format!("Sign-in failed: {e}"));
                self.phase = Phase::Failed { message: e, retry: Retry::Ready };
            }
            Msg::GameEnded(ended) => {
                if std::mem::take(&mut self.launcher_login) {
                    let _ = prefs::clear_login(&self.cfg.prefs_location);
                }
                match ended {
                    Ended::NeverStarted => {
                        log::warn("The game did not start within 90 seconds");
                        self.notice = Some("The game did not start. Try again, or use Verify game files in Settings.".into());
                    }
                    Ended::Closed => log::info("The game closed"),
                    Ended::Cancelled => log::info("Stopped waiting for the game"),
                }
                self.phase = Phase::Ready;
                self.fetch_game_login_avatar();
                self.ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                self.ctx.send_viewport_cmd(ViewportCommand::Focus);
            }
        }
    }

    fn version_text(&self) -> String {
        self.latest_version.clone().or_else(|| self.installed.as_ref().map(|i| i.version.clone())).unwrap_or_else(|| "Not installed".into())
    }
}

const FULL_UV: Rect = Rect { min: Pos2::ZERO, max: Pos2 { x: 1.0, y: 1.0 } };

fn image(painter: &egui::Painter, texture: &TextureHandle, rect: Rect) {
    painter.image(texture.id(), rect, FULL_UV, Color32::WHITE);
}

fn character(painter: &egui::Painter, texture: &TextureHandle, focus: Vec2, center: Pos2, radius: f32) {
    let size = texture.size_vec2();
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let shown = size * (radius * 2.2 / size.x.max(size.y));
    let min = center - Vec2::new(shown.x * (1.0 - focus.x), shown.y * focus.y);
    let rect = Rect::from_min_size(min, shown);
    let mirrored = Rect::from_min_max(Pos2::new(1.0, 0.0), Pos2::new(0.0, 1.0));
    painter.with_clip_rect(Rect::from_center_size(center, Vec2::splat(radius * 2.0 + 4.0))).image(texture.id(), rect, mirrored, Color32::WHITE);
}

fn fit_width(texture: &TextureHandle, width: f32) -> Vec2 {
    let size = texture.size_vec2();
    Vec2::new(width, width * size.y / size.x)
}

fn cover(painter: &egui::Painter, texture: &TextureHandle, rect: Rect, focus_y: f32) {
    let size = texture.size_vec2();
    let scale = (rect.width() / size.x).max(rect.height() / size.y);
    let shown = Vec2::new(rect.width() / (size.x * scale), rect.height() / (size.y * scale));
    let center = Pos2::new(0.5, focus_y.clamp(shown.y / 2.0, 1.0 - shown.y / 2.0));
    painter.with_clip_rect(rect).image(texture.id(), rect, Rect::from_center_size(center, shown), Color32::WHITE);
}

fn text(painter: &egui::Painter, pos: Pos2, anchor: Align2, value: impl ToString, font: FontId, color: Color32) -> Rect {
    painter.text(pos, anchor, value.to_string(), font, color)
}

fn outlined(painter: &egui::Painter, pos: Pos2, anchor: Align2, value: &str, font: FontId, color: Color32, outline: f32) -> Rect {
    let drop = (font.size * 0.075).max(outline);
    painter.text(pos + Vec2::new(drop, drop * 1.1), anchor, value, font.clone(), Color32::from_black_alpha(230));
    painter.text(pos, anchor, value, font, color)
}

fn vertical_fade(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

fn check_badge(painter: &egui::Painter, center: Pos2) {
    painter.circle_filled(center, 8.0, GREEN);
    painter.add(egui::Shape::line(vec![center + Vec2::new(-3.6, 0.2), center + Vec2::new(-1.0, 2.9), center + Vec2::new(3.9, -2.7)], Stroke::new(1.9, Color32::WHITE)));
}

fn glow(painter: &egui::Painter, rect: Rect, color: Color32, spread: f32) {
    for i in 0..6 {
        let grow = spread * (i as f32 + 1.0) / 6.0;
        let alpha = color.a() as f32 * (1.0 - i as f32 / 6.0) / 6.0;
        painter.rect_filled(rect.expand(grow), CornerRadius::same((6.0 + grow) as u8), Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha as u8));
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()), f(a.a(), b.a()))
}

fn hit(ui: &mut egui::Ui, rect: Rect, key: &str, label: &str, enabled: bool) -> (egui::Response, f32) {
    let id = Id::new(("launcher-widget", key));
    let response = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let hover = ui.ctx().animate_bool_with_time(id.with("hover"), enabled && response.hovered(), 0.12);
    (response, hover)
}

const FRAME_SIZE: Vec2 = Vec2::new(383.0, 112.0);
const FRAME_EDGE: f32 = 24.0;

fn nine(painter: &egui::Painter, texture: egui::TextureId, rect: Rect, tint: Color32) {
    let scale = (rect.height() / FRAME_SIZE.y).clamp(0.3, 0.6);
    let edge = (FRAME_EDGE * scale).min(rect.width() / 2.0).min(rect.height() / 2.0);
    let xs = [rect.left(), rect.left() + edge, rect.right() - edge, rect.right()];
    let ys = [rect.top(), rect.top() + edge, rect.bottom() - edge, rect.bottom()];
    let us = [0.0, FRAME_EDGE / FRAME_SIZE.x, 1.0 - FRAME_EDGE / FRAME_SIZE.x, 1.0];
    let vs = [0.0, FRAME_EDGE / FRAME_SIZE.y, 1.0 - FRAME_EDGE / FRAME_SIZE.y, 1.0];
    let mut mesh = Mesh::with_texture(texture);
    for y in 0..4 {
        for x in 0..4 {
            mesh.vertices.push(egui::epaint::Vertex { pos: Pos2::new(xs[x], ys[y]), uv: Pos2::new(us[x], vs[y]), color: tint });
        }
    }
    for y in 0..3u32 {
        for x in 0..3u32 {
            let a = y * 4 + x;
            mesh.add_triangle(a, a + 1, a + 5);
            mesh.add_triangle(a, a + 5, a + 4);
        }
    }
    painter.add(mesh);
}

fn label_text(painter: &egui::Painter, center: Pos2, value: &str, room: Vec2, dim: bool) {
    let width = painter.layout_no_wrap(value.to_string(), blocky(100.0), TEXT).size().x.max(1.0);
    let size = (room.x / width * 100.0).min(room.y / 0.63);
    let center = center + Vec2::new(0.0, size * 0.065);
    let font = blocky(size);
    let drop = (size * 0.07).max(2.0);
    painter.text(center + Vec2::new(drop, drop), Align2::CENTER_CENTER, value, font.clone(), Color32::from_black_alpha(235));
    painter.text(center, Align2::CENTER_CENTER, value, font, if dim { Color32::from_gray(190) } else { Color32::WHITE });
}

fn button(ui: &mut egui::Ui, rect: Rect, key: &str, label: &str, skin: Skin, enabled: bool) -> bool {
    let (response, hover) = hit(ui, rect, key, label, enabled);
    let painter = ui.painter();
    let face = rect.translate(Vec2::new(0.0, -hover * 1.5));
    nine(painter, if enabled { skin.texture() } else { Skin::Gray.texture() }, face, if enabled { Color32::WHITE } else { Color32::from_gray(150) });
    if hover > 0.0 {
        painter.rect_filled(face.shrink(face.height() * 0.16), CornerRadius::ZERO, Color32::from_white_alpha((24.0 * hover) as u8));
    }
    outlined(painter, face.center(), Align2::CENTER_CENTER, label, heavy(14.5), if enabled { Color32::WHITE } else { Color32::from_gray(190) }, 1.2);
    enabled && response.clicked()
}

const PAD: f32 = 28.0;
const GAP: f32 = 20.0;
const BUTTON_H: f32 = 38.0;
const BUTTON_GAP: f32 = 12.0;

fn button_width(painter: &egui::Painter, label: &str) -> f32 {
    painter.layout_no_wrap(label.to_string(), heavy(14.5), TEXT).size().x + 46.0
}

fn button_row(ui: &mut egui::Ui, start: Pos2, buttons: &[(&str, &str, bool, bool)]) -> Option<String> {
    let painter = ui.painter().clone();
    let mut x = start.x;
    let mut clicked = None;
    for (key, label, primary, enabled) in buttons {
        let width = button_width(&painter, label);
        let rect = Rect::from_min_size(Pos2::new(x, start.y), Vec2::new(width, BUTTON_H));
        let skin = if *primary { Skin::Green } else { Skin::Gray };
        if button(ui, rect, key, label, skin, *enabled) {
            clicked = Some(key.to_string());
        }
        x += width + BUTTON_GAP;
    }
    clicked
}

fn section_title(painter: &egui::Painter, pos: Pos2, title: &str, description: &str, wrap: f32) -> f32 {
    text(painter, pos, Align2::LEFT_TOP, title, semi(18.0), TEXT);
    let galley = painter.layout(description.to_string(), regular(13.5), MUTED, wrap);
    let height = galley.size().y;
    painter.galley(pos + Vec2::new(0.0, 32.0), galley, MUTED);
    32.0 + height
}

fn card(painter: &egui::Painter, rect: Rect, hover: f32) {
    painter.rect_filled(rect.translate(Vec2::new(0.0, 5.0)).shrink(3.0), CornerRadius::same(6), Color32::from_black_alpha(90));
    nine(painter, skins().tables[1], rect, mix(Color32::from_gray(225), Color32::WHITE, hover));
}

fn progress_bar(painter: &egui::Painter, rect: Rect, fraction: Option<f32>, time: f64) {
    painter.rect_filled(rect, CornerRadius::ZERO, TRACK);
    let fill = match fraction {
        Some(f) => Rect::from_min_size(rect.min, Vec2::new(rect.width() * f.clamp(0.0, 1.0), rect.height())),
        None => {
            let width = rect.width() * 0.3;
            let x = rect.min.x + ((time * 0.7).fract() as f32) * (rect.width() + width) - width;
            Rect::from_min_max(Pos2::new(x.max(rect.min.x), rect.min.y), Pos2::new((x + width).min(rect.max.x), rect.max.y))
        }
    };
    if fill.width() > 1.0 {
        painter.rect_filled(fill, CornerRadius::ZERO, GREEN);
        painter.rect_filled(Rect::from_min_size(fill.min, Vec2::new(fill.width(), (rect.height() * 0.25).max(1.0))), CornerRadius::ZERO, Color32::from_white_alpha(45));
    }
}

fn initial(name: &str) -> String {
    name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into())
}

fn ellipsize(painter: &egui::Painter, value: &str, font: FontId, room: f32) -> String {
    let mut chars: Vec<char> = value.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate = format!("{}…", chars.iter().collect::<String>().trim_end());
        if painter.layout_no_wrap(candidate.clone(), font.clone(), TEXT).size().x <= room {
            return candidate;
        }
    }
    "…".to_string()
}

fn note_line(ui: &mut egui::Ui, line: &str) {
    let body = Color32::from_rgb(228, 228, 236);
    let trimmed = line.trim_start();
    let label = |ui: &mut egui::Ui, value: &str, font: FontId, color: Color32| {
        ui.add(egui::Label::new(egui::RichText::new(value).font(font).color(color)).wrap());
    };
    if trimmed.is_empty() {
        ui.add_space(6.0);
    } else if let Some(rest) = trimmed.strip_prefix("### ") {
        ui.add_space(4.0);
        label(ui, rest, semi(15.5), TEXT);
    } else if let Some(rest) = trimmed.strip_prefix("## ") {
        ui.add_space(8.0);
        label(ui, rest, semi(17.5), TEXT);
    } else if let Some(rest) = trimmed.strip_prefix("# ") {
        ui.add_space(10.0);
        label(ui, rest, bold(20.0), TEXT);
    } else if let Some(rest) = trimmed.strip_prefix("-# ") {
        label(ui, rest, regular(12.5), FAINT);
    } else if let Some(rest) = trimmed.strip_prefix("- ") {
        let nested = line.starts_with("  ");
        ui.horizontal_top(|ui| {
            ui.add_space(if nested { 26.0 } else { 6.0 });
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(egui::RichText::new(if nested { "◦" } else { "•" }).font(regular(15.0)).color(ACCENT));
            ui.add(egui::Label::new(egui::RichText::new(rest).font(regular(15.0)).color(body)).wrap());
        });
    } else {
        label(ui, trimmed, regular(15.0), body);
    }
}

const SERVER_RECHECK_SECONDS: f64 = 120.0;
const AVATAR_RECHECK_SECONDS: f64 = 300.0;

fn server_label(state: &api::ServerState) -> String {
    match state {
        api::ServerState::Checking => "checking".into(),
        api::ServerState::Online { sign_in, .. } => format!("online (launcher sign-in {})", if *sign_in { "live" } else { "not live yet" }),
        api::ServerState::MaintenanceSoon { .. } => "maintenance scheduled".into(),
        api::ServerState::Maintenance { .. } => "under maintenance".into(),
        api::ServerState::Offline => "offline".into(),
    }
}

fn maintenance_soon(starts: Option<u64>) -> String {
    let Some(starts) = starts else { return "Maintenance starts soon".into() };
    let minutes = (starts.saturating_sub(api::now_unix()) + 59) / 60;
    match minutes {
        0 => "Maintenance is starting now".into(),
        1 => "Maintenance starts in 1 minute".into(),
        2..=59 => format!("Maintenance starts in {minutes} minutes"),
        _ => format!("Maintenance starts in {} h {} min", minutes / 60, minutes % 60),
    }
}

fn excerpt(notes: &str, lines: usize) -> String {
    notes.lines().map(str::trim).filter(|l| !l.is_empty()).take(lines).collect::<Vec<_>>().join("\n")
}

impl LauncherApp {
    fn window_controls(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let painter = ui.painter().clone();
        let drag = ui.interact(Rect::from_min_max(rect.min, Pos2::new(rect.right() - 92.0, rect.top() + 64.0)), Id::new("title-drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        let close = Rect::from_min_size(Pos2::new(rect.right() - 46.0, rect.top()), Vec2::new(46.0, 32.0));
        let minimize = close.translate(Vec2::new(-46.0, 0.0));
        let (min_response, min_hover) = hit(ui, minimize, "minimize", "Minimize", true);
        painter.rect_filled(minimize, CornerRadius::ZERO, mix(Color32::TRANSPARENT, Color32::from_white_alpha(22), min_hover));
        let glyph = mix(TITLE_GREY, TEXT, min_hover);
        painter.line_segment([minimize.center() - Vec2::new(5.0, 0.0), minimize.center() + Vec2::new(5.0, 0.0)], Stroke::new(1.4, glyph));
        if min_response.clicked() {
            ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
        }
        let (close_response, close_hover) = hit(ui, close, "close", "Close", true);
        painter.rect_filled(close, CornerRadius::ZERO, mix(Color32::TRANSPARENT, CLOSE_RED, close_hover));
        let glyph = mix(TITLE_GREY, TEXT, close_hover);
        let c = close.center();
        painter.line_segment([c - Vec2::splat(5.0), c + Vec2::splat(5.0)], Stroke::new(1.4, glyph));
        painter.line_segment([c + Vec2::new(-5.0, 5.0), c + Vec2::new(5.0, -5.0)], Stroke::new(1.4, glyph));
        if close_response.clicked() {
            ui.ctx().send_viewport_cmd(ViewportCommand::Close);
        }
    }

    fn side_item(&mut self, ui: &mut egui::Ui, rect: Rect, key: &str, label: &str, selected: bool) -> (bool, egui::Painter) {
        let (response, hover) = hit(ui, rect, key, label, true);
        let painter = ui.painter().clone();
        let on = ui.ctx().animate_bool_with_time(Id::new(("side-selected", key)), selected, 0.18);
        painter.rect_filled(rect, CornerRadius::same(6), mix(mix(Color32::TRANSPARENT, SIDE_HOVER, hover), SIDE_SELECTED, on));
        (response.clicked(), painter)
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let painter = ui.painter().clone();
        painter.rect_filled(rect, CornerRadius::ZERO, SIDE);

        let plate = Rect::from_min_max(rect.min + Vec2::new(12.0, 14.0), Pos2::new(rect.right() - 12.0, rect.top() + 84.0));
        painter.rect_filled(plate.translate(Vec2::new(0.0, 5.0)).shrink(3.0), CornerRadius::same(6), Color32::from_black_alpha(120));
        let signed = self.account_name.clone().or_else(|| self.discord_name.clone());
        nine(&painter, rank_table(if signed.is_some() { self.rank } else { None }), plate, Color32::WHITE);
        let face = Pos2::new(plate.left() + 36.0, plate.center().y);
        let names = Rect::from_min_max(Pos2::new(plate.left() + 64.0, plate.top() + 4.0), Pos2::new(plate.right() - 12.0, plate.bottom() - 4.0));
        let clip = painter.with_clip_rect(names);
        match &signed {
            Some(name) => {
                let own = self.account_name.as_ref();
                match self.avatar.as_ref().filter(|(owner, _, _)| Some(owner) == own) {
                    Some((_, picture, focus)) => character(&painter, picture, *focus, face, 24.0),
                    None => {
                        painter.circle_filled(face, 20.0, Color32::from_black_alpha(90));
                        outlined(&painter, face, Align2::CENTER_CENTER, &initial(name), heavy(20.0), Color32::WHITE, 1.5);
                    }
                }
                let label = rank_label(self.rank).filter(|_| self.account_name.is_some());
                let font = heavy(if name.chars().count() > 9 { 18.0 } else { 23.0 });
                let shown = if painter.layout_no_wrap(name.clone(), font.clone(), TEXT).size().x > names.width() { ellipsize(&painter, name, font.clone(), names.width()) } else { name.clone() };
                let name_y = if label.is_some() { names.center().y - 8.0 } else { names.center().y };
                outlined(&clip, Pos2::new(names.center().x, name_y), Align2::CENTER_CENTER, &shown, font, Color32::WHITE, 1.5);
                if let Some(label) = label {
                    outlined(&clip, Pos2::new(names.center().x, name_y + 20.0), Align2::CENTER_CENTER, label, heavy(12.0), Color32::from_gray(225), 1.0);
                }
            }
            None => {
                image(&painter, &self.art.avatar, Rect::from_center_size(face, Vec2::splat(40.0)));
                outlined(&clip, names.center(), Align2::CENTER_CENTER, "Not signed in", heavy(15.5), Color32::WHITE, 1.2);
            }
        }

        let game = Rect::from_min_max(Pos2::new(rect.left() + 12.0, rect.top() + 100.0), Pos2::new(rect.right() - 12.0, rect.top() + 164.0));
        let (clicked, item) = self.side_item(ui, game, "side-game", "Project Reboot", matches!(self.page, Page::Play | Page::Notes | Page::Videos));
        let game_icon = self.game_icon();
        image(&item, &game_icon, Rect::from_center_size(Pos2::new(game.left() + 32.0, game.center().y), Vec2::splat(48.0)));
        let title_y = if self.wine.is_some() { game.center().y - 8.0 } else { game.center().y };
        outlined(&item, Pos2::new(game.left() + 64.0, title_y), Align2::LEFT_CENTER, "Project Reboot", heavy(17.0), Color32::WHITE, 1.5);
        if self.wine.is_some() {
            text(&item, Pos2::new(game.left() + 65.0, title_y + 19.0), Align2::LEFT_CENTER, "Windows · Proton", semi(11.5), MUTED);
        }
        if clicked {
            self.go(Page::Play);
        }

        let logs = Rect::from_min_max(Pos2::new(rect.left() + 12.0, rect.bottom() - SAFE + 6.0 - 50.0), Pos2::new(rect.right() - 12.0, rect.bottom() - SAFE + 6.0));
        let settings = logs.translate(Vec2::new(0.0, -54.0));
        let news = settings.translate(Vec2::new(0.0, -54.0));
        let line = news.top() - 14.0;
        painter.line_segment([Pos2::new(rect.left() + 16.0, line), Pos2::new(rect.right() - 16.0, line)], Stroke::new(2.0, EDGE));
        text(&painter, Pos2::new(rect.left() + 18.0, line - 13.0), Align2::LEFT_CENTER, format!("Launcher v{CURRENT_VERSION}"), semi(11.0), Color32::from_gray(95));
        let entries = [
            (news, "side-news", "What\u{2019}s new", Page::Notes, self.art.whats_new.clone(), 34.0),
            (settings, "side-settings", "Settings", Page::Settings, self.art.settings.clone(), 34.0),
            (logs, "side-logs", "Logs", Page::Logs, self.art.console.clone(), 37.0),
        ];
        for (place, key, label, page, icon, size) in entries {
            let (clicked, item) = self.side_item(ui, place, key, label, self.page == page);
            let box_size = icon.size_vec2() * (size / icon.size_vec2().max_elem());
            image(&item, &icon, Rect::from_center_size(Pos2::new(place.left() + 30.0, place.center().y), box_size));
            outlined(&item, Pos2::new(place.left() + 60.0, place.center().y), Align2::LEFT_CENTER, label, heavy(19.0), Color32::WHITE, 1.5);
            if clicked {
                self.go(page);
            }
        }
    }

    fn header(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let painter = ui.painter().clone();
        painter.rect_filled(rect, CornerRadius::ZERO, HEADER_BG);
        self.window_controls(ui, rect);
        outlined(&painter, Pos2::new(rect.center().x, rect.top() + 26.0), Align2::CENTER_CENTER, "Project Reboot", heavy(21.0), TITLE_GREY, 1.5);
        let version = self.version_text();
        let version = if version.starts_with(|c: char| c.is_ascii_digit()) { format!("v{version}") } else { version };
        outlined(&painter, Pos2::new(rect.center().x, rect.top() + 49.0), Align2::CENTER_CENTER, &version, heavy(15.0), TITLE_GREY, 1.2);
        let font = heavy(20.0);
        let width = |label: &str| painter.layout_no_wrap(label.to_string(), font.clone(), TEXT).size().x;
        let mut placed: Vec<(Page, &str, f32, f32)> = Vec::new();
        let mut x = rect.left() + SAFE + 8.0;
        let mut left = vec![(Page::Play, "Play")];
        match self.page {
            Page::Settings => left.push((Page::Settings, "Settings")),
            Page::Logs => left.push((Page::Logs, "Logs")),
            _ => {}
        }
        for (page, label) in left {
            let w = width(label);
            placed.push((page, label, x, w));
            x += w + 28.0;
        }
        let mut x = rect.right() - SAFE - 8.0;
        for (page, label) in [(Page::Videos, "Videos"), (Page::Notes, "Patch Notes")] {
            let w = width(label);
            x -= w;
            placed.push((page, label, x, w));
            x -= 28.0;
        }
        let y = rect.top() + 84.0;
        let mut underline = None;
        for (page, label, x, w) in placed {
            let area = Rect::from_min_size(Pos2::new(x - 8.0, y - 16.0), Vec2::new(w + 16.0, 32.0));
            let (response, hover) = hit(ui, area, &format!("tab-{label}"), label, true);
            let active = self.page == page;
            let color = if active { Color32::WHITE } else { mix(TAB_GREY, Color32::from_gray(215), hover) };
            outlined(&painter, Pos2::new(x, y), Align2::LEFT_CENTER, label, font.clone(), color, 1.5);
            if active {
                underline = Some((x, w));
            }
            if response.clicked() {
                self.go(page);
            }
        }
        if let Some((target_x, target_w)) = underline {
            let ctx = ui.ctx();
            let ux = ctx.animate_value_with_time(Id::new("tab-underline-x"), target_x, 0.22);
            let uw = ctx.animate_value_with_time(Id::new("tab-underline-w"), target_w, 0.22);
            painter.rect_filled(Rect::from_min_size(Pos2::new(ux, rect.bottom() - 7.0), Vec2::new(uw, 3.0)), CornerRadius::ZERO, MAGENTA);
        }
    }

    fn play_page(&mut self, ui: &mut egui::Ui, origin: Pos2, width: f32) -> f32 {
        let painter = ui.painter().clone();
        let time = ui.input(|i| i.time);
        let hero = Rect::from_min_size(origin, Vec2::new(width, HERO));
        cover(&painter, self.cover.texture(), hero, 0.5);
        vertical_fade(&painter, Rect::from_min_max(Pos2::new(hero.left(), hero.bottom() - 70.0), hero.max), Color32::TRANSPARENT, Color32::from_black_alpha(110));
        let logo = fit_width(&self.art.logo, (width * 0.26).min(250.0));
        image(&painter, &self.art.logo, Rect::from_center_size(Pos2::new(hero.center().x, hero.top() + SAFE + logo.y / 2.0), logo));
        let strip = Rect::from_min_size(hero.left_bottom(), Vec2::new(width, STRIP));

        let ready = self.installed.is_some();
        let (label, enabled, status, fill, skin): (String, bool, String, Option<Option<f32>>, Skin) = match &self.phase {
            Phase::Updating { text, progress } => {
                let label = if text.starts_with("Moving") {
                    "MOVING"
                } else if text.starts_with("Checking") || progress.is_none() {
                    "CHECKING"
                } else if self.installed.is_none() {
                    "INSTALLING"
                } else {
                    "UPDATING"
                };
                let status = match progress {
                    Some(p) => format!("{text}  ·  {:.0}%", p * 100.0),
                    None => text.clone(),
                };
                (label.into(), false, status, Some(*progress), Skin::Gray)
            }
            Phase::Ready => {
                let status = match (&self.installed, &self.server) {
                    (None, _) => "Not installed yet",
                    (Some(_), api::ServerState::Offline) => "Server offline",
                    (Some(_), api::ServerState::Maintenance { .. }) => "Under maintenance",
                    (Some(_), _) => "Up to date",
                };
                ("PLAY".into(), ready, status.to_string(), None, Skin::Green)
            }
            Phase::SigningIn { text } => ("SIGNING IN".into(), false, text.clone(), None, Skin::Gray),
            Phase::Choose { .. } => ("SIGNING IN".into(), false, "Choose an account".into(), None, Skin::Gray),
            Phase::SignIn { .. } => ("SIGNING IN".into(), false, "Sign in to play".into(), None, Skin::Gray),
            Phase::Playing { account, .. } if account.is_empty() => ("PLAYING".into(), false, "Playing".into(), None, Skin::Blue),
            Phase::Playing { account, .. } => ("PLAYING".into(), false, format!("Playing as {account}"), None, Skin::Blue),
            Phase::Failed { .. } => ("PLAY".into(), false, "Something went wrong".into(), None, Skin::Red),
        };
        match fill {
            Some(fraction) => progress_bar(&painter, strip, fraction, time),
            None => {
                painter.rect_filled(strip, CornerRadius::ZERO, TRACK);
            }
        }

        let play = Rect::from_center_size(Pos2::new(hero.center().x, hero.bottom() + STRIP / 2.0), Vec2::new(236.0, 66.0));
        let (response, hover) = hit(ui, play, "play-button", &label, enabled);
        if enabled {
            let pulse = ((time * 2.2).sin() as f32 * 0.5 + 0.5) * 0.6 + 0.4;
            glow(&painter, play.shrink(8.0), Color32::from_rgba_unmultiplied(70, 230, 110, (110.0 * pulse) as u8), 16.0);
        }
        let face = play.translate(Vec2::new(0.0, -hover * 2.0));
        painter.rect_filled(face.translate(Vec2::new(0.0, 6.0)).shrink(4.0), CornerRadius::same(4), Color32::from_black_alpha(140));
        let busy = matches!(self.phase, Phase::Updating { .. } | Phase::SigningIn { .. } | Phase::Playing { .. });
        nine(&painter, skin.texture(), face, if enabled || busy || matches!(self.phase, Phase::Failed { .. }) { Color32::WHITE } else { Color32::from_gray(150) });
        label_text(&painter, face.center() + Vec2::new(0.0, -2.0), &label, Vec2::new(face.width() - 30.0, face.height() * 0.66), !(enabled || busy));
        if enabled && response.clicked() {
            log::info("Play pressed: making sure the game is up to date first");
            self.play_after_update = true;
            self.start_update();
        }

        let status_y = play.bottom() + 18.0;
        let is_ready = matches!(self.phase, Phase::Ready);
        let detail: Option<(String, Color32)> = match (&self.phase, &self.installed) {
            (Phase::Ready, None) => Some(("It downloads automatically before you play".into(), MUTED)),
            (Phase::Ready, Some(_)) => match &self.server {
                api::ServerState::Checking => Some(("Checking the server...".into(), FAINT)),
                api::ServerState::Online { .. } => None,
                api::ServerState::MaintenanceSoon { starts, .. } => Some((maintenance_soon(*starts), WARN)),
                api::ServerState::Maintenance { .. } => Some(("Only staff can join right now".into(), WARN)),
                api::ServerState::Offline => Some(("Try again in a minute".into(), MUTED)),
            },
            _ => None,
        };
        let primary_color = match (&self.phase, &self.server) {
            (Phase::Ready, _) if self.installed.is_none() => TEXT,
            (Phase::Ready, api::ServerState::Offline) => ERROR,
            (Phase::Ready, api::ServerState::Maintenance { .. }) => WARN,
            (Phase::Ready, _) => TEXT,
            _ => MUTED,
        };
        let primary_font = heavy(if is_ready { 17.0 } else { 15.0 });
        let checked = is_ready && self.installed.is_some() && !matches!(self.server, api::ServerState::Offline | api::ServerState::Maintenance { .. });
        let primary_width = painter.layout_no_wrap(status.clone(), primary_font.clone(), primary_color).size().x;
        let badge = if checked { 22.0 } else { 0.0 };
        let primary_left = play.center().x - (primary_width + badge) / 2.0;
        if checked {
            check_badge(&painter, Pos2::new(primary_left + 8.0, status_y));
        }
        outlined(&painter, Pos2::new(primary_left + badge, status_y), Align2::LEFT_CENTER, &status, primary_font, primary_color, 1.2);
        if let Some((line, color)) = &detail {
            text(&painter, Pos2::new(play.center().x, status_y + 22.0), Align2::CENTER_CENTER, line, semi(12.5), *color);
        }
        if let Phase::SigningIn { .. } = self.phase {
            let cancel_width = button_width(&painter, "Cancel");
            let cancel = Rect::from_min_size(Pos2::new(play.right() + 14.0, strip.bottom() + 8.0), Vec2::new(cancel_width, BUTTON_H));
            if button(ui, cancel, "cancel-sign-in", "Cancel", Skin::Gray, true) {
                self.cancel.store(true, Ordering::Relaxed);
                self.phase = Phase::Ready;
                self.sign_in_link = None;
            }
            if let Some(link) = self.sign_in_link.clone() {
                let label = "Copy sign-in link";
                let copy_width = button_width(&painter, label);
                let copy = Rect::from_min_size(Pos2::new(play.left() - 14.0 - copy_width, strip.bottom() + 8.0), Vec2::new(copy_width, BUTTON_H));
                if button(ui, copy, "copy-sign-in-link", label, Skin::Gray, true) {
                    ui.ctx().copy_text(link);
                    self.notice = Some("Sign-in link copied. Paste it into your browser if it did not open by itself.".into());
                }
            }
        }

        let (dot, server) = match &self.server {
            api::ServerState::Checking => (FAINT, "Checking the server"),
            api::ServerState::Online { .. } => (GREEN_HOVER, "Server online"),
            api::ServerState::MaintenanceSoon { .. } => (WARN, "Maintenance soon"),
            api::ServerState::Maintenance { .. } => (WARN, "Under maintenance"),
            api::ServerState::Offline => (ERROR, "Server offline"),
        };
        if !matches!(self.phase, Phase::SigningIn { .. }) {
            let at = Pos2::new(origin.x + SAFE + 10.0, strip.bottom() + 32.0);
            painter.circle_filled(at, 6.0, INK);
            painter.circle_filled(at, 4.5, dot);
            outlined(&painter, at + Vec2::new(14.0, 0.0), Align2::LEFT_CENTER, server, heavy(15.5), MUTED, 1.0);
        }

        let mut x = origin.x + width - SAFE - 4.0;
        let icon_y = strip.bottom() + 32.0;
        let mut links: Vec<(&str, &str, TextureHandle, f32, &str)> = Vec::new();
        if !config::DISCORD_URL.is_empty() {
            links.push(("link-discord", "Discord server", self.art.discord.clone(), 46.0, config::DISCORD_URL));
        }
        if !config::YOUTUBE_URL.is_empty() {
            links.push(("link-youtube", "YouTube channel", self.art.youtube_icon.clone(), 50.0, config::YOUTUBE_URL));
        }
        for (key, label, icon, size, url) in links {
            let shown = fit_width(&icon, size);
            let place = Rect::from_min_size(Pos2::new(x - shown.x, icon_y - shown.y / 2.0), shown);
            let (response, hover) = hit(ui, place.expand(4.0), key, label, true);
            image(&painter, &icon, Rect::from_center_size(place.center() - Vec2::new(0.0, hover * 2.0), shown * (1.0 + hover * 0.08)));
            if response.clicked() {
                let _ = open::that_detached(url);
            }
            x = place.left() - 18.0;
        }

        let row_h = 110.0;
        let row_top = (strip.bottom() + 84.0).max(origin.y + HEIGHT - HEADER - SAFE - row_h);
        let left = origin.x + SAFE;
        let inner = width - SAFE * 2.0;
        let newest_video = self.playlists.newest(1).into_iter().next();
        let patch_width = if newest_video.is_some() { (inner * 0.54).floor() } else { inner };
        let newest = self.posts().into_iter().next();
        let poster = newest.as_ref().and_then(|post| self.post_picture(post)).or_else(|| self.poster.clone()).unwrap_or_else(|| self.art.poster.clone());
        let (kicker, title, summary) = match &newest {
            Some(post) => {
                let date = api::short_date(&post.date);
                (
                    if date.is_empty() { "PATCH NOTES".to_string() } else { format!("PATCH NOTES  ·  {}", date.to_uppercase()) },
                    post.title.clone(),
                    excerpt(&post.body.lines().map(api::plain_line).collect::<Vec<_>>().join(" "), 2),
                )
            }
            None => ("PATCH NOTES".to_string(), format!("Project Reboot {}", self.version_text()), "No patch notes yet. Join the Discord server for the latest news.".to_string()),
        };
        if self.teaser_card(ui, Rect::from_min_size(Pos2::new(left, row_top), Vec2::new(patch_width, row_h)), "news-update", Some(&poster), &kicker, &title, &summary) {
            self.news_selected = newest.map(|post| post.id);
            self.go(Page::Notes);
        }
        if let Some(video) = newest_video {
            self.want_thumb(&video.id);
            let thumb = self.video_thumbs.get(&video.id).cloned();
            let date = api::short_date(&video.date);
            let kicker = if date.is_empty() { "LATEST VIDEO".to_string() } else { format!("LATEST VIDEO  ·  {}", date.to_uppercase()) };
            let place = Rect::from_min_max(Pos2::new(left + patch_width + 16.0, row_top), Pos2::new(left + inner, row_top + row_h));
            if self.teaser_card(ui, place, &format!("latest-video-{}", video.id), thumb.as_ref(), &kicker, &video.title, "") {
                if let Some(link) = api::watch_url(&video.id) {
                    log::info("Opening a video on YouTube");
                    let _ = open::that_detached(link);
                }
            }
        }
        row_top + row_h + SAFE - origin.y
    }

    #[allow(clippy::too_many_arguments)]
    fn teaser_card(&self, ui: &mut egui::Ui, rect: Rect, key: &str, picture: Option<&TextureHandle>, kicker: &str, title: &str, body: &str) -> bool {
        let (response, hover) = hit(ui, rect, key, title, true);
        let painter = ui.painter().clone();
        let frame = rect.translate(Vec2::new(0.0, -hover * 2.0));
        card(&painter, frame, hover);
        let pad = 12.0;
        let picture_h = frame.height() - pad * 2.0;
        let picture_rect = Rect::from_min_size(frame.min + Vec2::splat(pad), Vec2::new((picture_h * 16.0 / 9.0).min(frame.width() * 0.42), picture_h));
        match picture {
            Some(texture) => cover(&painter, texture, picture_rect, 0.5),
            None => {
                painter.rect_filled(picture_rect, CornerRadius::ZERO, Color32::from_rgb(20, 20, 20));
            }
        }
        painter.rect_stroke(picture_rect, CornerRadius::ZERO, Stroke::new(2.0, INK), StrokeKind::Outside);
        let text_left = picture_rect.right() + 14.0;
        let room = frame.right() - 16.0 - text_left;
        let clip = painter.with_clip_rect(Rect::from_min_max(Pos2::new(text_left, frame.top() + 8.0), Pos2::new(frame.right() - 14.0, frame.bottom() - 10.0)));
        outlined(&clip, Pos2::new(text_left, frame.top() + 22.0), Align2::LEFT_CENTER, kicker, heavy(11.0), ACCENT, 1.0);
        let title_font = heavy(15.5);
        let galley = painter.layout(title.to_string(), title_font, Color32::WHITE, room);
        let title_rows = galley.rows.len().min(if body.is_empty() { 3 } else { 2 });
        let title_h = galley.rows.iter().take(title_rows).map(|row| row.height()).sum::<f32>();
        let title_clip = clip.with_clip_rect(Rect::from_min_size(Pos2::new(text_left - 2.0, frame.top() + 32.0), Vec2::new(room + 4.0, title_h + 3.0)));
        title_clip.galley_with_override_text_color(Pos2::new(text_left + 1.5, frame.top() + 35.5), galley.clone(), INK);
        title_clip.galley(Pos2::new(text_left, frame.top() + 34.0), galley, Color32::WHITE);
        if !body.is_empty() {
            let body_galley = painter.layout(body.to_string(), regular(12.5), MUTED, room);
            let body_top = frame.top() + 38.0 + title_h;
            let line_h = painter.fonts(|f| f.row_height(&regular(12.5)));
            let lines = ((frame.bottom() - 10.0 - body_top) / line_h).floor().max(0.0);
            let body_clip = clip.with_clip_rect(Rect::from_min_max(Pos2::new(text_left, body_top), Pos2::new(frame.right() - 14.0, body_top + lines * line_h)));
            body_clip.galley(Pos2::new(text_left, body_top), body_galley, MUTED);
        }
        response.clicked()
    }

    fn notes_page(&mut self, ui: &mut egui::Ui, origin: Pos2, width: f32, height: f32) -> f32 {
        let painter = ui.painter().clone();
        let frame = Rect::from_min_size(origin + Vec2::new(PAD, PAD), Vec2::new(width - PAD * 2.0, (height - PAD * 2.0).max(420.0)));
        card(&painter, frame, 0.0);
        let posts = self.posts();
        if posts.is_empty() {
            text(&painter, frame.center() - Vec2::new(0.0, 12.0), Align2::CENTER_CENTER, "No patch notes yet", semi(20.0), TEXT);
            text(&painter, frame.center() + Vec2::new(0.0, 16.0), Align2::CENTER_CENTER, "Join the Discord server for the latest news.", regular(14.0), MUTED);
            return frame.bottom() + PAD - origin.y;
        }
        let selected = self.news_selected.as_ref().and_then(|id| posts.iter().position(|p| &p.id == id)).unwrap_or(0);
        let post = posts[selected].clone();
        self.want_image(&post);
        let list_width = if posts.len() > 1 { 270.0 } else { 0.0 };
        if posts.len() > 1 {
            let list = Rect::from_min_size(frame.min + Vec2::new(14.0, 14.0), Vec2::new(list_width, frame.height() - 28.0));
            painter.rect_filled(list, CornerRadius::same(4), Color32::from_black_alpha(80));
            text(&painter, Pos2::new(list.left() + 16.0, list.top() + 18.0), Align2::LEFT_CENTER, format!("ALL UPDATES  ·  {}", posts.len()), bold(11.5), ACCENT);
            let rows = Rect::from_min_max(Pos2::new(list.left() + 6.0, list.top() + 36.0), list.max - Vec2::new(6.0, 8.0));
            let mut pick = None;
            ui.scope_builder(UiBuilder::new().max_rect(rows), |ui| {
                egui::ScrollArea::vertical().id_salt("notes-list").auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    for (index, item) in posts.iter().enumerate() {
                        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 58.0), Sense::hover());
                        let (response, hover) = hit(ui, row, &format!("note-{}", item.id), &item.title, true);
                        let painter = ui.painter();
                        let active = index == selected;
                        if active || hover > 0.0 {
                            painter.rect_filled(row, CornerRadius::same(8), if active { SELECTED } else { mix(Color32::TRANSPARENT, CARD_HOVER, hover) });
                        }
                        if active {
                            painter.rect_filled(Rect::from_min_size(row.min + Vec2::new(0.0, 10.0), Vec2::new(3.0, row.height() - 20.0)), CornerRadius::same(2), GREEN);
                        }
                        let clip = painter.with_clip_rect(row.shrink2(Vec2::new(12.0, 2.0)).intersect(ui.clip_rect()));
                        let title = clip.layout_no_wrap(item.title.clone(), semi(14.5), if active { TEXT } else { mix(MUTED, TEXT, hover) });
                        let room = row.width() - 30.0;
                        let shown = if title.size().x > room { ellipsize(&clip, &item.title, semi(14.5), room) } else { item.title.clone() };
                        text(&clip, Pos2::new(row.left() + 16.0, row.top() + 20.0), Align2::LEFT_CENTER, shown, semi(14.5), if active { TEXT } else { mix(MUTED, TEXT, hover) });
                        let date = api::short_date(&item.date);
                        let sub = if index == 0 && !date.is_empty() { format!("{date}  ·  Latest") } else if index == 0 { "Latest".to_string() } else { date };
                        text(&clip, Pos2::new(row.left() + 16.0, row.top() + 40.0), Align2::LEFT_CENTER, sub, regular(12.0), if index == 0 { GREEN_HOVER } else { FAINT });
                        if response.clicked() {
                            pick = Some(item.id.clone());
                        }
                    }
                });
            });
            if let Some(id) = pick {
                if self.news_selected.as_deref() != Some(id.as_str()) {
                    self.news_selected = Some(id);
                    self.page_since = ui.input(|i| i.time);
                }
            }
        }
        let left = frame.left() + PAD + if list_width > 0.0 { list_width + 4.0 } else { 0.0 };
        let right = frame.right() - PAD;
        let column = right - left;
        let date = api::short_date(&post.date);
        text(&painter, Pos2::new(left, frame.top() + PAD), Align2::LEFT_TOP, if date.is_empty() { "PATCH NOTES".to_string() } else { format!("PATCH NOTES  ·  {}", date.to_uppercase()) }, bold(11.5), ACCENT);
        let heading = painter.layout(post.title.clone(), bold(26.0), TEXT, column);
        let heading_height = heading.size().y;
        painter.galley(Pos2::new(left, frame.top() + PAD + 20.0), heading, TEXT);
        let body = Rect::from_min_max(Pos2::new(left, frame.top() + PAD + 32.0 + heading_height), Pos2::new(right + 12.0, frame.bottom() - 16.0));
        let picture = self.news_images.get(&post.id).cloned().or_else(|| if post.id == "release" { Some(self.poster.clone().unwrap_or_else(|| self.art.poster.clone())) } else { None });
        ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
            egui::ScrollArea::vertical().id_salt(("notes-body", post.id.clone())).auto_shrink([false, false]).show(ui, |ui| {
                ui.set_width(column);
                ui.spacing_mut().item_spacing.y = 5.0;
                if let Some(picture) = &picture {
                    let size = picture.size_vec2();
                    let scale = (column / size.x).min(320.0 / size.y).min(1.0);
                    let shown = size * scale;
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(column, shown.y + 14.0), Sense::hover());
                    let place = Rect::from_min_size(rect.min, shown);
                    ui.painter().image(picture.id(), place, FULL_UV, Color32::WHITE);
                    ui.painter().rect_stroke(place, CornerRadius::same(6), Stroke::new(1.0, EDGE_STRONG), StrokeKind::Outside);
                }
                for line in post.body.lines() {
                    note_line(ui, line);
                }
                ui.add_space(12.0);
            });
        });
        frame.bottom() + PAD - origin.y
    }

    fn game_icon(&mut self) -> TextureHandle {
        let Some(exe) = self.installed.as_ref().map(|installed| installed.exe.clone()) else {
            return self.art.icon.clone();
        };
        let stamp = std::fs::metadata(&exe).and_then(|m| m.modified()).ok();
        if let Some((cached, at, texture)) = &self.game_icon {
            if *cached == exe && *at == stamp {
                return texture.clone();
            }
        }
        let texture = match gameicon::load(&exe) {
            Some(picture) => {
                let picture = image::imageops::resize(&picture, 96, 96, image::imageops::FilterType::Lanczos3);
                self.ctx.load_texture("game-icon", egui::ColorImage::from_rgba_unmultiplied([96, 96], picture.as_raw()), TextureOptions::LINEAR)
            }
            None => {
                log::info("The game has no icon the launcher can read, so it shows the built-in one");
                self.art.icon.clone()
            }
        };
        self.game_icon = Some((exe, stamp, texture.clone()));
        texture
    }

    fn read_log(&self, max_bytes: u64) -> String {
        if self.client_log {
            config::client_log(&self.cfg.install_dir).map(|(path, _)| log::tail_file(&path, max_bytes)).unwrap_or_default()
        } else {
            log::tail(max_bytes)
        }
    }

    fn logs_page(&mut self, ui: &mut egui::Ui, origin: Pos2, width: f32, height: f32) -> f32 {
        let painter = ui.painter().clone();
        let frame = Rect::from_min_size(origin + Vec2::new(PAD, PAD), Vec2::new(width - PAD * 2.0, (height - PAD * 2.0).max(440.0)));
        card(&painter, frame, 0.0);
        let inner_left = frame.left() + PAD;
        let tabs_y = frame.top() + PAD + 10.0;
        let tab_font = heavy(18.0);
        let mut x = inner_left;
        for (client, label) in [(false, "Launcher logs"), (true, "Client logs")] {
            let w = painter.layout_no_wrap(label.to_string(), tab_font.clone(), TEXT).size().x;
            let area = Rect::from_min_size(Pos2::new(x - 8.0, tabs_y - 16.0), Vec2::new(w + 16.0, 34.0));
            let (response, hover) = hit(ui, area, &format!("log-tab-{client}"), label, true);
            let active = self.client_log == client;
            let color = if active { Color32::WHITE } else { mix(TAB_GREY, Color32::from_gray(215), hover) };
            outlined(&painter, Pos2::new(x, tabs_y), Align2::LEFT_CENTER, label, tab_font.clone(), color, 1.2);
            if active {
                painter.rect_filled(Rect::from_min_size(Pos2::new(x, tabs_y + 14.0), Vec2::new(w, 3.0)), CornerRadius::ZERO, MAGENTA);
            }
            if response.clicked() && !active {
                self.client_log = client;
                self.log_text = self.read_log(96 * 1024);
            }
            x += w + 30.0;
        }
        let client = if self.client_log { config::client_log(&self.cfg.install_dir) } else { None };
        let (file, folder) = if self.client_log {
            let folder = config::client_log_dir(&self.cfg.install_dir);
            (client.as_ref().map(|(path, _)| path.clone()).unwrap_or_else(|| folder.join("Player.log")), folder)
        } else {
            (log::path(), log::dir())
        };
        let now = ui.input(|i| i.time);
        if now - self.log_checked >= 1.0 {
            self.log_checked = now;
            let seen = std::fs::metadata(&file).ok().map(|m| (file.clone(), m.len(), m.modified().ok()));
            if seen != self.log_seen {
                self.log_seen = seen;
                self.log_text = self.read_log(96 * 1024);
            }
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        let description = if !self.client_log {
            "If something goes wrong, copy this and share it with Project Reboot staff. It never contains passwords, login tokens or Discord details."
        } else if matches!(client, Some((_, true))) {
            "The game's own log from your last session. If the game crashed or acted up, copy this and share it with Project Reboot staff."
        } else {
            "The game's own log. If the game crashes or acts up, copy this and share it with Project Reboot staff."
        };
        let galley = painter.layout(description.to_string(), regular(13.5), MUTED, frame.width() - PAD * 2.0);
        let used = 32.0 + galley.size().y;
        painter.galley(Pos2::new(inner_left, frame.top() + PAD + 32.0), galley, MUTED);
        let has_log = file.is_file();
        let row = Pos2::new(inner_left, frame.top() + PAD + used + 18.0);
        match button_row(ui, row, &[("log-copy", "Copy log", true, has_log), ("log-refresh", "Refresh", false, true), ("log-open", "Open log file", false, has_log), ("log-folder", "Open folder", false, folder.is_dir())]).as_deref() {
            Some("log-copy") => {
                ui.ctx().copy_text(self.read_log(256 * 1024));
                self.notice = Some(if self.client_log { "The client log is copied to your clipboard." } else { "The launcher log is copied to your clipboard." }.into());
            }
            Some("log-refresh") => self.log_text = self.read_log(96 * 1024),
            Some("log-open") => {
                let _ = open::that_detached(&file);
            }
            Some("log-folder") => {
                let _ = open::that_detached(&folder);
            }
            _ => {}
        }
        let body = Rect::from_min_max(Pos2::new(inner_left, row.y + BUTTON_H + GAP), frame.max - Vec2::splat(PAD));
        painter.rect_filled(body, CornerRadius::same(4), Color32::from_rgb(18, 18, 18));
        painter.rect_stroke(body, CornerRadius::same(8), Stroke::new(1.0, EDGE), StrokeKind::Inside);
        let empty = if self.client_log { "The game hasn't written a log yet. It shows up here as soon as the game starts." } else { "The log is empty." };
        let shown = if self.log_text.trim().is_empty() { empty.to_string() } else { self.log_text.clone() };
        let client_view = self.client_log;
        ui.scope_builder(UiBuilder::new().max_rect(body.shrink(16.0)), |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            egui::ScrollArea::vertical().id_salt(("log-scroll", client_view)).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                for line in shown.lines() {
                    let color = if client_view {
                        if line.contains("Exception") || line.contains("Error") || line.contains("error") { ERROR } else if line.contains("Warning") { WARN } else { Color32::from_rgb(196, 214, 238) }
                    } else if line.contains("] ERROR") {
                        ERROR
                    } else if line.contains("] WARN") {
                        WARN
                    } else {
                        Color32::from_rgb(196, 214, 238)
                    };
                    ui.add(egui::Label::new(egui::RichText::new(line).font(FontId::monospace(12.5)).color(color)).wrap());
                }
            });
        });
        frame.bottom() + PAD - origin.y
    }

    fn settings_page(&mut self, ui: &mut egui::Ui, origin: Pos2, width: f32) -> f32 {
        let painter = ui.painter().clone();
        let left = origin.x + PAD;
        let card_width = (width - PAD * 2.0).min(840.0);
        let wrap = card_width - PAD * 2.0;
        let busy = self.busy();
        let mut y = origin.y + PAD;
        let card_height = |description_height: f32| PAD + description_height + 18.0 + BUTTON_H + PAD;

        let path = self.cfg.install_dir.display().to_string();
        let files_text = format!("The game is installed in {path}. Pick another folder to move it there, or choose a folder that already has the game to use it without downloading.");
        let measured = painter.layout(files_text.clone(), regular(13.5), MUTED, wrap).size().y + 32.0;
        let files = Rect::from_min_size(Pos2::new(left, y), Vec2::new(card_width, card_height(measured)));
        card(&painter, files, 0.0);
        let used = section_title(&painter, files.min + Vec2::splat(PAD), "Game files", &files_text, wrap);
        match button_row(ui, Pos2::new(files.left() + PAD, files.top() + PAD + used + 18.0), &[("verify-files", "Verify game files", true, !busy), ("change-folder", "Change folder", false, !busy), ("open-folder", "Open folder", false, self.cfg.install_dir.is_dir())]).as_deref() {
            Some("verify-files") => self.verify_files(),
            Some("change-folder") => self.change_folder(),
            Some("open-folder") => {
                let _ = open::that_detached(&self.cfg.install_dir);
            }
            _ => {}
        }
        y = files.bottom() + GAP;

        let login_text = "This PC stays signed in for 30 days, so Play logs you straight in. The game itself is signed in for each session and signed out again when you close it.";
        let measured = painter.layout(login_text.to_string(), regular(13.5), MUTED, wrap).size().y + 32.0;
        let login = Rect::from_min_size(Pos2::new(left, y), Vec2::new(card_width, card_height(measured)));
        card(&painter, login, 0.0);
        let used = section_title(&painter, login.min + Vec2::splat(PAD), "Game login", login_text, wrap);
        if button_row(ui, Pos2::new(login.left() + PAD, login.top() + PAD + used + 18.0), &[("sign-out", "Sign out on this PC", false, !matches!(self.phase, Phase::Playing { .. }))]).is_some() {
            log::info("Game login cleared from this PC from Settings");
            if let Some(saved) = remember::load() {
                let (cfg, agent) = (self.cfg.clone(), self.agent.clone());
                thread::spawn(move || api::forget(&agent, &cfg, &saved.remember));
            }
            remember::clear();
            self.notice = Some(match prefs::clear_login(&self.cfg.prefs_location) {
                Ok(()) => "The game is signed out on this PC.".into(),
                Err(e) => e,
            });
            api::save_account(None);
            self.discord_name = None;
            self.account_name = None;
            self.rank = None;
            self.avatar = None;
        }
        y = login.bottom() + GAP;

        let team = fit_width(&self.art.team, 96.0);
        let about_wrap = wrap - team.x - PAD;
        let platform = match &self.wine {
            Some(version) => format!(" Running on Linux through Wine {version}."),
            None => String::new(),
        };
        let about_text = format!("Project Reboot Launcher {CURRENT_VERSION}. Open source, and every download is checked against a signed file list before it is used.{platform}");
        let measured = painter.layout(about_text.clone(), regular(13.5), MUTED, about_wrap).size().y + 32.0;
        let has_source = !config::SOURCE_URL.is_empty();
        let about_height = if has_source { card_height(measured) } else { PAD + measured + PAD };
        let about = Rect::from_min_size(Pos2::new(left, y), Vec2::new(card_width, about_height.max(team.y + PAD * 2.0)));
        card(&painter, about, 0.0);
        let used = section_title(&painter, about.min + Vec2::splat(PAD), "About", &about_text, about_wrap);
        image(&painter, &self.art.team, Rect::from_min_size(Pos2::new(about.right() - PAD - team.x, about.center().y - team.y / 2.0), team));
        if has_source && button_row(ui, Pos2::new(about.left() + PAD, about.top() + PAD + used + 18.0), &[("source", "View source code", false, true)]).is_some() {
            let _ = open::that_detached(config::SOURCE_URL);
        }
        about.bottom() + PAD - origin.y
    }

    fn content(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let now = ui.input(|i| i.time);
        let t = ease(((now - self.page_since) / 0.24) as f32);
        let offset = (1.0 - t) * 18.0;
        let page = self.page;
        ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            ui.multiply_opacity(t);
            egui::ScrollArea::vertical().id_salt(("page-scroll", page)).auto_shrink([false, false]).show(ui, |ui| {
                let origin = ui.max_rect().min + Vec2::new(0.0, offset);
                let width = rect.width();
                let used = match page {
                    Page::Play => self.play_page(ui, origin, width),
                    Page::Notes => self.notes_page(ui, origin, width, rect.height()),
                    Page::Videos => self.videos_page(ui, origin, width),
                    Page::Logs => self.logs_page(ui, origin, width, rect.height()),
                    Page::Settings => self.settings_page(ui, origin, width),
                };
                ui.allocate_space(Vec2::new(width, used.max(rect.height()) - offset));
            });
        });
        if t < 1.0 {
            ui.ctx().request_repaint();
        }
    }

    fn notice(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let open = self.notice.is_some();
        let shown = ui.ctx().animate_bool_with_time(Id::new("notice"), open, 0.2);
        if open && shown < 1.0 {
            ui.ctx().request_repaint();
        }
        let Some(notice) = self.notice.clone() else { return };
        let painter = ui.painter().clone();
        let galley = painter.layout(notice, regular(13.5), WARN, (rect.width() - 160.0).min(620.0));
        let size = galley.size() + Vec2::new(76.0, 24.0);
        let toast = Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 18.0 + size.y / 2.0 - (1.0 - shown) * 20.0), size);
        painter.rect_filled(toast, CornerRadius::same(8), Color32::from_rgb(48, 40, 18));
        painter.rect_stroke(toast, CornerRadius::same(8), Stroke::new(1.0, Color32::from_rgb(120, 96, 36)), StrokeKind::Inside);
        painter.galley(toast.min + Vec2::new(20.0, 12.0), galley, WARN);
        let close = Rect::from_center_size(Pos2::new(toast.right() - 24.0, toast.center().y), Vec2::splat(24.0));
        let (response, hover) = hit(ui, close, "notice-close", "Dismiss", true);
        text(&painter, close.center(), Align2::CENTER_CENTER, "✕", semi(13.0), mix(WARN, TEXT, hover));
        if response.clicked() {
            self.notice = None;
        }
    }

    fn setup_screen(&mut self, ui: &mut egui::Ui, screen: Rect) {
        let shown = ui.ctx().animate_bool_with_time(Id::new("setup"), self.setup, 0.25);
        if !self.setup {
            return;
        }
        if shown < 1.0 {
            ui.ctx().request_repaint();
        }
        let painter = ui.painter().clone();
        let _ = ui.interact(screen, Id::new("setup-blocker"), Sense::click());
        painter.rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha((170.0 * shown) as u8));
        let width = 560.0;
        let wrap = width - PAD * 2.0;
        let intro = painter.layout(
            "Choose where to keep the game. Nothing is downloaded until you pick, and you can change it later in Settings.".to_string(),
            regular(14.5),
            MUTED,
            wrap,
        );
        let intro_height = intro.size().y;
        let option_height = 74.0;
        let unpublished = self.setup_size.is_none() && self.setup_unavailable.as_deref().map(update::is_not_published).unwrap_or(false);
        let (footer, footer_color) = match (self.setup_size, &self.setup_unavailable) {
            (Some(size), _) => (format!("Download size: about {:.0} MB", size as f64 / 1_048_576.0), FAINT),
            (None, Some(_)) if unpublished && !cfg!(windows) => (format!("The Linux version of Project Reboot isn't out yet. {}", config::PROTON_HELP), WARN),
            (None, Some(_)) if unpublished => (format!("No game build is published for {} yet.", config::PLATFORM), WARN),
            (None, Some(reason)) => (format!("Couldn't check the download size ({reason}). You can still pick a folder."), FAINT),
            (None, None) => ("Checking the download size...".to_string(), FAINT),
        };
        let footer = painter.layout(footer, regular(13.0), footer_color, wrap);
        let footer_height = footer.size().y;
        let error = self.setup_error.as_ref().map(|e| painter.layout(e.clone(), regular(13.0), ERROR, wrap));
        let error_height = error.as_ref().map(|g| g.size().y + 10.0).unwrap_or(0.0);
        let height = PAD + 34.0 + 12.0 + intro_height + GAP + option_height * 3.0 + 12.0 * 2.0 + GAP + footer_height + error_height + PAD;
        let panel = Rect::from_center_size(screen.center() + Vec2::new(0.0, (1.0 - shown) * 14.0), Vec2::new(width, height));
        card(&painter, panel, 0.0);
        let icon = Rect::from_min_size(panel.min + Vec2::new(PAD, PAD - 2.0), Vec2::splat(34.0));
        let game_icon = self.game_icon();
        image(&painter, &game_icon, icon);
        text(&painter, Pos2::new(icon.right() + 12.0, icon.center().y), Align2::LEFT_CENTER, "Welcome to Project Reboot!", semi(22.0), TEXT);
        painter.galley(Pos2::new(panel.left() + PAD, panel.top() + PAD + 46.0), intro, MUTED);
        let mut y = panel.top() + PAD + 46.0 + intro_height + GAP;
        let default_dir = config::default_install_dir();
        let default_label = if config::desktop_dir().is_some() && cfg!(windows) { "Install on my Desktop" } else { "Install in the recommended folder" };
        let options: [(&str, String, String, bool); 3] = [
            ("setup-default", default_label.to_string(), default_dir.display().to_string(), true),
            ("setup-choose", "Choose a different folder".to_string(), "Pick any folder. The game goes into a Project Reboot folder inside it.".to_string(), false),
            ("setup-existing", "I already have Project Reboot".to_string(), "Pick the folder that has the game in it, so nothing needs downloading again.".to_string(), false),
        ];
        let mut chosen = None;
        for (key, title, detail, recommended) in options {
            let row = Rect::from_min_size(Pos2::new(panel.left() + PAD, y), Vec2::new(wrap, option_height));
            let enabled = !(unpublished && key != "setup-existing");
            let (response, hover) = hit(ui, row, key, &title, enabled);
            let painter = if enabled { painter.clone() } else { let mut faded = painter.clone(); faded.set_opacity(0.4); faded };
            let base = if recommended { Color32::from_rgb(22, 74, 40) } else { ROW };
            let lit = if recommended { Color32::from_rgb(30, 98, 52) } else { ROW_HOVER };
            painter.rect_filled(row, CornerRadius::same(10), mix(base, lit, hover));
            painter.rect_stroke(row, CornerRadius::same(10), Stroke::new(1.5, mix(if recommended { GREEN } else { EDGE }, if recommended { GREEN_HOVER } else { ACCENT }, hover)), StrokeKind::Inside);
            let clip = painter.with_clip_rect(row.shrink2(Vec2::new(18.0, 4.0)));
            text(&clip, Pos2::new(row.left() + 20.0, row.top() + 24.0), Align2::LEFT_CENTER, &title, semi(16.0), TEXT);
            text(&clip, Pos2::new(row.left() + 20.0, row.top() + 49.0), Align2::LEFT_CENTER, &detail, regular(12.5), MUTED);
            if recommended {
                let chip_text = "Recommended";
                let chip_width = painter.layout_no_wrap(chip_text.to_string(), bold(11.0), TEXT).size().x + 18.0;
                let chip = Rect::from_min_size(Pos2::new(row.right() - 16.0 - chip_width, row.top() + 13.0), Vec2::new(chip_width, 22.0));
                painter.rect_filled(chip, CornerRadius::same(11), GREEN);
                text(&painter, chip.center(), Align2::CENTER_CENTER, chip_text, bold(11.0), Color32::WHITE);
            }
            text(&painter, Pos2::new(row.right() - 22.0, row.top() + 49.0), Align2::RIGHT_CENTER, "›", semi(20.0), mix(FAINT, TEXT, hover));
            if response.clicked() {
                chosen = Some(key);
            }
            y += option_height + 12.0;
        }
        painter.galley(Pos2::new(panel.left() + PAD, y), footer, footer_color);
        if let Some(error) = error {
            painter.galley(Pos2::new(panel.left() + PAD, y + footer_height + 10.0), error, ERROR);
        }
        match chosen {
            Some("setup-default") => self.finish_setup(default_dir),
            Some("setup-choose") => {
                if let Some(folder) = self.pick_folder("Choose where to keep Project Reboot") {
                    let target = config::install_target(&folder, &self.game_exe_name());
                    self.finish_setup(target);
                }
            }
            Some("setup-existing") => {
                if let Some(folder) = self.pick_folder("Choose the folder that has Project Reboot in it") {
                    let exe = self.game_exe_name();
                    match config::existing_install(&folder, &exe) {
                        config::Existing::Found(found) => self.finish_setup(found),
                        config::Existing::WindowsBuild => {
                            log::info("The chosen folder has the Windows build, not the Linux one");
                            self.setup_error = Some(format!("That folder has the Windows version of Project Reboot, and this launcher runs the Linux version. {}", config::PROTON_HELP));
                        }
                        config::Existing::Missing => self.setup_error = Some(format!("That folder doesn't have Project Reboot in it. Pick the folder that contains {exe}, or choose another option.")),
                    }
                }
            }
            _ => {}
        }
    }

    fn modal(&mut self, ui: &mut egui::Ui, screen: Rect) {
        let open = matches!(self.phase, Phase::Failed { .. } | Phase::Choose { .. } | Phase::SignIn { .. });
        let shown = ui.ctx().animate_bool_with_time(Id::new("modal"), open, 0.18);
        if open && shown < 1.0 {
            ui.ctx().request_repaint();
        }
        if !open {
            return;
        }
        if matches!(self.phase, Phase::SignIn { .. }) {
            self.sign_in_modal(ui, screen, shown);
            return;
        }
        let (title, message, accounts, retry) = match &self.phase {
            Phase::Failed { message, retry } => ("Something went wrong", message.clone(), Vec::new(), Some(*retry)),
            Phase::Choose { accounts, .. } => ("Choose an account", "Your Discord account owns more than one Project Reboot account. Pick the one to play.".to_string(), accounts.clone(), None),
            _ => return,
        };
        let painter = ui.painter().clone();
        let _ = ui.interact(screen, Id::new("modal-blocker"), Sense::click());
        painter.rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha((160.0 * shown) as u8));
        let width = 480.0;
        let galley = painter.layout(message, regular(14.5), MUTED, width - PAD * 2.0);
        let message_height = galley.size().y;
        let list = if accounts.is_empty() { 0.0 } else { accounts.len().min(6) as f32 * 54.0 + 4.0 };
        let height = PAD + 30.0 + 12.0 + message_height + GAP + list + BUTTON_H + PAD;
        let scale = 0.94 + 0.06 * shown;
        let panel = Rect::from_center_size(screen.center() + Vec2::new(0.0, (1.0 - shown) * 12.0), Vec2::new(width, height) * scale);
        card(&painter, panel, 0.0);
        text(&painter, panel.min + Vec2::splat(PAD), Align2::LEFT_TOP, title, semi(21.0), if retry.is_some() { ERROR } else { TEXT });
        painter.galley(Pos2::new(panel.left() + PAD, panel.top() + PAD + 42.0), galley, MUTED);
        let mut y = panel.top() + PAD + 42.0 + message_height + GAP;
        let mut action: Option<Box<dyn FnOnce(&mut LauncherApp)>> = None;
        if let Phase::Choose { session, .. } = &self.phase {
            for account in accounts.iter().take(6) {
                let row = Rect::from_min_size(Pos2::new(panel.left() + PAD, y), Vec2::new(panel.width() - PAD * 2.0, 46.0));
                let (response, hover) = hit(ui, row, &format!("account-{account}"), account, true);
                painter.rect_filled(row, CornerRadius::same(4), mix(ROW, ROW_HOVER, hover));
                painter.rect_stroke(row, CornerRadius::same(8), Stroke::new(1.0, mix(EDGE, ACCENT, hover)), StrokeKind::Inside);
                image(&painter, &self.art.avatar, Rect::from_center_size(Pos2::new(row.left() + 26.0, row.center().y), Vec2::splat(30.0)));
                text(&painter, Pos2::new(row.left() + 52.0, row.center().y), Align2::LEFT_CENTER, account, semi(15.0), TEXT);
                text(&painter, Pos2::new(row.right() - 16.0, row.center().y), Align2::RIGHT_CENTER, "›", semi(18.0), mix(FAINT, TEXT, hover));
                if response.clicked() {
                    let session = session.clone();
                    let account = account.clone();
                    action = Some(Box::new(move |app| app.request_ticket(session, account)));
                }
                y += 54.0;
            }
        }
        let bottom = panel.bottom() - PAD - BUTTON_H;
        match retry {
            Some(retry) => {
                let retry_width = button_width(&painter, "Try again");
                let primary = Rect::from_min_size(Pos2::new(panel.right() - PAD - retry_width, bottom), Vec2::new(retry_width, BUTTON_H));
                if button(ui, primary, "modal-retry", "Try again", Skin::Green, true) {
                    action = Some(Box::new(move |app| match retry {
                        Retry::Update => app.start_update(),
                        Retry::Ready | Retry::SignIn => app.start_play(),
                    }));
                }
                let fallback = retry == Retry::SignIn && self.installed.is_some();
                let second_label = if fallback { "Play without Discord" } else { "Close" };
                let second_width = button_width(&painter, second_label);
                let secondary = Rect::from_min_size(Pos2::new(primary.left() - BUTTON_GAP - second_width, bottom), Vec2::new(second_width, BUTTON_H));
                if button(ui, secondary, if fallback { "modal-plain" } else { "modal-close" }, second_label, Skin::Gray, true) {
                    action = Some(if fallback {
                        Box::new(|app: &mut LauncherApp| {
                            log::info("Playing without Discord after a sign-in problem");
                            app.start_plain_game();
                        })
                    } else {
                        Box::new(|app: &mut LauncherApp| app.phase = Phase::Ready)
                    });
                }
                let mut link_x = panel.left() + PAD;
                let mut links: Vec<(&str, &str)> = vec![("modal-log", "View log")];
                if fallback {
                    links.push(("modal-close", "Close"));
                }
                for (key, label) in links {
                    let width = painter.layout_no_wrap(label.to_string(), semi(14.0), TEXT).size().x;
                    let link = Rect::from_min_size(Pos2::new(link_x, bottom), Vec2::new(width + 4.0, BUTTON_H));
                    let (response, hover) = hit(ui, link, key, label, true);
                    text(&painter, link.left_center(), Align2::LEFT_CENTER, label, semi(14.0), mix(ACCENT, Color32::WHITE, hover));
                    if response.clicked() {
                        action = Some(if key == "modal-log" {
                            Box::new(|app: &mut LauncherApp| {
                                app.phase = Phase::Ready;
                                app.go(Page::Logs);
                            })
                        } else {
                            Box::new(|app: &mut LauncherApp| app.phase = Phase::Ready)
                        });
                    }
                    link_x = link.right() + 18.0;
                }
            }
            None => {
                let cancel_width = button_width(&painter, "Cancel");
                let cancel = Rect::from_min_size(Pos2::new(panel.right() - PAD - cancel_width, bottom), Vec2::new(cancel_width, BUTTON_H));
                if button(ui, cancel, "modal-cancel", "Cancel", Skin::Gray, true) {
                    action = Some(Box::new(|app| app.phase = Phase::Ready));
                }
            }
        }
        if let Some(action) = action {
            action(self);
        }
    }

    fn sign_in_modal(&mut self, ui: &mut egui::Ui, screen: Rect, shown: f32) {
        let Phase::SignIn { name, password, error, discord } = &self.phase else { return };
        let (mut name, mut password, error, discord) = (name.clone(), password.clone(), error.clone(), *discord);
        let painter = ui.painter().clone();
        let _ = ui.interact(screen, Id::new("modal-blocker"), Sense::click());
        painter.rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha((160.0 * shown) as u8));
        let width = 440.0;
        let inner = width - PAD * 2.0;
        let message = painter.layout("Use your Project Reboot username and password.".to_string(), regular(14.5), MUTED, inner);
        let error_galley = error.as_ref().map(|e| painter.layout(e.clone(), regular(14.0), ERROR, inner));
        let field_h = 40.0;
        let label_h = 22.0;
        let error_h = error_galley.as_ref().map_or(0.0, |g| g.size().y + 12.0);
        let discord_h = if discord { BUTTON_H + GAP } else { 0.0 };
        let height = PAD + 42.0 + message.size().y + GAP + error_h + (label_h + field_h) * 2.0 + 14.0 + GAP + discord_h + BUTTON_H + PAD;
        let panel = Rect::from_center_size(screen.center() + Vec2::new(0.0, (1.0 - shown) * 12.0), Vec2::new(width, height) * (0.94 + 0.06 * shown));
        card(&painter, panel, 0.0);
        text(&painter, panel.min + Vec2::splat(PAD), Align2::LEFT_TOP, "Sign in", semi(21.0), TEXT);
        let left = panel.left() + PAD;
        let mut y = panel.top() + PAD + 42.0;
        let message_h = message.size().y;
        painter.galley(Pos2::new(left, y), message, MUTED);
        y += message_h + GAP;
        if let Some(galley) = error_galley {
            let h = galley.size().y;
            painter.galley(Pos2::new(left, y), galley, ERROR);
            y += h + 12.0;
        }
        let field = |ui: &mut egui::Ui, y: f32, label: &str, value: &mut String, secret: bool| {
            text(&painter, Pos2::new(left, y), Align2::LEFT_TOP, label, semi(13.5), MUTED);
            let rect = Rect::from_min_size(Pos2::new(left, y + label_h), Vec2::new(inner, field_h));
            painter.rect_filled(rect, CornerRadius::same(4), FIELD);
            let edit = egui::TextEdit::singleline(value)
                .password(secret)
                .font(regular(15.0))
                .text_color(TEXT)
                .frame(false)
                .margin(egui::Margin::symmetric(12, 11))
                .desired_width(inner);
            let response = ui.put(rect, edit);
            let stroke = if response.has_focus() { ACCENT } else { EDGE_STRONG };
            painter.rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, stroke), StrokeKind::Inside);
            response
        };
        let name_response = field(ui, y, "Username", &mut name, false);
        y += label_h + field_h + 14.0;
        let password_response = field(ui, y, "Password", &mut password, true);
        y += label_h + field_h + GAP;
        if ui.memory(|m| m.focused().is_none()) {
            if name.is_empty() { name_response.request_focus() } else { password_response.request_focus() }
        }
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter)) && (name_response.lost_focus() || password_response.lost_focus());
        let mut action: Option<Box<dyn FnOnce(&mut LauncherApp)>> = None;
        if discord {
            let row = Rect::from_min_size(Pos2::new(left, y), Vec2::new(inner, BUTTON_H));
            if button(ui, row, "sign-in-discord", "Sign in with Discord", Skin::Blue, true) {
                action = Some(Box::new(|app: &mut LauncherApp| app.start_sign_in()));
            }
        }
        let bottom = panel.bottom() - PAD - BUTTON_H;
        let ready = !name.trim().is_empty() && !password.is_empty();
        let sign_in_width = button_width(&painter, "Sign in");
        let primary = Rect::from_min_size(Pos2::new(panel.right() - PAD - sign_in_width, bottom), Vec2::new(sign_in_width, BUTTON_H));
        if (button(ui, primary, "sign-in-password", "Sign in", Skin::Green, ready) || (enter && ready)) && action.is_none() {
            let (n, p) = (name.trim().to_string(), password.clone());
            action = Some(Box::new(move |app: &mut LauncherApp| app.start_password_sign_in(n, p)));
        }
        let cancel_width = button_width(&painter, "Cancel");
        let cancel = Rect::from_min_size(Pos2::new(primary.left() - BUTTON_GAP - cancel_width, bottom), Vec2::new(cancel_width, BUTTON_H));
        if button(ui, cancel, "sign-in-cancel", "Cancel", Skin::Gray, true) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            action = Some(Box::new(|app: &mut LauncherApp| app.phase = Phase::Ready));
        }
        if let Phase::SignIn { name: n, password: p, .. } = &mut self.phase {
            *n = name;
            *p = password;
        }
        if let Some(action) = action {
            action(self);
        }
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(msg) = self.rx.try_recv() {
            self.handle(msg);
        }
        let now = ctx.input(|i| i.time);
        if !self.busy() && !self.setup && now - self.server_checked > SERVER_RECHECK_SECONDS {
            self.check_server(false);
        }
        if let Phase::Playing { account, token } = &self.phase {
            if now - self.avatar_checked > AVATAR_RECHECK_SECONDS {
                if token.is_empty() {
                    self.avatar_checked = now;
                    self.fetch_game_login_avatar();
                } else {
                    let (account, token) = (Some(account.clone()).filter(|a| !a.is_empty()), token.clone());
                    self.fetch_avatar(account, token);
                }
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(SERVER_RECHECK_SECONDS));
        let (focused, minimized) = ctx.input(|i| (i.viewport().focused.unwrap_or(true), i.viewport().minimized.unwrap_or(false)));
        let animating = (self.page == Page::Play && matches!(self.phase, Phase::Ready)) || matches!(self.phase, Phase::Updating { .. } | Phase::SigningIn { .. });
        if animating && !minimized {
            ctx.request_repaint_after(std::time::Duration::from_millis(if focused { 33 } else { 500 }));
        }
        let overlay = self.setup || matches!(self.phase, Phase::Failed { .. } | Phase::Choose { .. } | Phase::SignIn { .. });
        self.cover.tick(self.page == Page::Play && focused && !minimized && !overlay, now);
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(MAIN)).show(ctx, |ui| {
            let screen = ui.max_rect();
            let side = Rect::from_min_max(screen.min, Pos2::new(screen.left() + SIDEBAR, screen.bottom()));
            let header = Rect::from_min_max(Pos2::new(side.right(), screen.top()), Pos2::new(screen.right(), screen.top() + HEADER));
            let body = Rect::from_min_max(Pos2::new(side.right(), header.bottom()), screen.max);
            vertical_fade(ui.painter(), body, MAIN, NAVY_BOTTOM);
            self.content(ui, body);
            self.header(ui, header);
            self.sidebar(ui, side);
            self.notice(ui, body);
            self.setup_screen(ui, screen);
            self.modal(ui, screen);
            ui.painter().rect_stroke(screen, CornerRadius::ZERO, Stroke::new(1.0, WINDOW), StrokeKind::Inside);
        });
    }
}
