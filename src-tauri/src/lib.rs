//! Qanuvi core: projects, files, git, the code index, checks, agent sessions, terminals and the test browser. The
//! desktop app (feature `desktop`, desktop.rs) and the master without a window (master.rs, Milestone 16) both use it.

pub mod agent;
pub mod agent_config;
pub mod agent_tools;
pub mod attachments;
pub mod backups;
pub mod brain;
pub mod brief;
pub mod browser;
pub mod browser_mcp;
pub mod checks;
pub mod claude_update;
pub mod dap;
pub mod data_location;
pub mod files;
pub mod git;
pub mod inbox;
pub mod index;
pub mod knowledge;
pub mod llm;
pub mod local_agent;
pub mod local_servers;
pub mod lsp;
pub mod machine;
pub mod master;
pub mod mcp;
pub mod memory;
pub mod memory_disk;
pub mod notify;
pub mod omniroute;
pub mod paths;
pub mod preview;
pub mod projects;
pub mod relay;
pub mod relay_client;
pub mod relay_server;
pub mod runs;
pub mod secrets;
pub mod sink;
pub mod startup;
pub mod store;
pub mod terminal;
pub mod usage;

#[cfg(feature = "desktop")]
mod account;
#[cfg(feature = "desktop")]
mod desktop;
#[cfg(feature = "desktop")]
mod desktop_notify;
#[cfg(feature = "desktop")]
mod remote;
#[cfg(feature = "desktop")]
pub use desktop::*;

use std::path::PathBuf;

/// Runs Qanuvi's own agent in the current folder (see `local_agent.rs`); returns the exit code.
pub fn run_agent(args: &[String]) -> i32 {
    local_agent::run(args)
}

/// Runs the code-index tools for an agent (see `mcp.rs`).
pub fn run_mcp(folder: &str, memory: Option<(&str, &str)>) {
    let root = std::path::Path::new(folder)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(folder));
    let mut server = mcp::Server::new(&root);
    if let Some((db, scope)) = memory
        && let Ok(db) = store::Db::open(std::path::Path::new(db))
    {
        server = server.with_memory(db, scope);
    }
    mcp::serve(&mut server);
}

/// `--mcp <folder> [--memory <database> --scope <project id>]`: the parts after the folder.
pub fn mcp_memory_args(args: &[String]) -> Option<(&str, &str)> {
    let at = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .map(String::as_str)
    };
    Some((at("--memory")?, at("--scope")?))
}

/// `qanuvi --mcp-browser <folder> [state file]`: the test browser's tools for an agent.
pub fn run_mcp_browser(folder: &str, state: Option<&str>) {
    let root = std::path::Path::new(folder)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(folder));
    browser_mcp::serve(&root, state.map(PathBuf::from));
}
