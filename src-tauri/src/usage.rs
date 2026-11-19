//! What Qanuvi uses in all (1f), so the owner sees Qanuvi's total in one place, whatever Task Manager groups apart.
//! Two halves since the migration (u2): a master's (its core, the watcher that started the core, and every process
//! the core started: agents, shells, language servers, start-up commands), and the screen's (the window and its
//! WebView2 processes), which the window measures itself, so any master version works with it.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    pub name: String,
    pub count: usize,
    pub memory: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Bytes of memory (working sets) of all of them.
    pub memory: u64,
    /// Share of the whole computer's processor time since the last look, in percent.
    pub cpu: f64,
    pub processes: usize,
    /// By program, the biggest first.
    pub parts: Vec<Part>,
    /// u2: bytes of the master program's own processes (core and watcher); the rest is what it started.
    pub own: u64,
}

/// The master program's file name (without .exe), for telling its processes apart.
const MASTER: &str = "qanuvi-master";

/// The processes that descend from `root` and are not under a `stop` one (`root` included, `stop` ones not).
pub fn descendants_until(root: u32, pairs: &[(u32, u32)], stop: &[u32]) -> Vec<u32> {
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        let p = out[i];
        for &(pid, parent) in pairs {
            if parent == p && pid != p && !out.contains(&pid) && !stop.contains(&pid) {
                out.push(pid);
            }
        }
        i += 1;
    }
    out
}

/// The processes that descend from `root`, given (pid, parent) pairs; `root` included.
pub fn descendants(root: u32, pairs: &[(u32, u32)]) -> Vec<u32> {
    descendants_until(root, pairs, &[])
}

/// Whose half a look measures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Look<'a> {
    /// A master's core (this process); `watched`: its watcher too (the core's parent).
    Master { watched: bool },
    /// The window (this process) and what it started, every master's service left out: of the master program's
    /// processes only `links` (the link helpers it started, `qanuvi-master --stdio`) are the screen's; any other
    /// (This PC's watcher and core, or a service a link helper started) is a master's half, with all under it.
    Screen { links: &'a [u32] },
}

/// The watcher above `core`: its parent when that is the master program too.
fn watcher_of(core: u32, all: &[(u32, u32, String)]) -> Option<u32> {
    let parent = all
        .iter()
        .find(|(p, _, _)| *p == core)
        .map(|(_, q, _)| *q)?;
    all.iter()
        .any(|(p, _, n)| *p == parent && is_master(n))
        .then_some(parent)
}

/// Which processes a look from `me` counts, given (pid, parent, name) of all.
pub fn chosen(me: u32, all: &[(u32, u32, String)], look: Look) -> Vec<u32> {
    let pairs: Vec<(u32, u32)> = all.iter().map(|(p, q, _)| (*p, *q)).collect();
    match look {
        Look::Master { watched } => {
            let mut ours = descendants(me, &pairs);
            if let Some(w) = watcher_of(me, all).filter(|_| watched) {
                ours.insert(0, w);
            }
            ours
        }
        Look::Screen { links } => {
            let stop: Vec<u32> = all
                .iter()
                .filter(|(p, _, n)| *p != me && is_master(n) && !links.contains(p))
                .map(|(p, _, _)| *p)
                .collect();
            descendants_until(me, &pairs, &stop)
        }
    }
}

