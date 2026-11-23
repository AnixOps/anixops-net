// A window program: Windows starts it for every `anixops://` link, and a console would flash up each time.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    // Anything but one link is not for this program: it does nothing rather than start the client.
    let Some(link) = anixops_net_link::link_from_args(std::env::args_os()) else {
        std::process::exit(2);
    };
    std::process::exit(anixops_net_link::os::forward(&link));
}

#[cfg(not(windows))]
fn main() {
    eprintln!("anixops-net-link is the anixops:// handler of the Windows client");
    std::process::exit(1);
}
