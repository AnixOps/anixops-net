//! The `anixops://` handler on Windows.
//!
//! The client (`networkcore-windows-gui.exe`) requires administrator rights, so registering it as the handler made
//! every link a new elevated process: an administrator prompt each time, even with the client already running, and
//! a link lost without a word when the prompt was declined. Windows registers this small program instead, which
//! runs with the rights of whoever opened the link:
//!
//! - when the client runs, the link goes to it over a window message ([`os::hand_over`]) and no prompt appears;
//! - when it does not, the client is started with the link (one prompt, [`os::start_client`]);
//! - when that prompt is declined or the start fails, the link is kept ([`keep_pending`]) and the client takes it
//!   the next time it starts within the hour ([`take_pending`]); the user is told either way.
//!
//! The client listens with a message-only window of [`RECEIVER_CLASS`] that lets this one message in from a
//! process with fewer rights ([`os::listen`]). A second start of the client itself hands over the same way before
//! it opens anything ([`Request::Show`] or the link it was started with).
//!
//! What crosses over is checked on both sides with [`accept`]: an `anixops:` link no longer than the window
//! takes, without control characters. The links can only do what a web page can already ask for (connect,
//! disconnect, the routing mode, a node, filling in the import form), so letting a process with fewer rights
//! hand them over gives it nothing it could not get by opening the link itself.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub use client_strings::Language;

/// The scheme, with its colon, as links start.
pub const SCHEME: &str = "anixops:";
/// The longest link taken, in UTF-16 units as the window counts them: the window refuses longer ones too
/// (`maxImportLength` + 256 in `ui/src/app-link.ts`).
pub const MAX_LINK_LENGTH: usize = 8 * 1024 + 256;
/// The class (and name) of the client's message-only window that receives links.
pub const RECEIVER_CLASS: &str = "AnixOpsNet.LinkReceiver";
/// `COPYDATASTRUCT::dwData` of a message carrying a link (UTF-8, no terminator).
pub const OPEN_TAG: usize = 0x414E_584C;
/// `COPYDATASTRUCT::dwData` of a message that only asks the client to show its window.
pub const SHOW_TAG: usize = 0x414E_5853;
/// The client, next to this program.
pub const CLIENT_EXECUTABLE: &str = "networkcore-windows-gui.exe";
/// This program, next to the client.
pub const LINK_EXECUTABLE: &str = "anixops-net-link.exe";
/// The folder under `%LOCALAPPDATA%` that holds the kept links.
pub const PENDING_FOLDER: &str = "AnixOps Net";
/// The file of kept links in [`PENDING_FOLDER`].
pub const PENDING_FILE: &str = "pending-links.txt";
/// Kept links the client takes at most; more are dropped.
pub const MAX_PENDING: usize = 16;
/// How long a kept link stays good, in seconds. A link is something the user just asked for: an hour later
/// `anixops://toggle` would surprise them.
pub const PENDING_LIFETIME: u64 = 60 * 60;

/// What one message asks of the running client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Bring the window forward (the client was started again without a link).
    Show,
    /// Follow this link (checked by [`accept`]).
    Open(String),
}

/// The link as it is taken over, or `None` when it is not one: an `anixops:` link (any case) no longer than
/// [`MAX_LINK_LENGTH`] and without control characters, so it is one line and one argument. A raw `"` (which a
/// URL may not hold) is written as `%22` so that the link stays one quoted argument.
pub fn accept(link: &str) -> Option<String> {
    let scheme = link.get(..SCHEME.len())?;
    if !scheme.eq_ignore_ascii_case(SCHEME) {
        return None;
    }
    if link.chars().any(char::is_control) {
        return None;
    }
    let link = link.replace('"', "%22");
    (link.encode_utf16().count() <= MAX_LINK_LENGTH).then_some(link)
}

