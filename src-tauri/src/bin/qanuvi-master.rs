//! `qanuvi-master`: Qanuvi without a window, for a Linux server or VPS (Milestone 16). The desktop app reaches it over
//! SSH; see master.rs.
//!
//! qanuvi-master serve       run the background service (also as a systemd user service): a small watcher that
//!                           starts the core, restarts it after a crash and (This PC) shows the tray icon
//! qanuvi-master core        the core itself (started by the watcher)
//! qanuvi-master --stdio     what the desktop app starts through SSH (starts the service when needed)
//! qanuvi-master status      is the service running?
//! qanuvi-master stop        stop the service
//! qanuvi-master login       sign this machine in to its Qanuvi account with an application password
//!                           (--site <address>; --password-stdin reads the login name and password from stdin)
//! qanuvi-master logout      forget this machine's sign-in
//! qanuvi-master servers     the servers this machine connects to: list, add <ssh address>, remove <name>
//! qanuvi-master pair        pair with a computer through the relay (--relay <address> --code <pairing code>)
//! qanuvi-master projects    the projects this master works on: list, add <folder>, remove <name>
//! qanuvi-master inbox       questions and notes: list, show <number>, answer <number> <choice> [text]
//! qanuvi-master update      put a newer version in (the running service does it and restarts from it)
//! qanuvi-master version     this program's version
//! qanuvi-master plan        plan runs: run <plan file> (follows it; Ctrl+C leaves, the run goes on), follow, list, stop
//! Options: --data <folder>  where its database and settings are (default ~/.local/share/qanuvi)

use std::path::PathBuf;

#[cfg(feature = "desktop")]
#[path = "../master_tray.rs"]
mod master_tray;

/// This PC's master shows a tray icon where there is a desktop to show it on (migration M4c; the watcher's, M5b).
fn tray() -> Option<qanuvi_lib::master::Tray> {
    #[cfg(feature = "desktop")]
    if master_tray::can_show() {
        return Some(master_tray::run);
    }
    None
}

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
    let site_at = args.iter().position(|a| a == "--site");
    let site = site_at.and_then(|i| args.get(i + 1)).map(String::as_str);
    let data = data_at
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("QANUVI_MASTER_DATA").map(PathBuf::from))
        .unwrap_or_else(qanuvi_lib::paths::default_master_dir);
    let command = args
        .iter()
        .enumerate()
        .skip(1)
        .find(|(i, a)| {
            !a.starts_with("--")
                && Some(*i) != data_at.map(|d| d + 1)
                && Some(*i) != site_at.map(|d| d + 1)
        })
        .map(|(_, a)| a.as_str());
    // M8a: a terminal command starts with "Update available: x" when its running master found one (status says it
    // in its own lines; the service, its core and the window's bridge never print it).
    if matches!(
        command,
        Some("login" | "logout" | "servers" | "pair" | "projects" | "inbox" | "plan")
    ) && !args.iter().any(|a| a == "--stdio")
    {
        qanuvi_lib::master::update_notice(&data);
    }
    let result = if args.iter().any(|a| a == "--stdio") {
        qanuvi_lib::master::stdio_bridge(&data)
    } else {
        match command {
            Some("serve") => {
                // A restart (Settings, Backups): the service before must end first (it has the data open).
                if let Some(pid) = qanuvi_lib::data_location::after_pid(args.iter().cloned()) {
                    qanuvi_lib::data_location::wait_for_exit(
                        pid,
                        std::time::Duration::from_secs(30),
                    );
                }
                qanuvi_lib::master::serve(&data, tray())
            }
            Some("core") => qanuvi_lib::master::serve_core(&data),
            Some("stop") => {
                let stopped = qanuvi_lib::master::stop_service(&data);
                println!("{}", if stopped { "Stopped." } else { "Not running." });
                Ok(())
            }
            Some("status") => qanuvi_lib::master::status_cli(&data),
            Some("update") => qanuvi_lib::master::update_cli(&data),
            Some("version") => {
                println!("{}", env!("CARGO_PKG_VERSION"));
                Ok(())
            }
            Some("login") => qanuvi_lib::account::cli_login(
                &qanuvi_lib::master::accounts_at(&data),
                site,
                args.iter().any(|a| a == "--password-stdin"),
            ),
            Some("logout") => {
                qanuvi_lib::account::cli_logout(&qanuvi_lib::master::accounts_at(&data), site)
            }
            Some(c @ ("servers" | "pair" | "projects" | "inbox" | "plan")) => {
                let rest: Vec<String> = args
                    .iter()
                    .skip_while(|a| a.as_str() != c)
                    .skip(1)
                    .cloned()
                    .collect();
                match c {
                    "servers" => qanuvi_lib::master::servers_cli(&data, &rest),
                    "projects" => qanuvi_lib::master::projects_cli(&data, &rest),
                    "inbox" => qanuvi_lib::master::inbox_cli(&data, &rest),
                    "plan" => qanuvi_lib::master::plan_cli(&data, &rest),
                    _ => qanuvi_lib::master::pair_cli(&data, &rest),
                }
            }
            _ => {
                eprintln!(
                    "Usage: qanuvi-master serve | core | --stdio | status | stop | update | version | login | logout | servers | pair | projects | inbox | plan  [--data <folder>] [--site <address>]"
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
