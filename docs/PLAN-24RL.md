# 24rl: a relay for 50-100k devices at once (proposal, 2026-11-21)

Status: PART 1 DONE (R1-R5, deployed on basevm376, in 0.1.35/0.1.36). PART 2a DONE (in 0.1.37, driven on both machines). PART 3 DONE (0.1.38, cap deployed). RELAY KIT BUILT (K1-K6, unpublished; setup.sh as root not driven). Earlier: 2a AGREED and BUILT
(not driven live yet). Parts 2, 3 and the relay kit are outlines; each gets its own detailed write-up before it is built.
Order: 2a, then 3 (free relays + fair use), then the kit (alongside or after 3); part 2 (direct links) goes with sl1.

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

**Part 2a (agreed, next): nobody types a relay address.** Found by the owner after part 1: today the address is
typed in three boxes (RemoteAccessPanel "Turn on", MachineServersView and MastersView pairing), the default only a
placeholder. To build:
- A1. The default is built in: "Turn on" goes to Qanuvi's relay (wss://relay.qanuvi.com) with nothing typed; its own
  port (47443) and the 443 fallback are learned and kept as since 0.1.35 (relay-roads.json). The master has the
  default too (an empty relay in relay.json means Qanuvi's), so the terminal and older records need nothing either.
- A2. A relay choice, folded away as one line "Relay: Qanuvi (default) · Change": Qanuvi relay (default); Your own
  relay (the only address box anywhere). The "Free relay" choice joins it in part 3.
- A3. Pairing follows the machine's relay: the other device asks only for the code and assumes Qanuvi's relay; a
  machine on another relay shows that relay next to the code, and the other device fills it in from there (the
  code card says "on relay X" and the pairing screen has the same folded "Relay · Change").
- Validate: turning on with nothing typed connects (Windows + laptop, own port); pairing a device with only the code;
  a custom relay set, shown next to the code, paired with it; older records with an address keep it.

**Relay kit (outline, with or after part 3): run your own relay.** A small download on qanuvi.com: the same
qanuvi-relay program (about 2 MB, Linux first, Windows later), a one-command setup script from deploy/relay/ (service,
certificate copy or its own certificate, firewall port) and a short guide; it prints the address to paste into "Your
own relay". The program already has what it needs (own TLS port, no nginx, renewals picked up).

**Part 2 (outline): keep traffic off the relay.** Direct links when both machines are on the same network (the
laptop and Home PC today), SSH for VPS masters as now; the relay only carries what cannot go directly. This is also
what makes sl1 (slow switch over the relay) mostly go away for home use.

**Part 3 (outline): free relays + fair use.** Qanuvi's own relay plus volunteer relays; a volunteer relay is used only
after the user says yes. A daily fallback cap per user on relayed traffic (idle links are not counted). The relay
program for volunteers is the same binary as ours (part 1 makes it run with its own TLS and no nginx). Adds "A free
relay" to part 2a's choice: a list fetched from qanuvi.com, one used only after the user picks it and says yes.

## Part 3 in detail (2026-11-22; owner: "check what you need, then go forward", defaults below taken)

Why: Qanuvi is free, so one relay paid by the owner must not carry everybody's traffic for ever. Two things help:
other people's relays (volunteers), and a fair-use cap so a few heavy users cannot fill Qanuvi's relay. Messages stay
sealed end to end on every relay; what a relay does see is who connects, when, from which address and how much.
That is why a volunteer relay is only ever used after the user says yes.

- F1. **The free relay list.** relays.json on the update channel beside latest.json, with relays.json.sig signed
  by the update key (the same minisign check as updates), so nobody can slip a relay into it. Each entry: address,
  name, who runs it, region, note. The owner adds relays by hand after checking them. The app fetches it when the
  relay choice is opened (and keeps the last good copy). Shipped empty until the first volunteer.
- F2. **"A free relay" in the relay choice** (third option beside Qanuvi relay / Your own relay). Lists the relays
  from F1, each tested on the spot (connects, ping time), best first. Picking one shows what that relay can and
  cannot see and asks "Use <name>'s relay?"; only a yes switches. Pairing works as for your own relay (A3).
- F3. **Fair use on a relay.** The relay counts relayed bytes per computer (master id) per UTC day, both ways;
  pings and pairing do not count. Over the cap the computer's links are slowed, not cut, until midnight UTC.
  Relay options --cap-per-day <MB> and --slow-to <KB/s>; none given = no cap (volunteers choose). Qanuvi's relay:
  2 GB a day, 256 KB/s after. The relay tells the computer its use (a Usage message), near and over the cap.
- F4. **What the user sees.** Remote access: "Today through the relay: 120 MB of 2 GB". Over the cap: "Slowed
  until <time> (your daily share of this relay is used)", with "a free relay" and "your own relay" offered.
  Nothing switches by itself.
- F5. **When the relay is down.** After a minute without a link, Remote access says so and offers the free relays
  (same yes as F2). No automatic switch (paired devices would need pairing again on the new relay).
- Validate: list signature (good, changed, missing); the free relay choice against local relays; the cap counted
  and slowed in a relay test (small cap); the use and over lines in the window; the down offer.

Decisions taken (owner said go forward): cap 2 GB/day per computer, slowed to 256 KB/s; slow down, not stop; the
list on the update channel, edited by the owner by hand; built and tested with local relays and shipped with an
empty list; relay kit after. Deploying the cap on basevm376 and publishing still ask first.

## Relay kit in detail (2026-11-22; revised after the owner: no domain, no port 80)

Why: "Your own relay" is only a box today. The kit makes it one download and one command on the servers people
really have: a bare VPS with only an IP address (no domain, no hosting), or a full hosting server like basevm376
(nginx/ISPConfig own 80 and 443, nothing there may be touched). A domain, port 80, certbot and nginx are never
needed. It is also what a volunteer runs before asking to be on the free list (part 3).

Why no real certificate is needed: pairing is SPAKE2 on the code and every message is sealed end to end
(relay.rs), so the relay's TLS never protected the messages; it only hides who talks to the relay from the network
and looks like ordinary TLS to firewalls. A certificate the relay makes for itself does that just as well, as long as
the apps know it is the right one: its fingerprint travels inside the address.

- K1. **The relay makes its own certificate.** `--tls-self` (with `--tls-listen`): at first start the relay makes
  a key and a certificate (rcgen, already in the build for tests; 20 years, ECDSA P-256) in its data folder and
  keeps them, so the fingerprint never changes. `qanuvi-relay address --host 203.0.113.5 --port 47443` prints the
  address: `wss://203.0.113.5:47443#pin=<sha256 of the certificate, base64url>`. Works with an IP or a name.
- K2. **Apps pin it.** An address with `#pin=` is checked by that fingerprint only (no certificate authority, no
  name check, any IP); without it, as today (public certificates). The pin travels with the address everywhere it
  already goes: Your own relay box, relay.json, the code card and pairing (A3), the free list (F1). The `#pin` part
  is never sent to the relay. A wrong certificate = "this is not the relay you set up" and no link.
- K3. **setup.sh, one command.** `sudo ./setup.sh` on the server. Checks first (Linux, systemd, root, the port
  free) and stops with a plain reason if not. Then: the relay's own user and folder, the program in /usr/local/bin
  (old kept as .prev), the service unit (same hardening as ours, own port only, `--tls-self`), the firewall port
  opened (ufw or firewalld when active, else a line saying which port to open, and a reminder about a hosting
  panel's or cloud provider's firewall). Host: the server's own public address found from its network setup;
  `--host <IP or name>` to say it (a NAT or a name). Options: `--port 47443` (default), `--cert/--key` for those who
  want a public certificate they already have (then no pin; watched for renewals as on basevm376), `--cap-per-day
  <MB>` (none by default), `--remove` (stops and removes it, data kept unless `--purge`). Run again with a newer
  program = update; the certificate and the address stay the same.
- K4. **It checks itself.** `qanuvi-relay check <address>` connects like an app (TLS with the pin, the relay's
  hello, a ping) and prints OK or why not. setup.sh runs it at the end, then prints the address and where to paste
  it (Settings, Reach this computer from anywhere, Relay · Change, Your own relay). If the check fails from outside
  only (a cloud firewall), the app's own test says so when it is pasted.
- K5. **The download.** `qanuvi-relay-kit-<version>-linux-x86_64.tar.gz` (program + setup.sh + short README) with
  .sha256 and .sig (update key), built on the laptop by `scripts/build-relay-kit.sh`, put on the update channel
  beside latest.json (moves to qanuvi.com with the channel). Linux x86_64 first; ARM and Windows later.
- K6. **In the app.** Under the "Your own relay" box: "Run your own on any Linux server: download the relay kit,
  run sudo ./setup.sh, paste the address it prints." The box takes the pinned address.
- Validate: relay tests: a self-made certificate kept across restarts; the right pin connects, a wrong pin and a
  missing pin are refused; `check` OK and refused. On the laptop (Zorin, systemd, LAN IP only, no domain): setup,
  check OK, a scratch master on This PC turned on with the pinned address and a device paired through it, rerun =
  update with the same address, --remove leaves nothing running. The kit's .sig verified after unpacking.

Defaults taken (owner's push-back = go): IP and self-made certificate is the normal road; public certificate only
when given; no cap unless set; Linux x86_64 first; the kit on the update channel. Deploying or publishing asks.

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
  example `"direct": "wss://relay.qanuvi.com:47443"`, from a flag `--public-direct`). Old apps ignore the field
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

## Owner answers (2026-11-21)

1. Port: not 8443 (many services use it to avoid 443); an unusual one. Chosen: **47443** (free on basevm376, not in
   /etc/services, next to the relay's internal 47300).
2. Certificate copy by a root path unit + the relay swapping it in by itself: yes.
3. nginx left as it is; the own port carries the load: yes.
4. R4 targets as written: yes.
