//! What this computer can run (Milestone 24): its memory and graphics card, so Qanuvi can suggest local models that
//! fit. Read once when asked; nothing is sent anywhere.

use serde::Serialize;
use std::process::Command;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Gpu {
    pub name: String,
    /// Its own memory in GB (on Apple chips: the part of the shared memory models can use).
    pub vram_gb: f64,
    /// "nvidia", "amd", "apple" or "other".
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    pub ram_gb: f64,
    pub gpu: Option<Gpu>,
    pub os: String,
    pub arch: String,
}

fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = quiet(&mut Command::new(program)).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

#[cfg(windows)]
fn ram_bytes() -> u64 {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: a plain call filling a struct whose size is set first.
    unsafe {
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut m) != 0 {
            m.ullTotalPhys
        } else {
            0
        }
    }
}

#[cfg(target_os = "linux")]
fn ram_bytes() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|k| k.parse::<u64>().ok())
        })
        .map(|kb| kb * 1024)
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
fn ram_bytes() -> u64 {
    output("sysctl", &["-n", "hw.memsize"])
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn ram_bytes() -> u64 {
    0
}

/// `nvidia-smi` lines like "NVIDIA GeForce RTX 3050 Ti Laptop GPU, 4096".
pub fn parse_nvidia(text: &str) -> Option<Gpu> {
    text.lines()
        .filter_map(|l| {
            let (name, mib) = l.rsplit_once(',')?;
            let mib: f64 = mib.trim().trim_end_matches("MiB").trim().parse().ok()?;
            Some(Gpu {
                name: name.trim().to_string(),
                vram_gb: round1(mib / 1024.0),
                kind: "nvidia".into(),
            })
        })
        .max_by(|a, b| a.vram_gb.total_cmp(&b.vram_gb))
}

fn gpu(ram_gb: f64) -> Option<Gpu> {
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        // Apple chips share memory; models can use about two thirds of it.
        return Some(Gpu {
            name: "Apple silicon".into(),
            vram_gb: round1(ram_gb * 0.66),
            kind: "apple".into(),
        });
    }
    if let Some(g) = output(
        "nvidia-smi",
        &[
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ],
    )
    .and_then(|t| parse_nvidia(&t))
    {
        return Some(g);
    }
    if cfg!(windows) {
        // Other cards: the name, and memory as Windows reports it (it tops out at 4 GB there).
        let text = output(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_VideoController | ForEach-Object { \"$($_.Name)|$($_.AdapterRAM)\" }",
            ],
        )?;
        return text
            .lines()
            .filter_map(|l| {
                let (name, bytes) = l.split_once('|')?;
                let bytes: f64 = bytes.trim().parse().ok()?;
                let lower = name.to_lowercase();
                // Built-in graphics share the main memory: not counted as a card.
                if lower.contains("intel") || lower.contains("microsoft basic") || bytes < 1.5e9 {
                    return None;
                }
                Some(Gpu {
                    name: name.trim().to_string(),
                    vram_gb: round1(bytes / 1_073_741_824.0),
                    kind: if lower.contains("radeon") || lower.contains("amd") {
                        "amd".into()
                    } else {
                        "other".into()
                    },
                })
            })
            .max_by(|a, b| a.vram_gb.total_cmp(&b.vram_gb));
    }
    None
}

#[cfg(windows)]
fn free_ram_bytes() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: as in ram_bytes.
    unsafe {
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        (GlobalMemoryStatusEx(&mut m) != 0).then_some(m.ullAvailPhys)
    }
}

#[cfg(target_os = "linux")]
fn free_ram_bytes() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = s
        .lines()
        .find(|l| l.starts_with("MemAvailable:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some(kb * 1024)
}

#[cfg(target_os = "macos")]
fn free_ram_bytes() -> Option<u64> {
    // Free, inactive and purgeable pages can all be given to a new program.
    let text = output("vm_stat", &[])?;
    let page: u64 = text
        .lines()
        .next()?
        .split("page size of ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let pages = |name: &str| -> u64 {
        text.lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.rsplit(':').next())
            .and_then(|v| v.trim().trim_end_matches('.').parse().ok())
            .unwrap_or(0)
    };
    Some((pages("Pages free") + pages("Pages inactive") + pages("Pages purgeable")) * page)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn free_ram_bytes() -> Option<u64> {
    None
}

/// What is free right now: main memory and (NVIDIA cards) the card's memory. Read just before something heavy starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Room {
    pub free_ram_gb: f64,
    pub free_vram_gb: f64,
}

