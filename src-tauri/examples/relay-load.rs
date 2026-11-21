//! 24rl R4: load test for qanuvi-relay (not shipped). Opens many master + handler pairs the way apps do, keeps most
//! of them idle, lets some talk, and reports how many are up and how long a message takes. Every link answers the
//! relay's pings and connects again (1 s, then up to 30 s apart, with jitter) when its link ends, as apps do.
//!
//! cargo run --release --example relay-load -- --relay ws://127.0.0.1:47300 --pairs 50000 --active 500
//!     [--ca cert.pem] [--sources 8] [--every 5] [--seconds 600] [--rate 2000]
//!
//! --relay wss://127.0.0.1:47443 with --ca uses the relay's own TLS port. --sources N spreads the links over
//! 127.0.0.2 .. 127.0.0.(N+1) so one machine does not run out of ports. --rate is new links per second.

use futures_util::{SinkExt, StreamExt};
use qanuvi_lib::relay::Msg;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone)]
struct Setup {
    relay: String,
    host: String,
    port: u16,
    tls: Option<Arc<rustls::ClientConfig>>,
    sources: u32,
    every: Duration,
}

#[derive(Default)]
struct Counts {
    up: AtomicU64,
    paired: AtomicU64,
    reconnects: AtomicU64,
    sent: AtomicU64,
    got: AtomicU64,
    lat: std::sync::Mutex<Vec<u32>>,
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

type Ws = tokio_tungstenite::WebSocketStream<Box<dyn Io>>;

fn arg(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn text(m: &Msg) -> Message {
    Message::text(serde_json::to_string(m).unwrap())
}

async fn open(s: &Setup, n: u64) -> Result<Ws, String> {
    let socket = tokio::net::TcpSocket::new_v4().map_err(|e| e.to_string())?;
    if s.sources > 0 {
        let src = std::net::Ipv4Addr::new(127, 0, 0, 2 + (n % s.sources as u64) as u8);
        socket
            .bind((src, 0).into())
            .map_err(|e| format!("bind {src}: {e}"))?;
    }
    let addr = tokio::net::lookup_host((s.host.as_str(), s.port))
        .await
        .map_err(|e| e.to_string())?
        .find(|a| a.is_ipv4())
        .ok_or("no IPv4 address")?;
    let tcp = socket.connect(addr).await.map_err(|e| e.to_string())?;
    let _ = tcp.set_nodelay(true);
    let io: Box<dyn Io> = match &s.tls {
        Some(cfg) => {
            let name = rustls::pki_types::ServerName::try_from(s.host.clone())
                .map_err(|e| e.to_string())?;
            let tls = tokio_rustls::TlsConnector::from(cfg.clone())
                .connect(name, tcp)
                .await
                .map_err(|e| e.to_string())?;
            Box::new(tls)
        }
        None => Box::new(tcp),
    };
    let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .read_buffer_size(4 << 10)
        .write_buffer_size(0);
    let (ws, _) = tokio_tungstenite::client_async_with_config(s.relay.as_str(), io, Some(config))
        .await
        .map_err(|e| e.to_string())?;
    Ok(ws)
}

/// The next relay message (pings are answered by reading); None when the link ended.
async fn next(ws: &mut Ws) -> Option<Msg> {
    loop {
        match ws.next().await? {
            Ok(Message::Text(t)) => {
                if let Ok(m) = serde_json::from_str(t.as_str()) {
                    return Some(m);
                }
            }
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

fn now_us(t0: Instant) -> u64 {
    t0.elapsed().as_micros() as u64
}

/// One pair, for as long as the program runs: connects, pairs, idles or talks, connects again after a drop.
async fn pair(s: Setup, n: u64, active: bool, c: Arc<Counts>, t0: Instant) {
    let mut wait = Duration::from_secs(1);
    let mut first = true;
    loop {
        if !first {
            c.reconnects.fetch_add(1, Ordering::Relaxed);
            let jitter = Duration::from_millis(n % 1000);
            tokio::time::sleep(wait + jitter).await;
            wait = (wait * 2).min(Duration::from_secs(30));
        }
        first = false;
        let Ok(mut master) = open(&s, n * 2).await else {
            continue;
        };
        if master
            .send(text(&Msg::HelloMaster {
                master_id: String::new(),
                secret: String::new(),
            }))
            .await
            .is_err()
        {
            continue;
        }
        let Some(Msg::Welcome { master_id, .. }) = next(&mut master).await else {
            continue;
        };
        c.up.fetch_add(1, Ordering::Relaxed);
        let Ok(mut handler) = open(&s, n * 2 + 1).await else {
            c.up.fetch_sub(1, Ordering::Relaxed);
            continue;
        };
        c.up.fetch_add(1, Ordering::Relaxed);
        let ok = async {
            handler
                .send(text(&Msg::Connect {
                    master_id,
                    pairing: "load".into(),
                    nonce: "n".into(),
                    session: 0,
                }))
                .await
                .ok()?;
            let Some(Msg::Connect { session, .. }) = next(&mut master).await else {
                return None;
            };
            master
                .send(text(&Msg::Accept {
                    session,
                    nonce: "m".into(),
                    direct: None,
                }))
                .await
                .ok()?;
            matches!(next(&mut handler).await?, Msg::Accept { .. }).then_some(session)
        }
        .await;
        let Some(session) = ok else {
            c.up.fetch_sub(2, Ordering::Relaxed);
            continue;
        };
        c.paired.fetch_add(1, Ordering::Relaxed);
        wait = Duration::from_secs(1);
        // Idle or talking until either link ends.
        let mut tick = tokio::time::interval(s.every);
        tick.tick().await;
        loop {
            tokio::select! {
                m = next(&mut master) => match m {
                    Some(Msg::Data { body, .. }) => {
                        if let Ok(sent_at) = body.split(':').next().unwrap_or("").parse::<u64>() {
                            let us = now_us(t0).saturating_sub(sent_at);
                            c.got.fetch_add(1, Ordering::Relaxed);
                            c.lat.lock().unwrap().push(us.min(u32::MAX as u64) as u32);
                        }
                    }
                    Some(_) => {}
                    None => break,
                },
                m = next(&mut handler) => if m.is_none() { break },
                _ = tick.tick(), if active => {
                    let body = format!("{}:{}", now_us(t0), "x".repeat(200));
                    if handler.send(text(&Msg::Data { session, body, more: false })).await.is_err() {
                        break;
                    }
                    c.sent.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        c.paired.fetch_sub(1, Ordering::Relaxed);
        c.up.fetch_sub(2, Ordering::Relaxed);
    }
}

fn pct(v: &mut [u32], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_unstable();
    let i = ((v.len() as f64 - 1.0) * p).round() as usize;
    v[i] as f64 / 1000.0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let relay = arg(&args, "--relay").unwrap_or_else(|| "ws://127.0.0.1:47300".into());
    let pairs: u64 = arg(&args, "--pairs").and_then(|v| v.parse().ok()).unwrap_or(1000);
    let active: u64 = arg(&args, "--active").and_then(|v| v.parse().ok()).unwrap_or(10);
    let sources: u32 = arg(&args, "--sources").and_then(|v| v.parse().ok()).unwrap_or(0);
    let every = Duration::from_secs(arg(&args, "--every").and_then(|v| v.parse().ok()).unwrap_or(5));
    let seconds: u64 = arg(&args, "--seconds").and_then(|v| v.parse().ok()).unwrap_or(120);
    let rate: u64 = arg(&args, "--rate").and_then(|v| v.parse().ok()).unwrap_or(2000);
    let rest = relay.split_once("://").map(|(_, r)| r).unwrap_or(&relay);
    let authority = rest.split('/').next().unwrap_or("");
    let (host, port) = authority.rsplit_once(':').expect("--relay needs a port");
    let tls = arg(&args, "--ca").map(|ca| {
        use rustls::pki_types::{CertificateDer, pem::PemObject};
        let mut roots = rustls::RootCertStore::empty();
        for c in CertificateDer::pem_file_iter(&ca).unwrap() {
            roots.add(c.unwrap()).unwrap();
        }
        Arc::new(
            rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        )
    });
    let setup = Setup {
        relay: relay.clone(),
        host: host.to_string(),
        port: port.parse().expect("port"),
        tls,
        sources,
        every,
    };
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async move {
        let c = Arc::new(Counts::default());
        let t0 = Instant::now();
        let pace = Duration::from_micros(1_000_000 / rate.max(1));
        let spawner = {
            let c = c.clone();
            let setup = setup.clone();
            tokio::spawn(async move {
                for n in 0..pairs {
                    tokio::spawn(pair(setup.clone(), n, n < active, c.clone(), t0));
                    tokio::time::sleep(pace).await;
                }
            })
        };
        let mut all_up_at: Option<f64> = None;
        let mut window = Vec::new();
        for _ in 0..seconds / 5 {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let paired = c.paired.load(Ordering::Relaxed);
            if all_up_at.is_none() && paired == pairs {
                all_up_at = Some(t0.elapsed().as_secs_f64());
            }
            let mut lat = std::mem::take(&mut *c.lat.lock().unwrap());
            window.extend_from_slice(&lat);
            println!(
                "t={:>4}s links up {:>6}  pairs {:>6}/{pairs}  reconnects {:>6}  sent {:>7} got {:>7}  \
                 p50 {:.2} ms  p99 {:.2} ms  max {:.2} ms",
                t0.elapsed().as_secs(),
                c.up.load(Ordering::Relaxed),
                paired,
                c.reconnects.load(Ordering::Relaxed),
                c.sent.load(Ordering::Relaxed),
                c.got.load(Ordering::Relaxed),
                pct(&mut lat, 0.5),
                pct(&mut lat, 0.99),
                lat.last().map(|v| *v as f64 / 1000.0).unwrap_or(0.0),
            );
        }
        let _ = spawner.await;
        println!(
            "SUMMARY all {pairs} pairs up at {:?} s; whole run p50 {:.2} ms p99 {:.2} ms over {} messages",
            all_up_at,
            pct(&mut window, 0.5),
            pct(&mut window, 0.99),
            window.len()
        );
    });
}
