//! `qanuvi-relay`: the relay server (Milestone 20). It passes sealed messages between computers and the devices that
//! reach them, and cannot read them. It runs behind a TLS proxy (nginx or Caddy) for wss://, on its own TLS port, or
//! both at once (24rl R2).
//!
//! qanuvi-relay [--listen 127.0.0.1:47300] [--tls-listen 0.0.0.0:47443 --cert <file> --key <file>]
//!              [--public-direct wss://relay.example.com:47443] [--data <folder>]
//!              [--cap-per-day <MB> [--slow-to <KB/s>]]   (24rl F3: a daily share per computer, slowed after it)
//!              [--tls-self]   (relay kit K1: its own certificate, kept in --data; no domain needed)
//! qanuvi-relay address --host <IP or name> [--port 47443] --data <folder>   (the address to paste into Qanuvi)
//! qanuvi-relay check <address>   (connects as an app would and says OK or why not)

const USAGE: &str = "Usage: qanuvi-relay [--listen 127.0.0.1:47300] \
[--tls-listen 0.0.0.0:47443 --cert <fullchain.pem> --key <key.pem> | --tls-self] \
[--public-direct wss://relay.example.com:47443] [--data <folder>] [--ping-every 30] \
[--cap-per-day <MB> [--slow-to <KB/s, default 256>]]\n\
       qanuvi-relay address --host <IP or name> [--port 47443] --data <folder>\n\
       qanuvi-relay check <address>\n\
With no --listen and no --tls-listen it listens on 127.0.0.1:47300 (for a TLS proxy in front).\n\
--tls-self: the relay makes its own certificate (kept in --data) and apps check it by the pin in its address, \
so a relay needs no domain.";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return;
    }
    match args.get(1).map(String::as_str) {
        Some("address") => return address(value("--host"), value("--port"), value("--data")),
        Some("check") => return check(args.get(2).cloned()),
        _ => {}
    }
    let tls_listen = value("--tls-listen");
    let listen =
        value("--listen").or_else(|| tls_listen.is_none().then(|| "127.0.0.1:47300".to_string()));
    let tls = match (value("--cert"), value("--key")) {
        (Some(cert), Some(key)) => Some(qanuvi_lib::relay_server::TlsFiles {
            cert: cert.into(),
            key: key.into(),
        }),
        _ => None,
    };
    let options = qanuvi_lib::relay_server::Options {
        listen,
        tls_listen,
        tls,
        tls_self: args.iter().any(|a| a == "--tls-self"),
        direct: value("--public-direct"),
        data: value("--data").map(std::path::PathBuf::from),
        ping_every: value("--ping-every")
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .map(std::time::Duration::from_secs),
        // 24rl F3: a daily share per computer (MB of relayed traffic), slowed after it (KB/s); none = no cap.
        cap_per_day: value("--cap-per-day")
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .map(|mb| mb << 20),
        slow_to: value("--slow-to")
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .map(|kb| kb << 10),
    };
    for (what, addr) in [("", &options.listen), (" (TLS)", &options.tls_listen)] {
        if let Some(a) = addr {
            eprintln!("qanuvi-relay listening on {a}{what}");
        }
    }
    if let Err(e) = qanuvi_lib::relay_server::run_with(&options) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn fail(why: impl std::fmt::Display) -> ! {
    eprintln!("{why}");
    std::process::exit(1);
}

/// Relay kit K1: the address of a relay with its own certificate (made now if it is not there yet).
fn address(host: Option<String>, port: Option<String>, data: Option<String>) {
    let host = host.unwrap_or_else(|| fail(format!("address needs --host <IP or name>.\n{USAGE}")));
    let data = data.unwrap_or_else(|| {
        fail("address needs --data <folder> (where the relay keeps its certificate).")
    });
    let port = port.unwrap_or_else(|| "47443".into());
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host
    };
    let files = qanuvi_lib::relay_server::self_made(std::path::Path::new(&data))
        .unwrap_or_else(|e| fail(e));
    let pin = qanuvi_lib::relay_server::pin_of_file(&files.cert).unwrap_or_else(|e| fail(e));
    println!("wss://{host}:{port}#pin={pin}");
}

/// Relay kit K4: connects to a relay as an app would (TLS, checked by the pin when the address has one).
fn check(addr: Option<String>) {
    let addr = addr
        .unwrap_or_else(|| fail("check needs an address, like wss://203.0.113.5:47443#pin=..."));
    let started = std::time::Instant::now();
    match qanuvi_lib::relay_client::Link::connect(&addr) {
        Ok(link) => {
            drop(link);
            println!(
                "OK: the relay answered in {} ms.",
                started.elapsed().as_millis()
            );
        }
        Err(e) => fail(format!("Not reached: {e}")),
    }
}