/// The link a program was started with: its only argument, when that is a link [`accept`] takes. Windows starts
/// the handler as `"<program>" "<link>"`; anything else is not a link to hand over.
pub fn link_from_args<I: IntoIterator<Item = OsString>>(args: I) -> Option<String> {
    let mut args = args.into_iter().skip(1);
    let link = args.next()?.into_string().ok()?;
    if args.next().is_some() {
        return None;
    }
    accept(&link)
}

/// What a second start of the client asks of the running one: the link it was started with, else to show the
/// window.
pub fn request_from_args<I: IntoIterator<Item = OsString>>(args: I) -> Request {
    link_from_args(args).map_or(Request::Show, Request::Open)
}

/// One argument quoted for a Windows command line, as `CommandLineToArgvW` reads it back: inside quotes, the
/// backslashes before a quote or the end are doubled and a quote is escaped.
pub fn quote_argument(argument: &str) -> String {
    let mut quoted = String::with_capacity(argument.len() + 2);
    quoted.push('"');
    let mut backslashes = 0;
    for character in argument.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            other => {
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                quoted.push(other);
                backslashes = 0;
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

/// The `shell\open\command` of the `anixops` URL class: this program with the link as its one argument.
pub fn handler_command(link_program: &Path) -> String {
    format!("\"{}\" \"%1\"", link_program.display())
}

/// The message for a request: its `dwData` and its bytes.
pub fn payload(request: &Request) -> (usize, Vec<u8>) {
    match request {
        Request::Show => (SHOW_TAG, Vec::new()),
        Request::Open(link) => (OPEN_TAG, link.as_bytes().to_vec()),
    }
}

/// The request a message carries, or `None` for one that is not ours or not a link [`accept`] takes.
pub fn read_payload(tag: usize, bytes: &[u8]) -> Option<Request> {
    match tag {
        SHOW_TAG if bytes.is_empty() => Some(Request::Show),
        // Each UTF-16 unit is at most three bytes of UTF-8; anything longer cannot be a link that is taken.
        OPEN_TAG if bytes.len() <= MAX_LINK_LENGTH * 3 => std::str::from_utf8(bytes)
            .ok()
            .and_then(accept)
            .map(Request::Open),
        _ => None,
    }
}

/// The file of kept links under `local_app_data` (`%LOCALAPPDATA%`).
pub fn pending_path(local_app_data: &Path) -> PathBuf {
    local_app_data.join(PENDING_FOLDER).join(PENDING_FILE)
}

/// The most the file of kept links is read or grown to.
const PENDING_BYTES: u64 = (MAX_PENDING * (MAX_LINK_LENGTH * 3 + 24)) as u64;

/// Keeps `link` (taken by [`accept`]) for the client's next start, as `<seconds since 1970>\t<link>` on a line of
/// its own. A file already at [`MAX_PENDING`] links or its size limit is left as it is.
pub fn keep_pending(path: &Path, link: &str, now: u64) -> io::Result<()> {
    let link = accept(link).ok_or_else(|| io::Error::other("not an anixops: link"))?;
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    if let Ok(existing) = fs::read_to_string(path) {
        if existing.lines().count() >= MAX_PENDING || existing.len() as u64 >= PENDING_BYTES {
            return Err(io::Error::other("too many links are kept already"));
        }
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{now}\t{link}")
}

/// Takes the kept links that are still good at `now`, oldest first, and removes the file. Lines that are not a
/// kept link, links older than [`PENDING_LIFETIME`] or from the future, and anything past [`MAX_PENDING`] are
/// dropped. The client reads this file with administrator rights while the user can write it, so only a plain
/// file is read, and no more than its size limit.
pub fn take_pending(path: &Path, now: u64) -> Vec<String> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Vec::new();
    };
    if !metadata.file_type().is_file() {
        return Vec::new();
    }
    let mut text = String::new();
    let read =
        fs::File::open(path).and_then(|file| file.take(PENDING_BYTES).read_to_string(&mut text));
    let _ = fs::remove_file(path);
    if read.is_err() {
        return Vec::new();
    }
    text.lines()
        .filter_map(|line| {
            let (kept_at, link) = line.split_once('\t')?;
            let kept_at: u64 = kept_at.parse().ok()?;
            if kept_at > now || now - kept_at > PENDING_LIFETIME {
                return None;
            }
            accept(link)
        })
        .take(MAX_PENDING)
        .collect()
}

/// `ERROR_CANCELLED`: the user declined the administrator prompt.
pub const DECLINED: u32 = 1223;

/// What the user is told when the client could not be started for a link: why (the prompt was declined, or the
/// Windows error), then whether the link was kept for the next start.
pub fn start_failure_message(language: Language, error: u32, kept: bool) -> String {
    let code = error.to_string();
    let why = if error == DECLINED {
        client_strings::text(language, "link.declined").to_string()
    } else {
        client_strings::format(language, "link.failed", &[code.as_str()])
    };
    let then = client_strings::text(language, if kept { "link.kept" } else { "link.lost" });
    format!("{why}\n{then}")
}

#[cfg(windows)]
pub mod os;

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("anixops-net-link-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn only_anixops_links_are_taken() {
        assert_eq!(
            accept("anixops://connect").as_deref(),
            Some("anixops://connect")
        );
        assert_eq!(
            accept("ANIXOPS:///toggle").as_deref(),
            Some("ANIXOPS:///toggle")
        );
        assert_eq!(accept("https://a.example"), None);
        assert_eq!(accept("anixops"), None);
        assert_eq!(accept(""), None);
        // Not one line, or not one argument.
        assert_eq!(accept("anixops://connect\nanixops://toggle"), None);
        assert_eq!(accept("anixops://connect\0"), None);
        assert_eq!(
            accept("anixops://select?node=\"HK\"").as_deref(),
            Some("anixops://select?node=%22HK%22")
        );
        // Text whose first bytes end inside a character is refused, not cut.
        assert_eq!(accept("香港"), None);
        assert_eq!(accept("anixop香"), None);
    }

    #[test]
    fn links_longer_than_the_window_takes_are_refused() {
        let head = "anixops://import?url=";
        let fits = format!("{head}{}", "a".repeat(MAX_LINK_LENGTH - head.len()));
        assert_eq!(accept(&fits).as_deref(), Some(fits.as_str()));
        assert_eq!(accept(&format!("{fits}a")), None);
    }

    #[test]
    fn the_link_is_the_only_argument() {
        assert_eq!(
            link_from_args(args(&["anixops-net-link.exe", "anixops://toggle"])).as_deref(),
            Some("anixops://toggle")
        );
        assert_eq!(link_from_args(args(&["anixops-net-link.exe"])), None);
        assert_eq!(
            link_from_args(args(&["a.exe", "anixops://toggle", "--debug"])),
            None
        );
        assert_eq!(
            link_from_args(args(&["a.exe", "--start-after-login"])),
            None
        );
        assert_eq!(
            request_from_args(args(&["networkcore-windows-gui.exe"])),
            Request::Show
        );
        assert_eq!(
            request_from_args(args(&["networkcore-windows-gui.exe", "anixops://connect"])),
            Request::Open("anixops://connect".to_string())
        );
    }

    #[test]
    fn arguments_are_quoted_as_windows_reads_them() {
        assert_eq!(quote_argument("anixops://toggle"), "\"anixops://toggle\"");
        assert_eq!(quote_argument("a b"), "\"a b\"");
        assert_eq!(quote_argument(r"a\b"), r#""a\b""#);
        assert_eq!(quote_argument(r"a\"), r#""a\\""#);
        assert_eq!(quote_argument(r#"a"b"#), r#""a\"b""#);
        assert_eq!(quote_argument(r#"a\"b"#), r#""a\\\"b""#);
        assert_eq!(quote_argument(""), "\"\"");
    }

    #[test]
    fn the_handler_command_passes_the_link_as_one_argument() {
        assert_eq!(
            handler_command(Path::new(
                r"C:\Program Files\AnixOps\NetworkCore\anixops-net-link.exe"
            )),
            r#""C:\Program Files\AnixOps\NetworkCore\anixops-net-link.exe" "%1""#
        );
    }

    #[test]
    fn messages_carry_only_what_is_taken() {
        let open = Request::Open("anixops://select?node=%E9%A6%99%E6%B8%AF".to_string());
        for request in [Request::Show, open] {
            let (tag, bytes) = payload(&request);
            assert_eq!(read_payload(tag, &bytes), Some(request));
        }
        assert_eq!(read_payload(OPEN_TAG, b"https://a.example"), None);
        assert_eq!(read_payload(OPEN_TAG, &[0xff, 0xfe]), None);
        assert_eq!(read_payload(SHOW_TAG, b"anixops://connect"), None);
        // The single instance plugin's own messages and anything else are not ours.
        assert_eq!(read_payload(1542, b"anixops://connect"), None);
        let long = format!("anixops://import?url={}", "a".repeat(MAX_LINK_LENGTH * 3));
        assert_eq!(read_payload(OPEN_TAG, long.as_bytes()), None);
    }

    #[test]
    fn the_user_is_told_why_and_whether_the_link_waits() {
        assert_eq!(
            start_failure_message(Language::English, DECLINED, true),
            "AnixOps Net did not start because administrator rights were not granted.\nThe link is kept and opens if you start AnixOps Net within the hour."
        );
        assert_eq!(
            start_failure_message(Language::Chinese, 2, false),
            "AnixOps Net 无法启动（错误 2）。\n链接没有打开。"
        );
        for language in Language::ALL {
            for kept in [true, false] {
                let message = start_failure_message(language, 5, kept);
                assert!(message.contains('5'), "{message}");
                assert!(!message.contains("{0}"), "{message}");
            }
        }
    }

    #[test]
    fn a_kept_link_is_taken_once_within_the_hour() {
        let dir = temp_dir("kept");
        let path = pending_path(&dir);
        keep_pending(&path, "anixops://import?url=https%3A%2F%2Fa.example", 1_000).unwrap();
        keep_pending(&path, "anixops://toggle", 1_010).unwrap();
        assert!(keep_pending(&path, "https://a.example", 1_020).is_err());
        assert_eq!(
            take_pending(&path, 1_000 + PENDING_LIFETIME),
            vec![
                "anixops://import?url=https%3A%2F%2Fa.example".to_string(),
                "anixops://toggle".to_string()
            ]
        );
        // Taken means gone: the next start finds nothing.
        assert!(!path.exists());
        assert!(take_pending(&path, 1_100).is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn old_future_and_foreign_lines_are_dropped() {
        let dir = temp_dir("stale");
        let path = pending_path(&dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "100\tanixops://connect\n\
             5000\tanixops://toggle\n\
             9999999\tanixops://disconnect\n\
             garbage\n\
             4000\thttps://a.example\n\
             4500\tanixops://mode?set=rule\n",
        )
        .unwrap();
        assert_eq!(
            take_pending(&path, 5_000),
            vec![
                "anixops://toggle".to_string(),
                "anixops://mode?set=rule".to_string()
            ]
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_number_of_kept_links_is_bounded() {
        let dir = temp_dir("bounded");
        let path = pending_path(&dir);
        for _ in 0..MAX_PENDING {
            keep_pending(&path, "anixops://toggle", 10).unwrap();
        }
        assert!(keep_pending(&path, "anixops://toggle", 10).is_err());
        assert_eq!(take_pending(&path, 10).len(), MAX_PENDING);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_folder_in_place_of_the_file_is_not_read() {
        let dir = temp_dir("folder");
        let path = pending_path(&dir);
        fs::create_dir_all(&path).unwrap();
        assert!(take_pending(&path, 10).is_empty());
        assert!(path.is_dir());
        let _ = fs::remove_dir_all(dir);
    }
}
