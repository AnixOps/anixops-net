//! Linux CI probe. This uses only an isolated temporary directory and loopback connections.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::thread;

use client_core::backend::{engine, mesh};

fn main() {
    let dir = PathBuf::from(std::env::var_os("RUNNER_TEMP").expect("runner temp directory"))
        .join("mesh-candidate-legacy");
    fs::create_dir(&dir).unwrap();
    fs::write(
        dir.join(mesh::FILE),
        r#"[{"name":"office","joined":true,"config":{"config_version":1,"network":{"name":"office","secret":"test-only","kind":"managed","secure_mode":true},"node":{"role":"access"},"peers":[{"server":"127.0.0.1","port":1,"transport":"tcp"}]}}]"#,
    )
    .unwrap();
    let old = mesh::load(&dir).expect("load version 1 configuration");
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].name, "office");
    assert!(old[0].joined);
    let profile = mesh::only_profile(&mesh::outbounds(&old)).expect("legacy mesh profile");
    fs::remove_file(dir.join(mesh::FILE)).unwrap();
    fs::remove_dir(&dir).unwrap();
    println!(
        "Version 1 configuration loaded with its joined state and converted to a mesh profile"
    );

    let mut client = engine::Engine::default();
    let direct = engine::direct_envelope();
    let mut proxy = client
        .start(&direct, "direct", 0)
        .expect("start normal proxy");
    if !mesh::ENABLED {
        let refusal = client.start(&profile.to_string(), "mesh", 0).unwrap_err();
        assert!(mesh::is_gated_refusal(&refusal));
        assert!(!client.is_running());
        proxy = client
            .start(&direct, "direct", 0)
            .expect("restart normal proxy after refusal");
        println!("Closed gate refused legacy mesh and allowed a clean normal proxy restart");
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let target = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&mut socket).read_line(&mut line).unwrap();
        assert!(line.starts_with("GET / HTTP/"));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .unwrap();
    });
    let result = Command::new("curl")
        .args([
            "--noproxy",
            "",
            "--proxy",
            &format!("socks5h://{proxy}"),
            "--max-time",
            "5",
            "--fail",
            "--silent",
            "--show-error",
            &format!("http://{target}/"),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    server.join().unwrap();
    assert_eq!(result.stdout, b"ok");
    client.stop();
    println!(
        "Normal proxy carried an HTTP response over loopback; mesh preview={}",
        mesh::ENABLED
    );
}
