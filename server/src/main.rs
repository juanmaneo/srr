use std::net::{IpAddr, Ipv4Addr, TcpListener, UdpSocket};
use std::path::Path;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use srr::game::StyleSet;
use srr::net::MAX_PLAYERS;
use srr_server::{Command, Limits, TICK, parse_args, serve_with_styles, usage};

/// How long to wait, after the server has stopped, for the writer threads to flush the close
/// frames to every browser.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(100);

fn main() -> ExitCode {
    let (port, root, styles) = match parse_args(std::env::args().skip(1)) {
        Ok(Command::Serve { port, root, styles }) => (port, root, styles),
        Ok(Command::Help) => {
            print!("{}", usage());
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("error: {error}\n\n{}", usage());
            return ExitCode::from(2);
        }
    };
    if !root.join("index.html").is_file() {
        eprintln!(
            "warning: {} has no index.html; build it with `cargo xtask dist`",
            root.display()
        );
    }
    let listener = match TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("error: cannot listen on port {port}: {error}");
            return ExitCode::FAILURE;
        }
    };
    print_addresses(port, &root, styles);
    let server = serve_with_styles(listener, root, TICK, Limits::default(), styles);
    // Ctrl+C: stop accepting, tell every browser, and let the server wind down.
    let stop = server.stopper();
    if ctrlc::set_handler(move || stop.stop()).is_err() {
        eprintln!("warning: cannot watch Ctrl+C: kill the process to stop the server");
    }
    server.join();
    thread::sleep(SHUTDOWN_GRACE);
    ExitCode::SUCCESS
}

fn print_addresses(port: u16, root: &Path, styles: StyleSet) {
    println!("Shattered Realm: Rampage - serving {}", root.display());
    println!("  this machine:  http://localhost:{port}");
    match lan_address() {
        Some(ip) => println!("  local network: http://{ip}:{port}   <- open this on every machine"),
        None => println!("  local network: http://<this machine's IP>:{port}"),
    }
    println!(
        "Up to {MAX_PLAYERS} browsers sit in the room; a match starts when everybody pressed Enter. Ctrl+C stops the server."
    );
    println!("Skins allowed: {styles} (the chalk stick figure always is).");
}

/// The address other machines reach this one at. Connecting a UDP socket sends nothing; it only
/// makes the OS pick the interface of the default route.
fn lan_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}
