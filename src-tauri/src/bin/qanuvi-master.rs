//! `qanuvi-master`: Qanuvi without a window, for a Linux server or VPS (Milestone 16). The desktop app reaches it over
//! SSH; see master.rs.
//!
//! qanuvi-master serve       run the background service (also as a systemd user service)
//! qanuvi-master --stdio     what the desktop app starts through SSH (starts the service when needed)
//! qanuvi-master status      is the service running?
//! qanuvi-master stop        stop the service
//! Options: --data <folder>  where its database and settings are (default ~/.local/share/qanuvi)

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Qanuvi's own tools for agents (the same as the desktop app's).
    // `--agent ...`: Qanuvi's own agent in the current folder, speaking JSON lines on stdin/stdout.
    if args.get(1).map(String::as_str) == Some("--agent") {
        std::process::exit(qanuvi_lib::run_agent(&args[2..]));
    }
    if args.get(1).map(String::as_str) == Some("--mcp") {
        qanuvi_lib::run_mcp(
            args.get(2).map(String::as_str).unwrap_or("."),
            qanuvi_lib::mcp_memory_args(&args),
        );
        return;
    }
    if args.get(1).map(String::as_str) == Some("--mcp-browser") {
        qanuvi_lib::run_mcp_browser(
            args.get(2).map(String::as_str).unwrap_or("."),
            args.get(3).map(String::as_str),
        );
        return;
    }
    let data_at = args.iter().position(|a| a == "--data");
    let data = data_at
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("QANUVI_MASTER_DATA").map(PathBuf::from))
        .unwrap_or_else(qanuvi_lib::paths::default_master_dir);
    let command = args
        .iter()
        .enumerate()
        .skip(1)
        .find(|(i, a)| !a.starts_with("--") && Some(*i) != data_at.map(|d| d + 1))
        .map(|(_, a)| a.as_str());
    let result = if args.iter().any(|a| a == "--stdio") {
        qanuvi_lib::master::stdio_bridge(&data)
    } else {
        match command {
            Some("serve") => qanuvi_lib::master::serve(&data),
            Some("stop") => {
                let stopped = qanuvi_lib::master::stop_service(&data);
                println!("{}", if stopped { "Stopped." } else { "Not running." });
                Ok(())
            }
            Some("status") => {
                if qanuvi_lib::master::is_running(&data) {
                    println!("Running (data in {}).", data.display());
                } else {
                    println!("Not running.");
                }
                Ok(())
            }
            _ => {
                eprintln!(
                    "Usage: qanuvi-master serve | --stdio | status | stop  [--data <folder>]"
                );
                std::process::exit(2);
            }
        }
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
