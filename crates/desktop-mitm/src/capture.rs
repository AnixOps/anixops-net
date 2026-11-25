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

/// What else an `export_capture` request may say: one exchange of the connection, the language of a `code`
/// export, and whether cookies, tokens and passwords are masked (the engine masks them unless told not to).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    #[serde(default)]
    pub exchange: Option<u64>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub redact: Option<bool>,
}

/// The engine's `export_capture` request: everything captured, one connection (`id`) or one of its exchanges.
pub fn export_request(format: &str, id: Option<u64>, options: ExportOptions) -> Value {
    let mut request = serde_json::json!({ "cmd": "export_capture", "format": format });
    if let Some(id) = id {
        request["id"] = Value::from(id);
        if let Some(exchange) = options.exchange {
            request["exchange"] = Value::from(exchange);
        }
    }
    if let Some(language) = options.language.filter(|language| !language.is_empty()) {
        request["language"] = Value::from(language);
    }
    if let Some(redact) = options.redact {
        request["redact"] = Value::from(redact);
    }
    request
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

/// Standard base64 with padding.
pub fn encode_base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let bytes = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let value = (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]);
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(char::from(
                    ALPHABET[((value >> (18 - 6 * index)) & 63) as usize],
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
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

/// A captured body to show decoded (`decode_body`): its bytes as base64, the response's `Content-Encoding` and
/// whether capture kept all of it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodeRequest {
    pub data_base64: String,
    pub content_encoding: String,
    #[serde(default)]
    pub complete: bool,
}

/// Undoes a captured body's `Content-Encoding` with the kernel's decoder (gzip, deflate, br, zstd, several in a
/// row), in the window's process with the inspector's limits: at most 16 MiB out, brotli windows up to 16 MiB and
/// zstd up to 8 MiB. Answers `{"data_base64", "truncated", "codings"}`; an unknown coding, a damaged body or a
/// window over the limit is an error, and the inspector then shows the bytes as captured.
pub fn decode_body(request: &DecodeRequest) -> Result<Value, String> {
    let data = decode_base64(&request.data_base64)?;
    let decoded = engine_embedded::capture::decode::decode_body(
        &data,
        &request.content_encoding,
        request.complete,
        engine_embedded::capture::decode::DecodeLimits::APP,
    )
    .map_err(|error| error.to_string())?;
    Ok(serde_json::json!({
        "data_base64": encode_base64(&decoded.data),
        "truncated": decoded.truncated,
        "codings": decoded.codings,
    }))
}

/// Writes `data` as `name` in `dir`, adding " (2)", " (3)"... before the extension instead of replacing a file. On
/// Unix the file is created 0600 (only this user can read it), whatever the process umask.
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
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        // Captured bodies and exports can hold credentials: readable by this user only, whatever the umask.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
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

/// A captured body the inspector saves or hands to another app (`save_body`, `open_body`): the bytes it shows (after
/// the Content-Encoding is undone), their `Content-Type`, and a name for the file without its extension.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BodyFile {
    pub data_base64: String,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub name: String,
}

/// The file extension for a body of `content_type`, and whether another app may be asked to open such a file.
/// Types that run code where they open (HTML, SVG, scripts, XML with style sheets, spreadsheets that read CSV
/// formulas) are saved but never opened from here; anything unknown is `bin`, which is not opened either.
fn body_type(content_type: &str) -> (&'static str, bool) {
    let media = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match media.as_str() {
        "image/png" => ("png", true),
        "image/jpeg" | "image/jpg" => ("jpg", true),
        "image/gif" => ("gif", true),
        "image/webp" => ("webp", true),
        "image/avif" => ("avif", true),
        "image/bmp" => ("bmp", true),
        "image/x-icon" | "image/vnd.microsoft.icon" => ("ico", true),
        "application/pdf" => ("pdf", true),
        "text/plain" => ("txt", true),
        "video/mp4" => ("mp4", true),
        "video/webm" => ("webm", true),
        "audio/mpeg" => ("mp3", true),
        "audio/mp4" => ("m4a", true),
        "audio/ogg" => ("ogg", true),
        "audio/wav" | "audio/x-wav" => ("wav", true),
        "application/json" => ("json", true),
        media if media.ends_with("+json") => ("json", true),
        "image/svg+xml" => ("svg", false),
        "text/html" => ("html", false),
        "text/css" => ("css", false),
        "text/csv" => ("csv", false),
        "text/javascript" | "application/javascript" => ("js", false),
        "application/xml" | "text/xml" => ("xml", false),
        media if media.ends_with("+xml") => ("xml", false),
        "application/x-www-form-urlencoded" => ("txt", false),
        "font/woff2" => ("woff2", false),
        "font/woff" => ("woff", false),
        "application/zip" => ("zip", false),
        _ => ("bin", false),
    }
}

