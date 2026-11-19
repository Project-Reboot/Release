use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use sha2::{Digest, Sha256};
use url::Url;

use crate::config::Config;
use crate::log;

pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_REQUEST_BYTES: usize = 16 * 1024;

pub struct Code {
    pub code: String,
    pub redirect_uri: String,
    pub verifier: String,
}

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn new_pkce() -> Result<Pkce, String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("could not start the sign-in: {e}"))?;
    let verifier = URL_SAFE_NO_PAD.encode(bytes);
    let challenge = challenge_for(&verifier);
    Ok(Pkce { verifier, challenge })
}

enum Outcome {
    Code(Code),
    NeedsConsent,
}

fn random_state() -> Result<String, String> {
    let mut bytes = [0u8; 24];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("could not start the sign-in: {e}"))?;
    Ok(hex::encode(bytes))
}

pub fn authorize_url(cfg: &Config, state: &str, challenge: &str, skip_consent: bool, port: u16) -> Result<Url, String> {
    let mut url = Url::parse(&cfg.discord_authorize_url).map_err(|e| format!("the Discord sign-in address is not valid: {e}"))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &cfg.discord_client_id)
        .append_pair("scope", "identify")
        .append_pair("state", state)
        .append_pair("redirect_uri", &Config::redirect_uri(port))
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("prompt", if skip_consent { "none" } else { "consent" });
    Ok(url)
}

fn open_browser(cfg: &Config, url: &Url) -> Result<(), String> {
    match &cfg.browser {
        Some(program) => Command::new(program).arg(url.as_str()).spawn().map(|_| ()).map_err(|e| format!("could not open the browser: {e}")),
        None => open::that_detached(url.as_str()).map_err(|e| format!("could not open your browser: {e}")),
    }
}

pub fn sign_in(cfg: &Config, cancel: &AtomicBool, on_link: &dyn Fn(String)) -> Result<Code, String> {
    match authorize(cfg, cancel, true, on_link)? {
        Outcome::Code(code) => Ok(code),
        Outcome::NeedsConsent => match authorize(cfg, cancel, false, on_link)? {
            Outcome::Code(code) => Ok(code),
            Outcome::NeedsConsent => Err("Discord did not finish the sign-in. Please try again.".into()),
        },
    }
}

fn listen(cfg: &Config) -> Result<(TcpListener, u16), String> {
    let mut busy = false;
    for &port in &cfg.redirect_ports {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => return Ok((listener, port)),
            Err(e) if e.kind() == ErrorKind::AddrInUse => busy = true,
            Err(_) => {}
        }
    }
    Err(if busy {
        "The sign-in could not start because its ports are busy. Close other Project Reboot launchers and try again.".into()
    } else {
        "Your system blocked every port the launcher uses for the Discord sign-in. Restarting your computer usually frees them.".into()
    })
}

fn authorize(cfg: &Config, cancel: &AtomicBool, skip_consent: bool, on_link: &dyn Fn(String)) -> Result<Outcome, String> {
    let (listener, port) = listen(cfg)?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let state = random_state()?;
    let redirect_uri = Config::redirect_uri(port);
    let pkce = new_pkce()?;
    let link = authorize_url(cfg, &state, &pkce.challenge, skip_consent, port)?;
    on_link(link.to_string());
    if let Err(e) = open_browser(cfg, &link) {
        log::warn(format!("The browser did not open ({e}); the sign-in link can be copied instead"));
    }
    let started = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Sign-in cancelled.".into());
        }
        if started.elapsed() >= SIGN_IN_TIMEOUT {
            return Err("The Discord sign-in took too long. Please try again.".into());
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(outcome) = handle(stream, &state, skip_consent, &redirect_uri, &pkce.verifier)? {
                    return Ok(outcome);
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(format!("the sign-in was interrupted: {e}")),
        }
    }
}

fn read_request_target(stream: &mut TcpStream) -> Option<String> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut data = Vec::new();
    let mut chunk = [0u8; 2048];
    while !data.windows(4).any(|w| w == b"\r\n\r\n") && data.len() < MAX_REQUEST_BYTES {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        data.extend_from_slice(&chunk[..read]);
    }
    let text = String::from_utf8_lossy(&data);
    let mut parts = text.lines().next()?.split_whitespace();
    (parts.next()? == "GET").then(|| parts.next().map(str::to_string)).flatten()
}

