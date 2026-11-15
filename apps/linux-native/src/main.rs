//! `anixops-net-preview`: the native Linux window.

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    use anixops_net_native::launch::Launch;

    let arguments: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    if anixops_net_native::window::run(Launch::parse(arguments)) {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}