/// Whether `open_body` will hand a body of `content_type` to another app.
pub fn may_open_body(content_type: &str) -> bool {
    body_type(content_type).1
}

/// `name` (or "body") with the extension of `content_type`, safe as a file name.
pub fn body_file_name(name: &str, content_type: &str) -> String {
    let stem = safe_file_name(name);
    let stem = if name.trim().is_empty() {
        "body"
    } else {
        stem.as_str()
    };
    format!("{stem}.{}", body_type(content_type).0)
}

/// Saves the body in `dir` without replacing an older file and answers where it went.
pub fn save_body(file: &BodyFile, dir: &Path) -> Result<PathBuf, String> {
    let data = decode_base64(&file.data_base64)?;
    save_new_file(dir, &body_file_name(&file.name, &file.content_type), &data)
}

/// How long a body handed to another app stays in [`open_dir`]: older files are removed each time one is added.
pub const OPENED_BODY_RETENTION: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// The folder name under the per-user base of [`open_dir`].
const OPEN_DIR_NAME: &str = "anixops-net-bodies";

/// Where bodies handed to other apps are written: a folder only this user can open.
///
/// - Linux and other Unix: `$XDG_RUNTIME_DIR/anixops-net-bodies` when the runtime directory is this user's and
///   private (systemd makes it 0700 on a tmpfs, so nothing outlives the session), otherwise
///   `<temp>/anixops-net-bodies-<uid>`. Either way the folder is made 0700, and an existing one must be a real
///   directory (not a link) owned by this user ([`ensure_private_dir`]); another account that created it first
///   makes opening fail instead of receiving the bodies.
/// - Windows: `%TEMP%\anixops-net-bodies`, inside the user's profile, which other standard accounts cannot read.
fn open_dir() -> Result<PathBuf, String> {
    #[cfg(unix)]
    {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute() && unix_private::is_private_dir(dir));
        let dir = match runtime {
            Some(runtime) => runtime.join(OPEN_DIR_NAME),
            None => std::env::temp_dir().join(format!("{OPEN_DIR_NAME}-{}", unix_private::uid())),
        };
        ensure_private_dir(&dir)?;
        remove_legacy_open_dir();
        Ok(dir)
    }
    #[cfg(not(unix))]
    {
        let dir = std::env::temp_dir().join(OPEN_DIR_NAME);
        fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        Ok(dir)
    }
}

/// Makes `dir` (its parent must exist) a directory only this user can open: created 0700, or, when it exists,
/// checked to be a directory that is not a link and is owned by this user, and tightened to 0700. Anything else is
/// refused, so a folder another account prepared never receives a body.
#[cfg(unix)]
pub fn ensure_private_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    let fail = |what: &str| format!("{} {what}; nothing was written", dir.display());
    match fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("{}: {error}", dir.display())),
    }
    let meta = fs::symlink_metadata(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    if !meta.file_type().is_dir() {
        return Err(fail("is not a folder"));
    }
    if meta.uid() != unix_private::uid() {
        return Err(fail("belongs to another account"));
    }
    if meta.mode() & 0o777 != 0o700 {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("{}: {error}", dir.display()))?;
        let mode = fs::symlink_metadata(dir)
            .map_err(|error| format!("{}: {error}", dir.display()))?
            .mode();
        if mode & 0o077 != 0 {
            return Err(fail("cannot be made private"));
        }
    }
    Ok(())
}

