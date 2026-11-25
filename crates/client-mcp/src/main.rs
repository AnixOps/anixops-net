//! `anixops-mcp`: the client's MCP server (docs/mcp-desktop.md). An AI tool on this computer starts it and speaks MCP
//! over standard input and output; the program asks the running app over the app's own local channel.
//!
//! On macOS it ships inside the app, at `AnixOps Net.app/Contents/MacOS/anixops-mcp`, and finds the app's channel
//! through the app's Info.plist. Windows and Linux follow in a later step (docs/mcp-desktop.md §10).

use std::process::ExitCode;

const HELP: &str = "anixops-mcp: lets an AI tool on this computer read the state of AnixOps Net (MCP over standard input and output).

Usage: anixops-mcp [--socket PATH]
       anixops-mcp --version | --help

The AI tool starts this program; it is not meant to be run by hand. Access is off until it is turned on in
AnixOps Net, Settings, Advanced, External tool access.

  --socket PATH  the app's command-line socket, instead of the one found through the app bundle";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut socket = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{HELP}");
                return ExitCode::SUCCESS;
            }
            "--version" => {
                println!("anixops-mcp {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--socket" => match rest.next() {
                Some(path) => socket = Some(path.clone()),
                None => return usage("--socket needs a path"),
            },
            other => return usage(&format!("unknown argument {other}")),
        }
    }
    run(socket)
}

fn usage(problem: &str) -> ExitCode {
    eprintln!("anixops-mcp: {problem}\n\n{HELP}");
    ExitCode::from(64)
}

#[cfg(unix)]
fn run(socket: Option<String>) -> ExitCode {
    let path = match socket
        .map(std::path::PathBuf::from)
        .map_or_else(app_socket, Ok)
    {
        Ok(path) => path,
        Err(problem) => {
            eprintln!("anixops-mcp: {problem}");
            return ExitCode::from(69);
        }
    };
    let server = client_mcp::Server::new(client_mcp::control_channel::ControlChannel::new(path));
    match server.run(std::io::stdin().lock(), std::io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("anixops-mcp: {error}");
            ExitCode::from(74)
        }
    }
}

#[cfg(not(unix))]
fn run(_socket: Option<String>) -> ExitCode {
    eprintln!("anixops-mcp: not available on this system yet");
    ExitCode::from(69)
}

/// The Mac app's socket: `~/Library/Group Containers/<group>/cli.sock`, the group read from the Info.plist of the
/// app this program sits in (`Contents/MacOS/anixops-mcp`).
#[cfg(target_os = "macos")]
fn app_socket() -> Result<std::path::PathBuf, String> {
    use std::path::{Path, PathBuf};
    let program = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|error| format!("cannot find this program's path: {error}"))?;
    let contents = program
        .parent()
        .and_then(Path::parent)
        .ok_or("this program is not inside the app")?;
    let output = std::process::Command::new("/usr/bin/plutil")
        .args(["-extract", "AnixOpsAppGroup", "raw", "-o", "-"])
        .arg(contents.join("Info.plist"))
        .output()
        .map_err(|error| format!("cannot read the app's Info.plist: {error}"))?;
    let group = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || group.is_empty() {
        return Err("this program is not inside AnixOps Net.app; pass --socket".to_string());
    }
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join("Library/Group Containers")
        .join(group)
        .join("cli.sock"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn app_socket() -> Result<std::path::PathBuf, String> {
    Err("not available on this system yet; pass --socket for a test".to_string())
}
