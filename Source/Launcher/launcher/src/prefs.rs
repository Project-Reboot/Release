use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;

pub const TOKEN_PREF: &str = "CognitoIdentity";
pub const LOGGED_OUT_PREF: &str = "loggedOut";

pub fn unity_hash(name: &str) -> u32 {
    name.chars().fold(5381u32, |hash, c| hash.wrapping_mul(33) ^ c as u32)
}

pub fn unity_value_name(name: &str) -> String {
    format!("{name}_h{}", unity_hash(name))
}

pub mod unity_file {
    use super::*;

    const DEFAULT_HEADER: &str = r#"<unity_prefs version_major="1" version_minor="1">"#;
    const FOOTER: &str = "</unity_prefs>";

    fn escape(text: &str) -> String {
        text.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
    }

    fn unescape(text: &str) -> String {
        text.replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&apos;", "'").replace("&amp;", "&")
    }

    fn entries(document: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut rest = document;
        while let Some(start) = rest.find("<pref ") {
            let after = &rest[start..];
            let end = match (after.find("</pref>"), after.find("/>")) {
                (Some(close), Some(empty)) if empty < close && !after[..empty].contains('>') => empty + 2,
                (Some(close), _) => close + "</pref>".len(),
                (None, Some(empty)) => empty + 2,
                (None, None) => break,
            };
            found.push(after[..end].to_string());
            rest = &after[end..];
        }
        found
    }

    fn attribute(entry: &str, name: &str) -> Option<String> {
        let marker = format!("{name}=\"");
        let start = entry.find(&marker)? + marker.len();
        let end = entry[start..].find('"')? + start;
        Some(unescape(&entry[start..end]))
    }

    fn value(entry: &str) -> Option<String> {
        let open = entry.find('>')? + 1;
        let close = entry.rfind("</pref>")?;
        (open <= close).then(|| unescape(&entry[open..close]))
    }

    fn header(document: &str) -> String {
        document
            .find("<unity_prefs")
            .and_then(|start| document[start..].find('>').map(|end| document[start..start + end + 1].to_string()))
            .unwrap_or_else(|| DEFAULT_HEADER.to_string())
    }