/// Memory kept for Windows/macOS/Linux and the user's other programs; nothing Qanuvi starts may go below it.
pub const RESERVE_GB: f64 = 1.5;

pub fn room() -> Option<Room> {
    let free_ram_gb = free_ram_bytes()? as f64 / 1_073_741_824.0;
    let free_vram_gb = output(
        "nvidia-smi",
        &["--query-gpu=memory.free", "--format=csv,noheader,nounits"],
    )
    .and_then(|t| {
        t.lines()
            .filter_map(|l| l.trim().parse::<f64>().ok())
            .reduce(f64::max)
    })
    .map(|mib| mib / 1024.0)
    .unwrap_or(0.0);
    Some(Room {
        free_ram_gb,
        free_vram_gb,
    })
}

/// Whether something needing `need_gb` fits now: what the card can hold goes there, the rest must fit in main memory
/// with the reserve left over. The error says it in numbers.
pub fn fits(what: &str, need_gb: f64, room: Room) -> Result<(), String> {
    let on_card = need_gb.min(room.free_vram_gb * 0.9);
    let in_ram = need_gb - on_card;
    if in_ram <= room.free_ram_gb - RESERVE_GB {
        return Ok(());
    }
    Err(format!(
        "{what} needs about {need_gb:.1} GB, but only {:.1} GB of memory is free (and {:.1} GB on the graphics card); Qanuvi keeps {RESERVE_GB} GB for the computer itself. Close a program or use a smaller model.",
        room.free_ram_gb, room.free_vram_gb
    ))
}

/// Checks now, if the room can be read (a computer Qanuvi cannot read is not stopped).
pub fn check(what: &str, need_gb: f64) -> Result<(), String> {
    match room() {
        Some(r) => fits(what, need_gb, r),
        None => Ok(()),
    }
}

pub fn detect() -> Machine {
    let ram_gb = round1(ram_bytes() as f64 / 1_073_741_824.0);
    Machine {
        ram_gb,
        gpu: gpu(ram_gb),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_room_before_starting_something() {
        let r = |ram, vram| Room {
            free_ram_gb: ram,
            free_vram_gb: vram,
        };
        // 2.5 GB model: fits a 4 GB card even with little main memory free.
        assert!(fits("m", 2.5, r(2.0, 3.5)).is_ok());
        // No card: must fit main memory with 1.5 GB to spare.
        assert!(fits("m", 2.5, r(4.5, 0.0)).is_ok());
        let e = fits("qwen3:4b", 2.5, r(3.0, 0.0)).unwrap_err();
        assert!(
            e.contains("qwen3:4b needs about 2.5 GB") && e.contains("3.0 GB"),
            "{e}"
        );
        // Half on the card, the rest in memory.
        assert!(fits("m", 6.0, r(5.0, 3.0)).is_ok());
        assert!(fits("m", 6.0, r(4.0, 2.0)).is_err());
        assert!(room().is_some_and(|x| x.free_ram_gb > 0.0));
    }

    #[test]
    fn reads_nvidia_cards() {
        let g = parse_nvidia("NVIDIA GeForce RTX 3050 Ti Laptop GPU, 4096\n").unwrap();
        assert_eq!(g.name, "NVIDIA GeForce RTX 3050 Ti Laptop GPU");
        assert_eq!(g.vram_gb, 4.0);
        let g = parse_nvidia("A, 8192\nB, 24576\n").unwrap();
        assert_eq!((g.name.as_str(), g.vram_gb), ("B", 24.0));
        assert!(parse_nvidia("").is_none());
    }

    #[test]
    fn knows_this_computer() {
        let m = detect();
        assert!(m.ram_gb > 0.5, "{m:?}");
    }
}
