# 24rl: a relay for 50-100k devices at once (proposal, 2026-11-21)

Status: PROPOSAL, part 1 written in detail, waiting for the owner's answers (end of this file). Parts 2 and 3 are
outlines only; each gets its own detailed write-up before it is built.

## The goal in one line

One relay program that holds 50,000 to 100,000 devices connected at once (almost all of them idle), costs nearly
nothing per idle device, and runs both behind nginx (as today) and on its own TLS port, so it is built once and
never touched again when it moves to its own VPS.

## Where it stands today (measured 2026-11-21)

- **One thread per link, and every thread wakes every 50 ms** (relay_server.rs serve_conn: 50 ms read wait, then a
  look at its outbox). 1,000 idle links = 20,000 wake-ups a second for nothing.
- **Hard ceiling about 9,000 links**: the service has TasksMax 9440 on basevm376, and each link is a thread.
- **Through nginx the ceiling is lower still, about 1,500 links**: nginx has worker_connections 768 x 4 workers =
  3,072 for every site on the server, and each relay link takes 2 (client side + relay side). nginx's main config
  is not ours to change (an ISPConfig update may rewrite it), so the 443 path stays a fallback, not the main road.
- **Memory per link is large**: tungstenite's default buffers are 128 KB to read + 128 KB to write; 100k links would
  want about 25 GB. basevm376 has 8 GB (5.2 GB free).
- **Each link's outbox has no limit**: a slow reader lets messages pile up in the relay's memory without end.
- **The certificate key cannot be read by the relay**: /var/www/relay.qanuvi.com/ssl/relay.qanuvi.com-le.key is
  root-only (0600), and the relay runs as its own user (qanuvi-relay).
- **A new port needs the firewall**: UFW is active (21, 22, 25, 53, 80, 110, 143, 443, 465, 587, 993 ... open).
- The messages themselves (relay.rs Msg, sealed end to end, long lines in parts since rl4) are fine and stay as they
  are, so 0.1.34 apps keep working with the new relay without an update.

## The parts

**Part 1 (this file, in detail): the relay rebuilt + its own TLS port + apps that use it.** Event-driven relay,
small memory per link, own port beside nginx, apps try the own port first and fall back to 443, a 100k-link load
test on the laptop, then the deploy on basevm376.

**Part 2 (outline): keep traffic off the relay.** Direct links when both machines are on the same network (the
laptop and Home PC today), SSH for VPS masters as now; the relay only carries what cannot go directly. This is also
what makes sl1 (slow switch over the relay) mostly go away for home use.

**Part 3 (outline): free relays + fair use.** Qanuvi's own relay plus volunteer relays; a volunteer relay is used only
after the user says yes. A daily fallback cap per user on relayed traffic (idle links are not counted). The relay
program for volunteers is the same binary as ours (part 1 makes it run with its own TLS and no nginx).

## Part 1 in detail

### R1. Relay core rebuilt on tokio (no polling)

- relay_server.rs: one tokio task per link instead of a thread; a link's task sleeps until a message comes in, a
  message goes out, or its ping timer fires. No 50 ms waits anywhere.
- Same State (masters, sessions, slots, pairing, registered masters file) and the same handle() logic; the lock is
  held only to look up who gets a message, never while sending.
- Each link's outbox gets a limit (proposed 64 MB queued): a link that falls further behind is ended with a log line
  ("too far behind"), as a dead link is today. Its sessions get Close as now.
- Small buffers: read and write buffers start at about 4 KB and grow only for a big message (rl4 parts are at most
  1 MB each), then shrink back.
- Pings as today (every 30 s, gone after 90 s silent), but from a timer per link instead of the poll loop.
- rl3 log lines kept as they are (same words, so the log reads the same).
- tokio is already in the build (Tauri brings it); the relay uses only its network and timer parts. The server build
  (no desktop feature) stays small; the size is checked after the build.
- Files: relay_server.rs, Cargo.toml, bin/qanuvi-relay.rs.
- Validate: every existing relay test passes unchanged (they start a local relay: pair, end to end, quiet handler
  forgotten, master cut off, long lines in parts, older read-only client stays). New: 2,000 idle links on a test
  relay use under 1% of one core over 60 s (fails with the old relay); a reader that stops reading is ended at the
  outbox limit and the other links stay.

### R2. Its own TLS port beside nginx

- New flags: `--tls-listen 0.0.0.0:<port> --cert <file> --key <file>`; `--listen 127.0.0.1:47300` stays for nginx.
  Both can run at once from one process (basevm376 today); a volunteer relay can run TLS only.
- TLS by rustls (already in the build through tungstenite). The relay looks at the certificate files' change time
  every 10 minutes and swaps in a renewed certificate by itself: new links get the new one, links already open are
  not touched (no drop at renewal).