/// Earlier versions wrote opened bodies to `<temp>/anixops-net-bodies` with the umask's permissions (often 0755 and
/// 0644). That folder is emptied and removed when it is this user's; a link or another account's folder is left
/// alone.
#[cfg(unix)]
fn remove_legacy_open_dir() {
    use std::os::unix::fs::MetadataExt;

    let legacy = std::env::temp_dir().join(OPEN_DIR_NAME);
    let Ok(meta) = fs::symlink_metadata(&legacy) else {
        return;
    };
    if meta.file_type().is_dir() && meta.uid() == unix_private::uid() {
        let _ = fs::remove_dir_all(&legacy);
    }
}

#[cfg(unix)]
mod unix_private {
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    /// This process's effective user id.
    pub fn uid() -> u32 {
        // SAFETY: geteuid takes no arguments, cannot fail and touches no memory of ours.
        unsafe { libc::geteuid() }
    }

    /// Whether `dir` is a directory (not a link) owned by this user that no other account can open.
    pub fn is_private_dir(dir: &Path) -> bool {
        std::fs::symlink_metadata(dir).is_ok_and(|meta| {
            meta.file_type().is_dir() && meta.uid() == uid() && meta.mode() & 0o077 == 0
        })
    }
}

/// Removes files older than [`OPENED_BODY_RETENTION`] from `dir`; failures are left for the next time.
fn prune_old_files(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .ok()
            .filter(fs::Metadata::is_file)
            .and_then(|meta| meta.modified().ok())
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > OPENED_BODY_RETENTION);
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Writes the body that [`open_body`] hands on into `dir`, after removing files past the retention.
fn write_body_for_opening(file: &BodyFile, dir: &Path) -> Result<PathBuf, String> {
    let data = decode_base64(&file.data_base64)?;
    prune_old_files(dir);
    save_new_file(dir, &body_file_name(&file.name, &file.content_type), &data)
}

/// Writes the body to a temporary file and opens it with the app the system uses for its type. Refuses types
/// that [`may_open_body`] does not allow, before writing anything.
pub fn open_body(file: &BodyFile) -> Result<PathBuf, String> {
    if !may_open_body(&file.content_type) {
        return Err(format!(
            "a body of type {:?} is saved, not opened",
            file.content_type
        ));
    }
    let dir = open_dir()?;
    let path = write_body_for_opening(file, &dir)?;
    open_with_system(&path)?;
    Ok(path)
}

