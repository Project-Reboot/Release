
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Remembered {
    pub account: String,
    pub remember: String,
}

fn path() -> PathBuf {
    config::data_dir().join("Launcher").join("signed-in.bin")
}

pub fn load() -> Option<Remembered> {
    let bytes = unprotect(&fs::read(path()).ok()?)?;
    serde_json::from_slice::<Remembered>(&bytes).ok().filter(|r| !r.remember.is_empty())
}

pub fn save(account: &str, remember: &str) -> Result<(), String> {
    if remember.is_empty() {
        return Ok(());
    }
    let json = serde_json::to_vec(&Remembered { account: account.to_string(), remember: remember.to_string() }).map_err(|e| e.to_string())?;
    let path = path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not remember the sign-in: {e}"))?;
    }
    write_private(&path, &protect(&json).ok_or("could not protect the saved sign-in")?).map_err(|e| format!("could not remember the sign-in: {e}"))
}

pub fn clear() {
    let _ = fs::remove_file(path());
}

#[cfg(not(windows))]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    file.write_all(bytes)
}

#[cfg(windows)]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    fs::write(path, bytes)
}

#[cfg(not(windows))]
fn protect(bytes: &[u8]) -> Option<Vec<u8>> {
    Some(bytes.to_vec())
}

#[cfg(not(windows))]
fn unprotect(bytes: &[u8]) -> Option<Vec<u8>> {
    Some(bytes.to_vec())
}

#[cfg(windows)]
fn protect(bytes: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
    let input = CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe { CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) };
    take(ok, output)
}

#[cfg(windows)]
fn unprotect(bytes: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
    let input = CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe { CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) };
    take(ok, output)
}

#[cfg(windows)]
fn take(ok: i32, output: windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    if ok == 0 || output.pbData.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe { LocalFree(output.pbData as _) };
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_bytes_come_back_unchanged_and_are_not_plain_text() {
        let secret = br#"{"account":"PLAYER","remember":"USER001.ABCDEF"}"#;
        let sealed = protect(secret).unwrap();
        if cfg!(windows) {
            assert!(!sealed.windows(6).any(|w| w == b"ABCDEF"), "the token is not readable in the file");
        }
        assert_eq!(unprotect(&sealed).unwrap(), secret);
    }

    #[test]
    fn damaged_files_are_ignored() {
        if cfg!(windows) {
            assert_eq!(unprotect(b"not protected"), None);
        }
    }
}