- Getting the certificate to the relay without opening up the key: a small systemd path unit (runs as root) watches
  ISPConfig's files in /var/www/relay.qanuvi.com/ssl/ and copies the .crt and .key into the relay's own folder
  (/var/lib/qanuvi-relay/tls/, owned by qanuvi-relay, key 0600) whenever ISPConfig renews them. Nothing in ISPConfig
  or nginx changes.
- The relay tells apps its own address in Welcome and in its first answer to a handler (a new optional field, for
  example `"direct": "wss://relay.qanuvi.com:8443"`, from a flag `--public-direct`). Old apps ignore the field
  (checked: Msg has no deny_unknown_fields).
- Files: relay_server.rs, relay.rs (the optional field), bin/qanuvi-relay.rs, docs (relay setup notes).
- Validate: a test relay with a test certificate takes a wss link on its TLS port and a ws link on the plain one at
  the same time and passes messages between them; a certificate replaced on disk is used for the next link while the
  first link stays open; Welcome carries the direct address and an old-style reader still reads it.

### R3. Apps try the own port first, then 443

- relay_client.rs Link::connect: when the relay has told its direct address before, try it first with a 3 s connect
  limit, then the saved address (wss://relay.qanuvi.com through nginx), as today. Today's connect has no time limit,
  so a blocked port could hang for 20 s or more; both tries get a limit.
- What worked is remembered per relay (in relay.json for the master, the handler's own settings for the window). A
  port that did not answer is not tried again for 6 hours, so a network that blocks it costs 3 s once, not on every
  reconnect.
- The relay status (Settings, Reach this computer from anywhere) says which road the link uses: "own port" or
  "through 443".
- Files: relay_client.rs, relay.rs, the relay status in the window (one component), tests.
- Validate: own port works -> used; own port blocked (test relay that does not answer on it) -> 443 within about 3 s,
  and the next reconnect goes straight to 443; a relay with no direct address -> exactly as today; pairing works on
  both roads.

### R4. Load test, 100k links (on the laptop, never on basevm376)

- A load tool (a test binary, not shipped): opens N links to a relay in pairs (master + handler, paired as in real
  use), keeps 99% idle and lets 1% send a message every few seconds; it spreads its links over several local
  addresses (127.0.0.2, 127.0.0.3 ...) so it does not run out of ports.
- Runs on the laptop (Linux like the server: 8 cores, 15 GB, open-file limit 1,048,576), relay and load tool both
  there, with TLS on (the own-port road) and without (the nginx road's relay side).
- Measured and written in the task list: relay memory per idle link, CPU while 100k sit idle (pings only), time for
  a message from handler to master at p50 and p99 with 1,000 links active, time for 100k links to come back after a
  relay restart.
- Targets (proposed): at most 30 KB of memory per idle link (100k = about 3 GB); idle CPU under 5% of one core;
  p99 under 50 ms; all 100k back within 2 minutes after a restart (apps already wait 1 s and up, rl2).
- If a target is missed, the cause is fixed in R1/R2 before the deploy.

### R5. Deploy on basevm376 (each step asks first)

- Relay build swapped into /usr/local/bin (old kept as .prev, as with rl1-rl4).
- systemd unit: the new flags, LimitNOFILE raised to 262144 (100k links need about 100k open files; 65536 today),
  TasksMax can stay (tasks no longer grow with links), plus the certificate path unit.
- Firewall: open the new port in UFW (or in the ISPConfig panel's firewall if ISPConfig manages UFW there).
- Drive live: Home PC and the laptop come back on the own port; a client told to skip the own port comes in
  through 443; the rl3 log shows both roads.
- The app side (R3) goes out in the next release (0.1.35, asks first). Until then 0.1.34 apps keep using 443, which
  works with the new relay.
- What basevm376 itself can hold is a separate number from the target: it is shared with the hosting and has about
  5 GB free, so its real ceiling is whatever R4's memory per link allows in about half of that. The 100k target is
  for the relay's own VPS later, which needs no code change.

## Questions for the owner (part 1)

1. **Port**: 8443 for the relay's own TLS port? (A common second TLS port; any port other than 443 can be blocked by
   some networks, which is what the 443 fallback is for.)
2. **Certificate**: copy ISPConfig's certificate into the relay's own folder with a root path unit, and the relay
   swaps it in by itself (no links dropped at renewal). OK, or do you prefer something else?
3. **nginx**: leave nginx as it is (about 1,500 links at most through 443, shared with every site) and let the own
   port carry the load? Raising worker_connections would mean changing nginx's main config.
4. **Targets** in R4 (30 KB per idle link, under 5% CPU idle, p99 under 50 ms): fine as they are?
