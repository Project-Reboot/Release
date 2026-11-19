use std::collections::HashSet;
use std::io::Read;

pub use ed25519_dalek::{SigningKey, VerifyingKey};
use ed25519_dalek::{Signature, Signer, Verifier};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MANIFEST_FILE: &str = "manifest.json";
pub const SIGNATURE_FILE: &str = "manifest.json.sig";
pub const LAUNCHER_FILES: [&str; 2] = ["ProjectRebootLauncher.exe", "ProjectRebootLauncher"];
pub const LAUNCHER_LIST_FILE: &str = "launcher.json";
pub const LAUNCHER_LIST_SIGNATURE: &str = "launcher.json.sig";
pub const WINDOWS_LAUNCHER_PATH: &str = "Windows Launcher/ProjectRebootLauncher.exe";
pub const LINUX_LAUNCHER_PATH: &str = "Linux Launcher/ProjectRebootLauncher";
pub const MAX_LAUNCHER_BYTES: u64 = 64 * 1024 * 1024;
const LAUNCHER_LIST_CONTEXT: &[u8] = b"Project Reboot launcher list\n";
pub const MAX_NOTES_BYTES: usize = 32 * 1024;
pub const POSTER_FILE: &str = "launcher-news.png";
pub const MAX_POSTER_BYTES: u64 = 4 * 1024 * 1024;
pub const PACKED_SUFFIX: &str = ".zst";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub version: String,
    pub exe: String,
    pub files: Vec<FileEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<PosterEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PosterEntry {
    pub size: u64,
    pub sha256: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LauncherList {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<LauncherBuild>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linux: Option<LauncherBuild>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LauncherBuild {
    pub file: String,
    pub size: u64,
    pub sha256: String,
}

impl LauncherList {
    pub fn validate(&self) -> Result<(), String> {
        if parse_version(&self.version).is_none() {
            return Err("the launcher list has no valid version".into());
        }
        if self.windows.is_none() && self.linux.is_none() {
            return Err("the launcher list has no launcher in it".into());
        }
        for (build, expected) in [(&self.windows, WINDOWS_LAUNCHER_PATH), (&self.linux, LINUX_LAUNCHER_PATH)] {
            if let Some(build) = build {
                if build.file != expected || !is_sha256_hex(&build.sha256) || build.size == 0 || build.size > MAX_LAUNCHER_BYTES {
                    return Err(format!("the launcher list has a bad entry for {expected}"));
                }
            }
        }
        Ok(())
    }

    pub fn for_this_system(&self) -> Option<&LauncherBuild> {
        if cfg!(windows) {
            self.windows.as_ref()
        } else {
            self.linux.as_ref()
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packed: Option<Packed>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Packed {
    pub size: u64,
    pub sha256: String,
}

impl FileEntry {
    pub fn packed_path(&self) -> String {
        format!("{}{PACKED_SUFFIX}", self.path)
    }

    pub fn transfer_size(&self) -> u64 {
        self.packed.as_ref().map(|p| p.size).unwrap_or(self.size)
    }
}

impl Manifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.version.trim().is_empty() {
            return Err("the update list has no version".into());
        }
        let mut seen = HashSet::new();
        for file in &self.files {
            if !is_safe_path(&file.path) {
                return Err(format!("the update list has an unsafe path: {}", file.path));
            }
            if !is_sha256_hex(&file.sha256) {
                return Err(format!("the update list has a bad hash for {}", file.path));
            }
            if !seen.insert(file.path.to_ascii_lowercase()) {
                return Err(format!("the update list names {} twice", file.path));
            }
            if let Some(packed) = &file.packed {
                if !is_sha256_hex(&packed.sha256) || packed.size == 0 || packed.size > file.size.saturating_add(4096) {
                    return Err(format!("the update list has a bad packed entry for {}", file.path));
                }
            }
        }
        if !seen.contains(&self.exe.to_ascii_lowercase()) {
            return Err(format!("the update list does not contain the game exe {}", self.exe));
        }
        if self.notes.as_ref().is_some_and(|n| n.len() > MAX_NOTES_BYTES) {
            return Err("the update notes are too long".into());
        }
        if let Some(poster) = &self.poster {
            if !is_sha256_hex(&poster.sha256) || poster.size == 0 || poster.size > MAX_POSTER_BYTES {
                return Err("the update list has a bad poster entry".into());
            }
        }
        Ok(())
    }

    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    pub fn transfer_size(&self) -> u64 {
        self.files.iter().map(FileEntry::transfer_size).sum()
    }
}

pub fn parse_version(text: &str) -> Option<Vec<u64>> {
    let parts: Option<Vec<u64>> = text.trim().split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty() && p.len() <= 4)
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(mut a), Some(mut b)) => {
            let len = a.len().max(b.len());
            a.resize(len, 0);
            b.resize(len, 0);
            a > b
        }
        _ => false,
    }
}

