//! `anixops-net-service`: see the library's documentation.

#[cfg(not(windows))]
fn main() {
    eprintln!("anixops-net-service runs on Windows only");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    use anixops_net_service::{scm, Options, Service};

    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    if !options.console {
        if let Err(error) = scm::dispatch(options) {
            eprintln!(
                "anixops-net-service was not started by the service manager ({error}); \
                 use --console to run it in the foreground"
            );
            std::process::exit(1);
        }
        return;
    }
    let service = match Service::start(&options) {
        Ok(service) => service,
        Err(error) => {
            eprintln!("anixops-net-service could not start: {error}");
            std::process::exit(1);
        }
    };
    // In the foreground the service runs until the console is closed or Ctrl+C ends the process.
    if let Err(error) = service.run() {
        eprintln!("anixops-net-service stopped listening: {error}");
        service.shutdown();
        std::process::exit(1);
    }
}