    pub fn string_pref(name: &str, text: &str) -> String {
        format!(r#"<pref name="{}" type="string">{}</pref>"#, escape(name), STANDARD.encode(text.as_bytes()))
    }

    pub fn int_pref(name: &str, number: i32) -> String {
        format!(r#"<pref name="{}" type="int">{number}</pref>"#, escape(name))
    }

    pub fn set(document: &str, updates: &[(&str, String)]) -> String {
        let mut list = entries(document);
        for (name, replacement) in updates {
            match list.iter().position(|entry| attribute(entry, "name").as_deref() == Some(*name)) {
                Some(index) => list[index] = replacement.clone(),
                None => list.push(replacement.clone()),
            }
        }
        let mut out = header(document);
        out.push('\n');
        for entry in &list {
            out.push('\t');
            out.push_str(entry);
            out.push('\n');
        }
        out.push_str(FOOTER);
        out.push('\n');
        out
    }

    pub fn read_string(document: &str, name: &str) -> Option<String> {
        let entry = entries(document).into_iter().find(|entry| attribute(entry, "name").as_deref() == Some(name))?;
        if attribute(&entry, "type").as_deref() != Some("string") {
            return None;
        }
        let bytes = STANDARD.decode(value(&entry)?.trim()).ok()?;
        String::from_utf8(bytes).ok()
    }

    pub fn read_int(document: &str, name: &str) -> Option<i32> {
        let entry = entries(document).into_iter().find(|entry| attribute(entry, "name").as_deref() == Some(name))?;
        value(&entry)?.trim().parse().ok()
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use winreg::enums::{RegType, HKEY_CURRENT_USER, KEY_READ};
    use winreg::{RegKey, RegValue};

    fn unity_string(text: &str) -> RegValue {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        RegValue { bytes, vtype: RegType::REG_BINARY }
    }

    fn open_for_writing(key_path: &str) -> Result<RegKey, String> {
        RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(key_path)
            .map(|(key, _)| key)
            .map_err(|e| format!("could not open the game settings: {e}"))
    }

    pub fn write_login(location: &str, token: &str) -> Result<(), String> {
        let key = open_for_writing(location)?;
        key.set_raw_value(unity_value_name(TOKEN_PREF), &unity_string(token)).map_err(|e| format!("could not save the login for the game: {e}"))?;
        key.set_value(unity_value_name(LOGGED_OUT_PREF), &0u32).map_err(|e| format!("could not save the login for the game: {e}"))?;
        Ok(())
    }

    pub fn clear_login(location: &str) -> Result<(), String> {
        let key = open_for_writing(location)?;
        key.set_raw_value(unity_value_name(TOKEN_PREF), &unity_string("")).map_err(|e| format!("could not clear the game login: {e}"))?;
        key.set_value(unity_value_name(LOGGED_OUT_PREF), &1u32).map_err(|e| format!("could not clear the game login: {e}"))?;
        Ok(())
    }

    pub fn read_token(location: &str) -> Option<String> {
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(location, KEY_READ).ok()?;
        let value = key.get_raw_value(unity_value_name(TOKEN_PREF)).ok()?;
        let bytes: Vec<u8> = value.bytes.iter().copied().take_while(|b| *b != 0).collect();
        String::from_utf8(bytes).ok()
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn save(location: &str, updates: &[(&str, String)]) -> Result<(), String> {
        let path = Path::new(location);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("could not open the game settings: {e}"))?;
        }
        let current = fs::read_to_string(path).unwrap_or_default();
        let next = unity_file::set(&current, updates);
        let partial = path.with_extension("launcher-tmp");
        fs::write(&partial, next).map_err(|e| format!("could not save the game settings: {e}"))?;
        fs::rename(&partial, path).map_err(|e| format!("could not save the game settings: {e}"))
    }

    pub fn write_login(location: &str, token: &str) -> Result<(), String> {
        save(location, &[(TOKEN_PREF, unity_file::string_pref(TOKEN_PREF, token)), (LOGGED_OUT_PREF, unity_file::int_pref(LOGGED_OUT_PREF, 0))])
            .map_err(|e| format!("could not save the login for the game: {e}"))
    }

    pub fn clear_login(location: &str) -> Result<(), String> {
        save(location, &[(TOKEN_PREF, unity_file::string_pref(TOKEN_PREF, "")), (LOGGED_OUT_PREF, unity_file::int_pref(LOGGED_OUT_PREF, 1))])
            .map_err(|e| format!("could not clear the game login: {e}"))
    }

    pub fn read_token(location: &str) -> Option<String> {
        unity_file::read_string(&fs::read_to_string(location).ok()?, TOKEN_PREF)
    }
}

pub use platform::{clear_login, read_token, write_login};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_names_match_what_the_client_writes() {
        assert_eq!(unity_value_name("CognitoIdentity"), "CognitoIdentity_h1401444196");
        assert_eq!(unity_value_name("loggedOut"), "loggedOut_h2412648585");
        assert_eq!(unity_value_name("lastLoginKey"), "lastLoginKey_h2295063355");
        assert_eq!(unity_value_name("PasswordForUser"), "PasswordForUser_h1077986352");
    }

    #[test]
    fn the_linux_prefs_file_keeps_the_games_other_settings() {
        let original = "<unity_prefs version_major=\"1\" version_minor=\"1\">\n\t<pref name=\"Screenmanager Fullscreen mode\" type=\"int\">3</pref>\n\t<pref name=\"language\" type=\"string\">RW5nbGlzaA==</pref>\n\t<pref name=\"loggedOut\" type=\"int\">1</pref>\n</unity_prefs>\n";
        let updated = unity_file::set(original, &[(TOKEN_PREF, unity_file::string_pref(TOKEN_PREF, "test-token")), (LOGGED_OUT_PREF, unity_file::int_pref(LOGGED_OUT_PREF, 0))]);
        assert!(updated.starts_with("<unity_prefs version_major=\"1\" version_minor=\"1\">"));
        assert!(updated.contains("<pref name=\"Screenmanager Fullscreen mode\" type=\"int\">3</pref>"));
        assert_eq!(unity_file::read_string(&updated, "language").as_deref(), Some("English"));
        assert_eq!(unity_file::read_string(&updated, TOKEN_PREF).as_deref(), Some("test-token"));
        assert_eq!(unity_file::read_int(&updated, LOGGED_OUT_PREF), Some(0));
        assert_eq!(updated.matches("name=\"loggedOut\"").count(), 1);
        assert!(updated.contains("type=\"string\">dGVzdC10b2tlbg==</pref>"));
    }

    #[test]
    fn an_empty_linux_prefs_file_gets_a_unity_header() {
        let created = unity_file::set("", &[(TOKEN_PREF, unity_file::string_pref(TOKEN_PREF, ""))]);
        assert!(created.starts_with("<unity_prefs version_major=\"1\" version_minor=\"1\">\n"));
        assert!(created.trim_end().ends_with("</unity_prefs>"));
        assert_eq!(unity_file::read_string(&created, TOKEN_PREF).as_deref(), Some(""));
    }

    #[cfg(windows)]
    #[test]
    fn a_login_round_trips_through_a_scratch_key() {
        use winreg::enums::{RegType, HKEY_CURRENT_USER};
        use winreg::RegKey;
        let key = format!(r"Software\RebootTeam\Project Reboot Launcher Test {}", std::process::id());
        write_login(&key, "test-token").unwrap();
        assert_eq!(read_token(&key).as_deref(), Some("test-token"));
        let raw = RegKey::predef(HKEY_CURRENT_USER).open_subkey(&key).unwrap().get_raw_value(unity_value_name(TOKEN_PREF)).unwrap();
        assert_eq!(raw.vtype, RegType::REG_BINARY);
        assert_eq!(raw.bytes.last(), Some(&0u8));
        let logged_out: u32 = RegKey::predef(HKEY_CURRENT_USER).open_subkey(&key).unwrap().get_value(unity_value_name(LOGGED_OUT_PREF)).unwrap();
        assert_eq!(logged_out, 0);
        clear_login(&key).unwrap();
        assert_eq!(read_token(&key).as_deref(), Some(""));
        RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&key).unwrap();
    }

    #[cfg(not(windows))]
    #[test]
    fn a_login_round_trips_through_a_scratch_prefs_file() {
        let dir = std::env::temp_dir().join(format!("pr-launcher-prefs-{}", std::process::id()));
        let file = dir.join("prefs");
        let location = file.to_string_lossy().to_string();
        write_login(&location, "test-token").unwrap();
        assert_eq!(read_token(&location).as_deref(), Some("test-token"));
        clear_login(&location).unwrap();
        assert_eq!(read_token(&location).as_deref(), Some(""));
        let _ = std::fs::remove_dir_all(dir);
    }
}
