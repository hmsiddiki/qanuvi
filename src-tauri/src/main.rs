// Keeps a console window from opening next to the app on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `qanuvi --mcp <folder>`: answer an agent's code-index tools on stdin/stdout, with no window.
    let args: Vec<String> = std::env::args().collect();
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
    // `qanuvi --mcp-browser <folder> [state file]`: the test browser's tools for an agent.
    if args.get(1).map(String::as_str) == Some("--mcp-browser") {
        qanuvi_lib::run_mcp_browser(
            args.get(2).map(String::as_str).unwrap_or("."),
            args.get(3).map(String::as_str),
        );
        return;
    }
    qanuvi_lib::run()
}