pub fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn is_safe_path(path: &str) -> bool {
    if path.is_empty() || path.len() > 260 || path.starts_with('/') || path.contains('\\') || path.contains(':') {
        return false;
    }
    path.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && !part.ends_with('.')
            && !part.ends_with(' ')
            && !part.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
    })
}

pub fn parse_public_key(hex_text: &str) -> Result<VerifyingKey, String> {
    let bytes: [u8; 32] = hex::decode(hex_text.trim())
        .map_err(|_| "the public key is not hex".to_string())?
        .try_into()
        .map_err(|_| "the public key must be 32 bytes".to_string())?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| "the public key is not valid".to_string())
}

pub fn parse_private_key(hex_text: &str) -> Result<SigningKey, String> {
    let bytes: [u8; 32] = hex::decode(hex_text.trim())
        .map_err(|_| "the private key is not hex".to_string())?
        .try_into()
        .map_err(|_| "the private key must be 32 bytes".to_string())?;
    Ok(SigningKey::from_bytes(&bytes))
}

pub fn public_key_hex(key: &SigningKey) -> String {
    hex::encode(key.verifying_key().to_bytes())
}

pub fn sign(manifest_bytes: &[u8], key: &SigningKey) -> String {
    hex::encode(key.sign(manifest_bytes).to_bytes())
}

fn check_signature(signed: &[u8], signature_text: &str, public_key: &VerifyingKey) -> Result<(), String> {
    let signature: [u8; 64] = hex::decode(signature_text.trim())
        .map_err(|_| "the update signature is not hex".to_string())?
        .try_into()
        .map_err(|_| "the update signature has the wrong length".to_string())?;
    public_key
        .verify(signed, &Signature::from_bytes(&signature))
        .map_err(|_| "the update list is not signed by Project Reboot".to_string())
}

fn launcher_signed_bytes(list_bytes: &[u8]) -> Vec<u8> {
    [LAUNCHER_LIST_CONTEXT, list_bytes].concat()
}

pub fn sign_launcher_list(list_bytes: &[u8], key: &SigningKey) -> String {
    hex::encode(key.sign(&launcher_signed_bytes(list_bytes)).to_bytes())
}

pub fn verify_launcher_list(list_bytes: &[u8], signature_text: &str, public_key: &VerifyingKey) -> Result<LauncherList, String> {
    check_signature(&launcher_signed_bytes(list_bytes), signature_text, public_key)?;
    let list: LauncherList = serde_json::from_slice(list_bytes).map_err(|e| format!("the launcher list is not valid: {e}"))?;
    list.validate()?;
    Ok(list)
}

