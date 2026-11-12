//! What Qanuvi uses in all (1f): its own process and every process it started (the window's WebView2 processes, the
//! agents and their helpers, start-up commands), so the owner sees Qanuvi's total in one place, whatever Task Manager
//! groups apart.

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
}

/// The processes that descend from `root`, given (pid, parent) pairs; `root` included.
pub fn descendants(root: u32, pairs: &[(u32, u32)]) -> Vec<u32> {
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        let p = out[i];
        for &(pid, parent) in pairs {
            if parent == p && pid != p && !out.contains(&pid) {
                out.push(pid);
            }
        }
        i += 1;
    }
    out
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

    static LAST: Mutex<Option<(Instant, u64)>> = Mutex::new(None);

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

    pub fn usage() -> Usage {
        let all = processes();
        let pairs: Vec<(u32, u32)> = all.iter().map(|(p, q, _)| (*p, *q)).collect();
        let ours = descendants(std::process::id(), &pairs);
        let mut memory = 0u64;
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
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
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
        }
    }
}

/// Qanuvi's total now (on other systems: none yet).
pub fn usage() -> Option<Usage> {
    #[cfg(windows)]
    {
        Some(imp::usage())
    }
    #[cfg(not(windows))]
    {
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

    #[cfg(windows)]
    #[test]
    fn measures_this_process() {
        let u = usage().unwrap();
        assert!(u.processes >= 1);
        assert!(u.memory > 0);
    }
}