fn is_master(name: &str) -> bool {
    name.trim_end_matches(".exe").eq_ignore_ascii_case(MASTER)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Instant;
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// The last look's time and processor time, the master's and the screen's apart.
    static LAST: Mutex<[Option<(Instant, u64)>; 2]> = Mutex::new([None, None]);

    fn ft(f: &FILETIME) -> u64 {
        ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
    }

    fn processes() -> Vec<(u32, u32, String)> {
        let mut out = Vec::new();
        // SAFETY: a process snapshot read entry by entry, then closed.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return out;
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if Process32FirstW(snap, &mut e) != 0 {
                loop {
                    let len = e
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(e.szExeFile.len());
                    out.push((
                        e.th32ProcessID,
                        e.th32ParentProcessID,
                        String::from_utf16_lossy(&e.szExeFile[..len]),
                    ));
                    if Process32NextW(snap, &mut e) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
        }
        out
    }

    pub fn usage(look: Look<'_>) -> Usage {
        let master = matches!(look, Look::Master { .. });
        let all = processes();
        let ours = chosen(std::process::id(), &all, look);
        let mut memory = 0u64;
        let mut own = 0u64;
        let mut cpu_time = 0u64;
        let mut parts: HashMap<String, Part> = HashMap::new();
        for pid in &ours {
            let name = all
                .iter()
                .find(|(p, _, _)| p == pid)
                .map(|(_, _, n)| n.trim_end_matches(".exe").to_string())
                .unwrap_or_default();
            // SAFETY: a handle opened for reading counters only, closed after use.
            let (mem, time) = unsafe {
                let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, *pid);
                if h.is_null() {
                    continue;
                }
                let mut pmc: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
                pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
                let mem = if GetProcessMemoryInfo(h, &mut pmc, pmc.cb) != 0 {
                    pmc.WorkingSetSize as u64
                } else {
                    0
                };
                let (mut c, mut x, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) = (
                    std::mem::zeroed(),
                    std::mem::zeroed(),
                    std::mem::zeroed(),
                    std::mem::zeroed(),
                );
                let time = if GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u) != 0 {
                    ft(&k) + ft(&u)
                } else {
                    0
                };
                CloseHandle(h);
                (mem, time)
            };
            memory += mem;
            cpu_time += time;
            if master && is_master(&name) {
                own += mem;
            }
            let part = parts.entry(name.clone()).or_insert(Part {
                name,
                count: 0,
                memory: 0,
            });
            part.count += 1;
            part.memory += mem;
        }
        // Processor share since the last look (100 ns units of process time over wall time and cores).
        let now = Instant::now();
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1) as f64;
        let mut looks = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let last = &mut looks[usize::from(!master)];
        let cpu = match *last {
            Some((t, before)) if cpu_time >= before => {
                let wall = now.duration_since(t).as_secs_f64() * 10_000_000.0;
                if wall > 0.0 {
                    ((cpu_time - before) as f64 / wall / cores * 100.0).min(100.0)
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        *last = Some((now, cpu_time));
        let mut parts: Vec<Part> = parts.into_values().collect();
        parts.sort_by_key(|p| std::cmp::Reverse(p.memory));
        Usage {
            memory,
            cpu,
            processes: ours.len(),
            parts,
            own,
        }
    }
}

/// A master's half now: its core, its watcher when `watched`, and all the core started (on other systems: none yet).
pub fn usage(watched: bool) -> Option<Usage> {
    #[cfg(windows)]
    {
        Some(imp::usage(Look::Master { watched }))
    }
    #[cfg(not(windows))]
    {
        let _ = watched;
        None
    }
}

/// The screen's half now: this window and what it started (WebView2, its link helpers `links`), every master's
/// service left out (on other systems: none yet).
pub fn screen_usage(links: &[u32]) -> Option<Usage> {
    #[cfg(windows)]
    {
        Some(imp::usage(Look::Screen { links }))
    }
    #[cfg(not(windows))]
    {
        let _ = links;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_every_process_started_from_qanuvi() {
        // 10 is Qanuvi; 11 its WebView2, 12 a WebView2 child, 13 an agent, 14 its helper; 20 and 21 are not ours.
        let pairs = [(11, 10), (12, 11), (13, 10), (14, 13), (20, 1), (21, 20)];
        let mut d = descendants(10, &pairs);
        d.sort();
        assert_eq!(d, [10, 11, 12, 13, 14]);
    }

    fn named(list: &[(u32, u32, &str)]) -> Vec<(u32, u32, String)> {
        list.iter()
            .map(|(p, q, n)| (*p, *q, n.to_string()))
            .collect()
    }

    // 5 the window; 6, 7 its WebView2; 8 the watcher the window started; 9 its core; 10 an agent; 11 a language
    // server; 12 a link helper the window started (`--stdio`), 13 its ssh; 30 another program's.
    const TREE: &[(u32, u32, &str)] = &[
        (5, 1, "qanuvi.exe"),
        (6, 5, "msedgewebview2.exe"),
        (7, 6, "msedgewebview2.exe"),
        (8, 5, "qanuvi-master.exe"),
        (9, 8, "qanuvi-master.exe"),
        (10, 9, "agent.exe"),
        (11, 9, "node.exe"),
        (12, 5, "qanuvi-master.exe"),
        (13, 12, "ssh.exe"),
        (30, 1, "other.exe"),
    ];

    #[test]
    fn a_watched_master_counts_its_watcher_and_all_it_started() {
        let mut d = chosen(9, &named(TREE), Look::Master { watched: true });
        d.sort();
        assert_eq!(d, [8, 9, 10, 11]);
        let mut d = chosen(9, &named(TREE), Look::Master { watched: false });
        d.sort();
        assert_eq!(d, [9, 10, 11]);
    }

    #[test]
    fn a_master_never_counts_a_parent_that_is_not_its_watcher() {
        // A core started by hand from a shell (a test master): the shell is not Qanuvi's.
        let tree = named(&[
            (2, 1, "powershell.exe"),
            (9, 2, "qanuvi-master.exe"),
            (10, 9, "agent.exe"),
        ]);
        let mut d = chosen(9, &tree, Look::Master { watched: true });
        d.sort();
        assert_eq!(d, [9, 10]);
    }

    #[test]
    fn the_screen_leaves_this_pcs_service_out_and_keeps_its_link_helpers() {
        let mut d = chosen(5, &named(TREE), Look::Screen { links: &[12] });
        d.sort();
        assert_eq!(d, [5, 6, 7, 12, 13]);
    }

    #[test]
    fn the_screen_leaves_out_a_service_its_link_helper_started() {
        // The helper (12) found no service and started one: 12 -> watcher 8 -> core 9 (a local test master's).
        let tree = named(&[
            (5, 1, "qanuvi.exe"),
            (12, 5, "qanuvi-master.exe"),
            (8, 12, "qanuvi-master.exe"),
            (9, 8, "qanuvi-master.exe"),
            (10, 9, "agent.exe"),
        ]);
        let mut d = chosen(5, &tree, Look::Screen { links: &[12] });
        d.sort();
        assert_eq!(d, [5, 12]);
    }

    #[cfg(windows)]
    #[test]
    fn measures_this_process() {
        let u = usage(false).unwrap();
        assert!(u.processes >= 1);
        assert!(u.memory > 0);
        let s = screen_usage(&[]).unwrap();
        assert!(s.memory > 0);
        assert!(u.processes >= 1);
        assert!(u.memory > 0);
    }
}
