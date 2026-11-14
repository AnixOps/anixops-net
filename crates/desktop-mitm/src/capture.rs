//! Saving the engine's capture exports (`export_capture`: pcapng or HAR) for the user. The engine answers with the
//! file as base64; the desktop clients decode it here and put it in the user's Downloads folder.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// One `export_capture` answer: the engine's file name and the decoded file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub file_name: String,
    pub content_type: String,
    /// Still base64, as the engine sent it.
    pub data_base64: String,
}

impl Export {
    /// Reads an `export_capture` answer: `{"file_name", "content_type", "data_base64", ...}`.
    pub fn from_answer(answer: &Value) -> Result<Self, String> {
        let text = |key: &str| {
            answer
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("the capture export has no {key}"))
        };
        Ok(Self {
            file_name: safe_file_name(&text("file_name")?),
            content_type: text("content_type")?,
            data_base64: text("data_base64")?,
        })
    }

    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        decode_base64(&self.data_base64)
    }
}

/// `name` without directories or characters Windows refuses in file names.
pub fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if cleaned.is_empty() {
        "anixops-capture".to_string()
    } else {
        cleaned.to_string()
    }
}

/// Standard base64 with or without padding; whitespace is skipped.
pub fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    fn value(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some(u32::from(byte - b'A')),
            b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    let mut ended = false;
    for byte in text.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        if byte == b'=' {
            ended = true;
            continue;
        }
        if ended {
            return Err("the capture export is not valid base64".to_string());
        }
        let digit = value(byte).ok_or("the capture export is not valid base64")?;
        acc = (acc << 6) | digit;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// The user's Downloads folder: `XDG_DOWNLOAD_DIR` or the `user-dirs.dirs` entry on Linux, `%USERPROFILE%\Downloads`
/// on Windows, `$HOME/Downloads` otherwise. Falls back to the home folder when there is no Downloads folder.
pub fn downloads_dir() -> Option<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    if !cfg!(windows) {
        if let Some(dir) = std::env::var_os("XDG_DOWNLOAD_DIR").filter(|value| !value.is_empty()) {
            return Some(PathBuf::from(dir));
        }
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        if let Some(dir) = fs::read_to_string(config.join("user-dirs.dirs"))
            .ok()
            .and_then(|text| user_dirs_download(&text, &home))
        {
            return Some(dir);
        }
    }
    let downloads = home.join("Downloads");
    Some(if downloads.is_dir() { downloads } else { home })
}

/// The `XDG_DOWNLOAD_DIR="$HOME/..."` line of a `user-dirs.dirs` file.
fn user_dirs_download(text: &str, home: &Path) -> Option<PathBuf> {
    let value = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("XDG_DOWNLOAD_DIR="))?
        .trim_matches('"');
    if let Some(rest) = value.strip_prefix("$HOME") {
        let rest = rest.trim_start_matches('/');
        return Some(if rest.is_empty() {
            home.to_path_buf()
        } else {
            home.join(rest)
        });
    }
    value.starts_with('/').then(|| PathBuf::from(value))
}

/// Writes `data` as `name` in `dir`, adding " (2)", " (3)"... before the extension instead of replacing a file.
pub fn save_new_file(dir: &Path, name: &str, data: &[u8]) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let name = safe_file_name(name);
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem.to_string(), format!(".{extension}")),
        _ => (name.clone(), String::new()),
    };
    for attempt in 1..1000 {
        let candidate = if attempt == 1 {
            dir.join(&name)
        } else {
            dir.join(format!("{stem} ({attempt}){extension}"))
        };
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                file.write_all(data)
                    .map_err(|error| format!("{}: {error}", candidate.display()))?;
                return Ok(candidate);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("{}: {error}", candidate.display())),
        }
    }
    Err(format!("{}: too many files named {name}", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_base64_with_and_without_padding() {
        assert_eq!(decode_base64("").unwrap(), b"");
        assert_eq!(decode_base64("Zg==").unwrap(), b"f");
        assert_eq!(decode_base64("Zm8=").unwrap(), b"fo");
        assert_eq!(decode_base64("Zm9v").unwrap(), b"foo");
        assert_eq!(decode_base64("Zm9vYg").unwrap(), b"foob");
        assert_eq!(
            decode_base64("AP/+\n7w==").unwrap(),
            [0x00, 0xff, 0xfe, 0xef]
        );
        assert!(decode_base64("Zm9v!").is_err());
        assert!(decode_base64("Zg==Zg").is_err());
    }

    #[test]
    fn file_names_lose_directories_and_reserved_characters() {
        assert_eq!(
            safe_file_name("anixops-capture-7.pcapng"),
            "anixops-capture-7.pcapng"
        );
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("C:\\x\\a:b?.har"), "a_b_.har");
        assert_eq!(safe_file_name(".."), "anixops-capture");
    }

    #[test]
    fn reads_the_download_folder_of_user_dirs() {
        let home = Path::new("/home/a");
        let text =
            "# comment\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_DOWNLOAD_DIR=\"$HOME/下载\"\n";
        assert_eq!(
            user_dirs_download(text, home),
            Some(PathBuf::from("/home/a/下载"))
        );
        assert_eq!(
            user_dirs_download("XDG_DOWNLOAD_DIR=\"/data/dl\"", home),
            Some(PathBuf::from("/data/dl"))
        );
        assert_eq!(
            user_dirs_download("XDG_DOWNLOAD_DIR=\"$HOME/\"", home),
            Some(home.to_path_buf())
        );
        assert_eq!(
            user_dirs_download("XDG_DESKTOP_DIR=\"$HOME/Desktop\"", home),
            None
        );
    }

    #[test]
    fn never_replaces_an_existing_file() {
        let dir = std::env::temp_dir().join(format!("desktop-mitm-capture-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let first = save_new_file(&dir, "a.har", b"1").unwrap();
        let second = save_new_file(&dir, "a.har", b"2").unwrap();
        assert_eq!(first, dir.join("a.har"));
        assert_eq!(second, dir.join("a (2).har"));
        assert_eq!(fs::read(&first).unwrap(), b"1");
        assert_eq!(fs::read(&second).unwrap(), b"2");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_an_export_answer() {
        let answer = serde_json::json!({
            "format": "har",
            "file_name": "anixops-capture-3.har",
            "content_type": "application/json",
            "size": 2,
            "data_base64": "e30=",
        });
        let export = Export::from_answer(&answer).unwrap();
        assert_eq!(export.file_name, "anixops-capture-3.har");
        assert_eq!(export.bytes().unwrap(), b"{}");
        assert!(Export::from_answer(&serde_json::json!({})).is_err());
    }
}
