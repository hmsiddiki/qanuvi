//! `qanuvi-relay`: the relay server (Milestone 20). It passes sealed messages between computers and the devices that
//! reach them, and cannot read them. Run it on a VPS or hosting behind a TLS proxy (nginx or Caddy) for wss://.
//!
//! qanuvi-relay [--listen 127.0.0.1:47300] [--data <folder>]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qanuvi-relay [--listen 127.0.0.1:47300] [--data <folder>]");
        return;
    }
    let listen = value("--listen").unwrap_or_else(|| "127.0.0.1:47300".into());
    let data = value("--data").map(std::path::PathBuf::from);
    eprintln!("qanuvi-relay listening on {listen}");
    if let Err(e) = qanuvi_lib::relay_server::run(&listen, data.as_deref()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
