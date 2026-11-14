//! The app's own log of the desktop and Android clients, for the log viewer (iOS shows the extension's log the same
//! way). Each file is one module; a line is `[unix_ms=…] message`, optionally `[unix_ms=…] [warn] message`, the
//! format the Windows client already writes. The engine keeps no log, so only what the app records is shown.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// A file grows to this size before it is moved to `<name>.1` and started again.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Bytes read from the end of each file.
const TAIL_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

/// One line of a log, in the interface's `LogEntry` shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Unix milliseconds; 0 when the line carries no time.
    pub time_ms: u64,
    pub module: String,
    pub level: Level,
    pub message: String,
}

/// Appends one line to `path`, moving a full file aside first.
pub fn append(path: &Path, level: Level, message: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::metadata(path)
        .map(|meta| meta.len() >= MAX_FILE_BYTES)
        .unwrap_or(false)
    {
        fs::rename(path, rotated(path))?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let message = redact(&message.replace(['\r', '\n'], " "));
    match level {
        Level::Info => writeln!(file, "[unix_ms={}] {message}", now_ms()),
        _ => writeln!(file, "[unix_ms={}] [{}] {message}", now_ms(), level.tag()),
    }
}

/// The last `limit` lines of the given `(module, file)` logs, oldest first.
pub fn read(files: &[(&str, PathBuf)], limit: usize) -> Vec<Entry> {
    let mut entries = Vec::new();
    for (module, path) in files {
        for line in tail(path).lines() {
            if let Some(entry) = parse(module, line) {
                entries.push(entry);
            }
        }
    }
    entries.sort_by_key(|entry| entry.time_ms);
    let start = entries.len().saturating_sub(limit);
    entries.split_off(start)
}

/// Empties the given logs and their moved-aside parts.
pub fn clear(files: &[(&str, PathBuf)]) -> io::Result<()> {
    for (_, path) in files {
        for path in [path.clone(), rotated(path)] {
            match fs::remove_file(&path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
    }
    Ok(())
}

/// One line; a line without a level is an error when it says it failed.
pub fn parse(module: &str, line: &str) -> Option<Entry> {
    let line = line.trim_end();
    if line.is_empty() {
        return None;
    }
    let (time_ms, rest) = match line
        .strip_prefix("[unix_ms=")
        .and_then(|rest| rest.split_once(']'))
        .and_then(|(time, rest)| Some((time.parse::<u64>().ok()?, rest.trim_start())))
    {
        Some(parsed) => parsed,
        None => (0, line),
    };
    let (level, message) = match rest.strip_prefix('[').and_then(|rest| rest.split_once(']')) {
        Some(("info", message)) => (Level::Info, message.trim_start()),
        Some(("warn", message)) => (Level::Warn, message.trim_start()),
        Some(("error", message)) => (Level::Error, message.trim_start()),
        _ => (guess(rest), rest),
    };
    Some(Entry {
        time_ms,
        module: module.to_string(),
        level,
        message: redact(message),
    })
}

/// Drops the query of every URL in `text`: subscription links carry their token there.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(scheme) = rest.find("://") {
        let end = rest[scheme..]
            .find(|c: char| c.is_whitespace() || matches!(c, ')' | '"' | '\'' | '>'))
            .map_or(rest.len(), |end| scheme + end);
        let start = rest[..scheme]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
            .map_or(0, |index| index + 1);
        out.push_str(&rest[..start]);
        let url = &rest[start..end];
        match url.find('?') {
            Some(query) => {
                out.push_str(&url[..query]);
                out.push_str("?…");
            }
            None => out.push_str(url),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn guess(message: &str) -> Level {
    let lower = message.to_ascii_lowercase();
    if ["fatal", "panic", "failed", "error", "could not"]
        .iter()
        .any(|word| lower.contains(word))
    {
        Level::Error
    } else {
        Level::Info
    }
}

/// The end of the moved-aside part and the file, whole lines only.
fn tail(path: &Path) -> String {
    let mut text = String::new();
    let current = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    if current < TAIL_BYTES {
        text.push_str(&read_end(&rotated(path), TAIL_BYTES - current));
    }
    text.push_str(&read_end(path, TAIL_BYTES));
    text
}

fn read_end(path: &Path, bytes: u64) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let length = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let start = length.saturating_sub(bytes);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut buffer = Vec::new();
    if file.read_to_end(&mut buffer).is_err() {
        return String::new();
    }
    let mut text = String::from_utf8_lossy(&buffer).into_owned();
    if start > 0 {
        // The first line was cut.
        text = text
            .split_once('\n')
            .map(|(_, rest)| rest.to_string())
            .unwrap_or_default();
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn rotated(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".1");
    PathBuf::from(name)
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_windows_lines_and_levels() {
        let entry = parse("gui", "[unix_ms=12] automatic connection completed").unwrap();
        assert_eq!((entry.time_ms, entry.level), (12, Level::Info));
        assert_eq!(entry.message, "automatic connection completed");
        assert_eq!(
            parse("gui", "[unix_ms=13] connect failed: x")
                .unwrap()
                .level,
            Level::Error
        );
        let warn = parse("app", "[unix_ms=14] [warn] slow").unwrap();
        assert_eq!((warn.level, warn.message.as_str()), (Level::Warn, "slow"));
        assert_eq!(parse("app", "plain").unwrap().time_ms, 0);
        assert!(parse("app", "  ").is_none());
    }

    #[test]
    fn drops_url_queries() {
        assert_eq!(
            redact("error sending request for url (https://a.example/sub?token=x&b=1): timed out"),
            "error sending request for url (https://a.example/sub?…): timed out"
        );
        assert_eq!(redact("why? no links here"), "why? no links here");
        assert_eq!(redact("what? see http://a/b?c"), "what? see http://a/b?…");
        assert_eq!(redact("vless://id@h:1?type=ws#n"), "vless://id@h:1?…");
    }

    #[test]
    fn appends_reads_newest_last_and_clears() {
        let dir = std::env::temp_dir().join(format!("app-log-{}", std::process::id()));
        let a = dir.join("a.log");
        let b = dir.join("b.log");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&a, "[unix_ms=1] one\n[unix_ms=3] three\n").unwrap();
        fs::write(&b, "[unix_ms=2] [error] two\n").unwrap();
        append(&b, Level::Warn, "four\nlines").unwrap();
        let files = [("a", a.clone()), ("b", b.clone())];
        let entries = read(&files, 3);
        let messages: Vec<_> = entries.iter().map(|entry| entry.message.as_str()).collect();
        assert_eq!(messages, ["two", "three", "four lines"]);
        assert_eq!(entries[0].module, "b");
        assert_eq!(entries[2].level, Level::Warn);
        clear(&files).unwrap();
        assert!(read(&files, 10).is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_full_file_moves_aside_and_is_still_read() {
        let dir = std::env::temp_dir().join(format!("app-log-rotate-{}", std::process::id()));
        let path = dir.join("app.log");
        fs::create_dir_all(&dir).unwrap();
        let mut big = String::from("[unix_ms=1] old\n");
        while (big.len() as u64) < MAX_FILE_BYTES {
            big.push_str("[unix_ms=2] filler line\n");
        }
        fs::write(&path, big).unwrap();
        append(&path, Level::Info, "new").unwrap();
        assert!(rotated(&path).exists());
        let entries = read(&[("app", path.clone())], 2);
        assert_eq!(entries.last().unwrap().message, "new");
        assert_eq!(entries[0].message, "filler line");
        let _ = fs::remove_dir_all(dir);
    }
}