pub fn verify(manifest_bytes: &[u8], signature_text: &str, public_key: &VerifyingKey) -> Result<Manifest, String> {
    check_signature(manifest_bytes, signature_text, public_key)?;
    let manifest: Manifest = serde_json::from_slice(manifest_bytes).map_err(|e| format!("the update list is not valid: {e}"))?;
    manifest.validate()?;
    Ok(manifest)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn hash_reader(mut reader: impl Read) -> std::io::Result<(String, u64)> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((hex::encode(hasher.finalize()), size))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn sample() -> Manifest {
        Manifest {
            version: "1.3.0".into(),
            exe: "Project Reboot.exe".into(),
            files: vec![
                FileEntry { path: "Project Reboot.exe".into(), size: 3, sha256: sha256_hex(b"exe"), packed: None },
                FileEntry { path: "Project Reboot_Data/data.unity3d".into(), size: 4, sha256: sha256_hex(b"data"), packed: None },
            ],
            notes: None,
            poster: None,
        }
    }

    fn list() -> LauncherList {
        LauncherList {
            version: "0.2.0".into(),
            windows: Some(LauncherBuild { file: WINDOWS_LAUNCHER_PATH.into(), size: 10, sha256: sha256_hex(b"windows") }),
            linux: Some(LauncherBuild { file: LINUX_LAUNCHER_PATH.into(), size: 12, sha256: sha256_hex(b"linux") }),
        }
    }

    #[test]
    fn a_signed_manifest_verifies() {
        let bytes = serde_json::to_vec(&sample()).unwrap();
        let signature = sign(&bytes, &key());
        let public = parse_public_key(&public_key_hex(&key())).unwrap();
        assert_eq!(verify(&bytes, &signature, &public).unwrap(), sample());
    }

    #[test]
    fn a_changed_manifest_is_rejected() {
        let bytes = serde_json::to_vec(&sample()).unwrap();
        let signature = sign(&bytes, &key());
        let mut tampered = sample();
        tampered.files[0].sha256 = sha256_hex(b"malware");
        let tampered_bytes = serde_json::to_vec(&tampered).unwrap();
        let public = parse_public_key(&public_key_hex(&key())).unwrap();
        assert!(verify(&tampered_bytes, &signature, &public).is_err());
    }

    #[test]
    fn another_key_is_rejected() {
        let bytes = serde_json::to_vec(&sample()).unwrap();
        let signature = sign(&bytes, &SigningKey::from_bytes(&[9u8; 32]));
        let public = parse_public_key(&public_key_hex(&key())).unwrap();
        assert!(verify(&bytes, &signature, &public).is_err());
    }

    #[test]
    fn unsafe_paths_are_refused() {
        for bad in ["", "/abs", "..\\x", "a/../b", "C:/x", "a//b", "a/./b", "a\\b", "con.", "x?", "a/b "] {
            assert!(!is_safe_path(bad), "{bad}");
        }
        for good in ["Project Reboot.exe", "Project Reboot_Data/il2cpp_data/Metadata/global-metadata.dat", "D3D12/D3D12Core.dll"] {
            assert!(is_safe_path(good), "{good}");
        }
    }

    #[test]
    fn a_manifest_without_its_exe_is_refused() {
        let mut manifest = sample();
        manifest.exe = "Other.exe".into();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn duplicate_paths_are_refused_ignoring_case() {
        let mut manifest = sample();
        let mut copy = manifest.files[1].clone();
        copy.path = copy.path.to_uppercase();
        manifest.files.push(copy);
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(is_newer("1.0.1", "1.0"));
        assert!(!is_newer("1.0", "1.0.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("abc", "0.1.0"));
    }

    #[test]
    fn a_signed_launcher_list_round_trips() {
        let bytes = serde_json::to_vec(&list()).unwrap();
        let signature = sign_launcher_list(&bytes, &key());
        let public = parse_public_key(&public_key_hex(&key())).unwrap();
        assert_eq!(verify_launcher_list(&bytes, &signature, &public).unwrap(), list());
        let mut tampered = list();
        tampered.windows.as_mut().unwrap().sha256 = sha256_hex(b"malware");
        assert!(verify_launcher_list(&serde_json::to_vec(&tampered).unwrap(), &signature, &public).is_err());
    }

    #[test]
    fn game_and_launcher_signatures_cannot_stand_in_for_each_other() {
        let public = parse_public_key(&public_key_hex(&key())).unwrap();
        let list_bytes = serde_json::to_vec(&list()).unwrap();
        assert!(verify_launcher_list(&list_bytes, &sign(&list_bytes, &key()), &public).is_err());
        let game_bytes = serde_json::to_vec(&sample()).unwrap();
        assert!(verify(&game_bytes, &sign_launcher_list(&game_bytes, &key()), &public).is_err());
        assert!(verify_launcher_list(&game_bytes, &sign_launcher_list(&game_bytes, &key()), &public).is_err());
    }

    #[test]
    fn a_launcher_list_only_points_at_the_launcher_folders() {
        let mut bad = list();
        bad.windows.as_mut().unwrap().file = "PC/Project Reboot.exe".into();
        assert!(bad.validate().is_err());
        let mut bad = list();
        bad.linux.as_mut().unwrap().size = MAX_LAUNCHER_BYTES + 1;
        assert!(bad.validate().is_err());
        let empty = LauncherList { version: "0.2.0".into(), windows: None, linux: None };
        assert!(empty.validate().is_err());
        let mut no_version = list();
        no_version.version = "soon".into();
        assert!(no_version.validate().is_err());
    }

    #[test]
    fn each_system_gets_its_own_launcher() {
        let picked = list().for_this_system().cloned().unwrap();
        assert_eq!(picked.file, if cfg!(windows) { WINDOWS_LAUNCHER_PATH } else { LINUX_LAUNCHER_PATH });
    }

    #[test]
    fn old_lists_with_a_launcher_entry_still_read() {
        let mut value = serde_json::to_value(sample()).unwrap();
        value["launcher"] = serde_json::json!({"version": "0.1.1", "size": 10, "sha256": sha256_hex(b"x"), "file": "ProjectRebootLauncher.exe"});
        let bytes = serde_json::to_vec(&value).unwrap();
        let public = parse_public_key(&public_key_hex(&key())).unwrap();
        assert_eq!(verify(&bytes, &sign(&bytes, &key()), &public).unwrap(), sample());
    }

    #[test]
    fn an_oversized_poster_is_refused() {
        let mut manifest = sample();
        manifest.poster = Some(PosterEntry { size: MAX_POSTER_BYTES + 1, sha256: sha256_hex(b"poster") });
        assert!(manifest.validate().is_err());
        manifest.poster = Some(PosterEntry { size: 10, sha256: sha256_hex(b"poster") });
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn packed_entries_are_signed_and_checked() {
        let mut manifest = sample();
        manifest.files[1].packed = Some(Packed { size: 2, sha256: sha256_hex(b"zz") });
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let public = parse_public_key(&public_key_hex(&key())).unwrap();
        let verified = verify(&bytes, &sign(&bytes, &key()), &public).unwrap();
        assert_eq!(verified.files[1].packed, manifest.files[1].packed);
        assert_eq!(verified.files[1].packed_path(), "Project Reboot_Data/data.unity3d.zst");
        assert_eq!(verified.transfer_size(), 3 + 2);
        assert_eq!(verified.total_size(), 3 + 4);
        let mut bad = sample();
        bad.files[0].packed = Some(Packed { size: 1_000_000, sha256: sha256_hex(b"zz") });
        assert!(bad.validate().is_err());
        bad.files[0].packed = Some(Packed { size: 2, sha256: "nothex".into() });
        assert!(bad.validate().is_err());
    }

    #[test]
    fn older_lists_without_packed_files_still_read() {
        let entry: FileEntry = serde_json::from_str(r#"{"path":"a.dll","size":3,"sha256":"00"}"#).unwrap();
        assert_eq!(entry.packed, None);
        assert_eq!(entry.transfer_size(), 3);
        assert!(!serde_json::to_string(&entry).unwrap().contains("packed"));
    }

    #[test]
    fn hashing_a_reader_matches_hashing_bytes() {
        let data = vec![42u8; 3_000_000];
        let (hash, size) = hash_reader(&data[..]).unwrap();
        assert_eq!(hash, sha256_hex(&data));
        assert_eq!(size, 3_000_000);
    }
}
