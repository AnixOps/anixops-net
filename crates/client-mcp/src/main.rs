//! `anixops-mcp`: the client's MCP server (docs/mcp-desktop.md). An AI tool on this computer starts it and speaks MCP
//! over standard input and output; the program asks the running app over the app's own local channel.
//!
//! On macOS it ships inside the app, at `AnixOps Net.app/Contents/MacOS/anixops-mcp`, and finds the app's channel
//! through the app's Info.plist. On Windows and Linux it ships next to the window and talks to the background
//! core over the channel the window uses (`client-ipc`), as an AI tool ([`client_ipc::Role::Agent`]).

use std::process::ExitCode;

const HELP: &str = "anixops-mcp: lets an AI tool on this computer read the state of AnixOps Net (MCP over standard input and output).

Usage: anixops-mcp [--socket PATH]
       anixops-mcp --version | --help

The AI tool starts this program; it is not meant to be run by hand. Access is off until it is turned on in
AnixOps Net, Settings, Advanced, External tool access.

  --socket PATH  the app's channel instead of the installed one: the command-line socket on macOS, the core's
                 socket on Linux or named pipe on Windows";

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

/// Serves the AI tool over standard input and output until it closes them.
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
fn serve(backend: impl client_mcp::Backend) -> ExitCode {
    let server = client_mcp::Server::new(backend);
    match server.run(std::io::stdin().lock(), std::io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("anixops-mcp: {error}");
            ExitCode::from(74)
        }
    }
}

#[cfg(target_os = "macos")]
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
    serve(client_mcp::control_channel::ControlChannel::new(path))
}

/// Windows and Linux: the core's channel, the named pipe or the daemon's socket the window uses too.
#[cfg(any(target_os = "linux", windows))]
fn run(socket: Option<String>) -> ExitCode {
    let endpoint = socket.unwrap_or_else(|| client_ipc::transport::DEFAULT_ENDPOINT.to_string());
    serve(client_mcp::agent::IpcChannel::new(
        endpoint,
        env!("CARGO_PKG_VERSION"),
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn run(_socket: Option<String>) -> ExitCode {
    eprintln!("anixops-mcp: not available on this system");
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