/// Opens `path` with its default app: Explorer's file association on Windows, `xdg-open` on Linux. No shell is
/// involved, and the path is one this module wrote.
fn open_with_system(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let mut command = std::process::Command::new("explorer.exe");
    #[cfg(all(unix, not(target_os = "android"), not(target_vendor = "apple")))]
    let mut command = std::process::Command::new("xdg-open");
    #[cfg(any(
        windows,
        all(unix, not(target_os = "android"), not(target_vendor = "apple"))
    ))]
    {
        let mut child = command
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| format!("no app could be started to open the file: {error}"))?;
        // Explorer and xdg-open hand the file on and exit; waiting off this thread keeps no zombie behind.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
    #[cfg(not(any(
        windows,
        all(unix, not(target_os = "android"), not(target_vendor = "apple"))
    )))]
    {
        let _ = path;
        Err("this platform opens files itself".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_body_files_by_type() {
        assert_eq!(
            body_file_name("response-body", "image/png"),
            "response-body.png"
        );
        assert_eq!(
            body_file_name("", "application/problem+json; charset=utf-8"),
            "body.json"
        );
        assert_eq!(body_file_name("../x", "application/x-protobuf"), "x.bin");
        assert_eq!(body_file_name("b", "Text/HTML"), "b.html");
    }

    #[test]
    fn opens_only_types_that_run_no_code() {
        for open in [
            "image/png",
            "image/jpeg",
            "application/pdf",
            "text/plain; charset=utf-8",
            "application/json",
            "audio/mpeg",
        ] {
            assert!(may_open_body(open), "{open}");
        }
        for saved in [
            "image/svg+xml",
            "text/html",
            "text/javascript",
            "application/xml",
            "text/csv",
            "application/octet-stream",
            "",
            "application/x-msdownload",
        ] {
            assert!(!may_open_body(saved), "{saved}");
        }
        let refused = open_body(&BodyFile {
            data_base64: encode_base64(b"<svg onload=alert(1)>"),
            content_type: "image/svg+xml".to_string(),
            name: "x".to_string(),
        });
        assert!(refused.is_err());
    }

    #[test]
    fn saves_bodies_without_replacing() {
        let dir = std::env::temp_dir().join(format!("body-save-{}", std::process::id()));
        let file = BodyFile {
            data_base64: encode_base64(&[0x89, b'P', b'N', b'G']),
            content_type: "image/png".to_string(),
            name: "response-body".to_string(),
        };
        let first = save_body(&file, &dir).unwrap();
        let second = save_body(&file, &dir).unwrap();
        assert_eq!(first.file_name().unwrap(), "response-body.png");
        assert_eq!(second.file_name().unwrap(), "response-body (2).png");
        assert_eq!(fs::read(&first).unwrap(), [0x89, b'P', b'N', b'G']);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::MetadataExt;
        fs::symlink_metadata(path).unwrap().mode() & 0o777
    }

    #[cfg(unix)]
    fn png_body() -> BodyFile {
        BodyFile {
            data_base64: encode_base64(b"synthetic secret body"),
            content_type: "image/png".to_string(),
            name: "response-body".to_string(),
        }
    }

    /// Audit 2026-11-25 NET-01: with the common 022 umask the folder must still be 0700 and the body 0600 the moment
    /// they exist.
    #[cfg(unix)]
    #[test]
    fn opened_bodies_are_private_whatever_the_umask() {
        let base = std::env::temp_dir().join(format!("body-open-private-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        // SAFETY: umask only swaps the process mask; 022 is the usual default, so other tests see no change.
        let previous = unsafe { libc::umask(0o022) };
        let dir = base.join("bodies");
        ensure_private_dir(&dir).unwrap();
        let path = write_body_for_opening(&png_body(), &dir).unwrap();
        unsafe { libc::umask(previous) };
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&path), 0o600);
        assert_eq!(fs::read(&path).unwrap(), b"synthetic secret body");
        fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn saved_files_are_private() {
        let dir = std::env::temp_dir().join(format!("body-save-private-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let previous = unsafe { libc::umask(0o022) };
        let path = save_new_file(&dir, "a.har", b"{}").unwrap();
        unsafe { libc::umask(previous) };
        assert_eq!(mode(&path), 0o600);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_open_folder_is_tightened() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("body-open-tighten-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let dir = base.join("bodies");
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        ensure_private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
        fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_open_folder_that_is_a_link_or_a_file_is_refused() {
        let base = std::env::temp_dir().join(format!("body-open-link-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let target = base.join("elsewhere");
        fs::create_dir_all(&target).unwrap();
        let link = base.join("bodies");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let error = ensure_private_dir(&link).unwrap_err();
        assert!(error.contains("is not a folder"), "{error}");
        let file = base.join("plain");
        fs::write(&file, b"").unwrap();
        assert!(ensure_private_dir(&file).is_err());
        fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_open_folder_is_private_and_this_users() {
        let dir = open_dir().unwrap();
        assert_eq!(mode(&dir), 0o700);
        assert!(unix_private::is_private_dir(&dir));
        assert!(dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(OPEN_DIR_NAME));
    }

    /// Audit 2026-11-25 NET-01 acceptance with two local accounts: CI (rust-linux) creates a second account and runs
    /// this with `ANIXOPS_OTHER_USER` set; it reads the files through `sudo -n -u`.
    #[cfg(unix)]
    #[test]
    #[ignore = "needs a second local account and passwordless sudo (CI rust-linux)"]
    fn another_account_cannot_read_opened_bodies() {
        use std::process::Command;

        let other = std::env::var("ANIXOPS_OTHER_USER").expect("ANIXOPS_OTHER_USER");
        let as_other = |args: &[&str]| {
            Command::new("sudo")
                .args(["-n", "-u", other.as_str(), "--"])
                .args(args)
                .output()
                .unwrap()
        };
        let base = std::env::temp_dir().join(format!("body-two-accounts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            // The base stays traversable, as /tmp is, so only the folder and file modes protect the body.
            fs::set_permissions(&base, fs::Permissions::from_mode(0o755)).unwrap();
        }
        // Control: the other account can read an ordinary 0644 file here, so a refusal below is the modes' doing.
        let control = base.join("control.txt");
        fs::write(&control, b"public").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&control, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let read = as_other(&["cat", control.to_str().unwrap()]);
        assert!(read.status.success(), "control read failed: {read:?}");

        let previous = unsafe { libc::umask(0o022) };
        let dir = base.join("bodies");
        ensure_private_dir(&dir).unwrap();
        let path = write_body_for_opening(&png_body(), &dir).unwrap();
        unsafe { libc::umask(previous) };
        let read = as_other(&["cat", path.to_str().unwrap()]);
        assert!(!read.status.success(), "another account read the body");
        assert!(!String::from_utf8_lossy(&read.stdout).contains("synthetic"));
        let list = as_other(&["ls", dir.to_str().unwrap()]);
        assert!(!list.status.success(), "another account listed the folder");
        // And the folder the client really uses.
        let real = open_dir().unwrap();
        let list = as_other(&["ls", real.to_str().unwrap()]);
        assert!(
            !list.status.success(),
            "another account listed {}",
            real.display()
        );
        fs::remove_dir_all(&base).unwrap();
    }

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
    fn encodes_base64_that_decodes_back() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(decode_base64(&encode_base64(&bytes)).unwrap(), bytes);
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
    fn builds_export_requests() {
        assert_eq!(
            export_request("har", None, ExportOptions::default()),
            serde_json::json!({ "cmd": "export_capture", "format": "har" })
        );
        let one = ExportOptions {
            exchange: Some(2),
            language: Some("go".to_string()),
            redact: Some(false),
        };
        assert_eq!(
            export_request("code", Some(7), one.clone()),
            serde_json::json!({
                "cmd": "export_capture", "format": "code", "id": 7, "exchange": 2,
                "language": "go", "redact": false,
            })
        );
        // An exchange means nothing without its connection.
        assert_eq!(
            export_request("curl", None, one),
            serde_json::json!({
                "cmd": "export_capture", "format": "curl", "language": "go", "redact": false,
            })
        );
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
    #[test]
    fn decodes_bodies_with_the_kernel() {
        // "hello" gzipped (RFC 1952 member, no name).
        let gzip = [
            0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0xcb, 0x48, 0xcd, 0xc9,
            0xc9, 0x07, 0x00, 0x86, 0xa6, 0x10, 0x36, 0x05, 0x00, 0x00, 0x00,
        ];
        let request = |coding: &str, data: &[u8], complete: bool| DecodeRequest {
            data_base64: encode_base64(data),
            content_encoding: coding.to_string(),
            complete,
        };
        let answer = decode_body(&request("gzip", &gzip, true)).unwrap();
        assert_eq!(
            decode_base64(answer["data_base64"].as_str().unwrap()).unwrap(),
            b"hello"
        );
        assert_eq!(answer["truncated"], false);
        assert_eq!(answer["codings"], serde_json::json!(["gzip"]));
        // Kept only in part: what came out so far, marked truncated.
        let cut = decode_body(&request("gzip", &gzip[..16], false)).unwrap();
        assert_eq!(cut["truncated"], true);
        assert!(decode_body(&request("compress", b"x", true)).is_err());
        assert!(decode_body(&request("gzip", b"not gzip", true)).is_err());
    }
}