fn handle(mut stream: TcpStream, state: &str, skip_consent: bool, redirect_uri: &str, verifier: &str) -> Result<Option<Outcome>, String> {
    let Some(target) = read_request_target(&mut stream) else {
        return Ok(None);
    };
    let Ok(url) = Url::parse(&format!("http://127.0.0.1{target}")) else {
        respond(&mut stream, 400, "Something went wrong", "The sign-in link was not valid. Go back to the launcher and try again.");
        return Ok(None);
    };
    if url.path() != "/callback" {
        respond(&mut stream, 404, "Not found", "Nothing here.");
        return Ok(None);
    }
    let query: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    let value = |name: &str| query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
    if value("state") != Some(state) {
        respond(&mut stream, 400, "Sign-in expired", "This sign-in page is from an older attempt. Go back to the launcher and try again.");
        return Ok(None);
    }
    if let Some(error) = value("error") {
        if skip_consent && error != "access_denied" {
            respond(&mut stream, 200, "One more step", "Discord needs your approval once. Switch back to your browser to continue.");
            return Ok(Some(Outcome::NeedsConsent));
        }
        respond(&mut stream, 200, "Sign-in cancelled", "You cancelled the Discord sign-in. You can close this tab.");
        return Err("The Discord sign-in was cancelled.".into());
    }
    match value("code") {
        Some(code) if !code.is_empty() && code.len() <= 512 => {
            respond(&mut stream, 200, "You're signed in", "You can close this tab and go back to the Project Reboot launcher.");
            Ok(Some(Outcome::Code(Code { code: code.to_string(), redirect_uri: redirect_uri.to_string(), verifier: verifier.to_string() })))
        }
        _ => {
            respond(&mut stream, 400, "Something went wrong", "Discord did not send a sign-in code. Go back to the launcher and try again.");
            Ok(None)
        }
    }
}

fn respond(stream: &mut TcpStream, status: u16, title: &str, text: &str) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Bad Request",
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>Project Reboot</title>\
<style>body{{margin:0;min-height:100vh;display:grid;place-items:center;background:radial-gradient(circle at 50% 40%,#16525e,#071a20 70%);color:#eaf6f7;font:16px/1.5 system-ui,sans-serif}}\
main{{text-align:center;padding:32px 40px;border-radius:14px;background:#0b262dcc;box-shadow:0 0 40px #2fd3e655}}h1{{margin:0 0 8px;font-size:24px;color:#ffffff}}p{{margin:0;color:#b9d6da}}</style>\
</head><body><main><h1>{title}</h1><p>{text}</p></main></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_matches_the_rfc_7636_example() {
        assert_eq!(challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn verifiers_are_fresh_and_use_only_allowed_characters() {
        let first = new_pkce().unwrap();
        let second = new_pkce().unwrap();
        assert_ne!(first.verifier, second.verifier);
        assert_eq!(first.verifier.len(), 43);
        assert!(first.verifier.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        assert_eq!(first.challenge, challenge_for(&first.verifier));
    }

    #[test]
    fn a_taken_port_falls_through_to_the_next_one() {
        let mut cfg = Config::load();
        let blocker = TcpListener::bind("127.0.0.1:0").unwrap();
        let taken = blocker.local_addr().unwrap().port();
        let free = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        cfg.redirect_ports = vec![taken, free];
        let (_listener, port) = listen(&cfg).unwrap();
        assert_eq!(port, free);
    }

    #[test]
    fn the_redirect_ports_avoid_the_windows_dynamic_range() {
        assert!(crate::config::REDIRECT_PORTS.iter().all(|&p| p > 1024 && p < 49152));
    }

    #[test]
    fn the_authorize_link_asks_only_for_identify() {
        let mut cfg = Config::load();
        cfg.discord_client_id = "1234".into();
        let url = authorize_url(&cfg, "abc", "challenge-xyz", true, 38219).unwrap();
        let pairs: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        assert!(pairs.contains(&("scope".into(), "identify".into())));
        assert!(pairs.contains(&("client_id".into(), "1234".into())));
        assert!(pairs.contains(&("state".into(), "abc".into())));
        assert!(pairs.contains(&("prompt".into(), "none".into())));
        assert!(pairs.contains(&("code_challenge".into(), "challenge-xyz".into())));
        assert!(pairs.contains(&("code_challenge_method".into(), "S256".into())));
        assert!(pairs.contains(&("redirect_uri".into(), "http://127.0.0.1:38219/callback".into())));
    }
}
