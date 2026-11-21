//! `qanuvi-relay`: the relay server (Milestone 20). It passes sealed messages between computers and the devices that
//! reach them, and cannot read them. It runs behind a TLS proxy (nginx or Caddy) for wss://, on its own TLS port, or
//! both at once (24rl R2).
//!
//! qanuvi-relay [--listen 127.0.0.1:47300] [--tls-listen 0.0.0.0:47443 --cert <file> --key <file>]
//!              [--public-direct wss://relay.example.com:47443] [--data <folder>]

const USAGE: &str = "Usage: qanuvi-relay [--listen 127.0.0.1:47300] \
[--tls-listen 0.0.0.0:47443 --cert <fullchain.pem> --key <key.pem>] \
[--public-direct wss://relay.example.com:47443] [--data <folder>] [--ping-every 30]\n\
With no --listen and no --tls-listen it listens on 127.0.0.1:47300 (for a TLS proxy in front).";

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
        direct: value("--public-direct"),
        data: value("--data").map(std::path::PathBuf::from),
        ping_every: value("--ping-every")
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .map(std::time::Duration::from_secs),
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
