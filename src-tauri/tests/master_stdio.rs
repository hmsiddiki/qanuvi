//! `qanuvi-master --stdio` as a caller (SSH, a script) sees it: when the bridge ends, its output ends too, even
//! though the background service it started keeps running (finding k1: on Windows the service used to inherit the
//! caller's output pipe, so the caller waited for ever).

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const EXE: &str = env!("CARGO_BIN_EXE_qanuvi-master");

#[test]
fn the_caller_sees_the_output_end_while_the_service_keeps_running() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().to_path_buf();
    let mut child = Command::new(EXE)
        .arg("--stdio")
        .arg("--data")
        .arg(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut o = Vec::new();
        let mut e = Vec::new();
        let _ = out.read_to_end(&mut o);
        let _ = err.read_to_end(&mut e);
        let _ = tx.send(e);
    });
    // Wait until the bridge has started the service, then end the bridge by closing its input.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !status(&dir).starts_with("Running") {
        assert!(
            std::time::Instant::now() < deadline,
            "the service did not start"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(child.stdin.take());
    let ended = rx.recv_timeout(Duration::from_secs(15));
    let _ = child.wait();
    let still_running = status(&dir).starts_with("Running");
    let _ = Command::new(EXE)
        .arg("stop")
        .arg("--data")
        .arg(&dir)
        .output();
    assert!(
        ended.is_ok(),
        "the caller's output did not end after --stdio exited"
    );
    assert!(
        still_running,
        "the service ends with the bridge; it should keep running"
    );
}

fn status(dir: &std::path::Path) -> String {
    let o = Command::new(EXE)
        .arg("status")
        .arg("--data")
        .arg(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned()
}
