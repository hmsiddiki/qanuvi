# Running qanuvi-relay on a Linux server

The relay passes sealed messages between Qanuvi machines and the devices that reach them. It cannot read them. One
program serves two roads at once:

- **through a TLS proxy** (nginx, Caddy) on 443: `--listen 127.0.0.1:47300`, with the proxy passing WebSocket
  upgrades to it;
- **on its own TLS port**: `--tls-listen 0.0.0.0:47443 --cert fullchain.pem --key key.pem`. With
  `--public-direct wss://your.host:47443` apps learn this port and use it first; 443 stays their fallback for
  networks that block other ports.

A relay without a proxy can run the TLS port alone. A renewed certificate on disk is picked up within 10 minutes for
new links (open links keep theirs).

## Files here (as used on relay.qanuvi.com)

- `qanuvi-relay.service`: the relay as its own user, both roads, 262,144 open files (one per link).
- `qanuvi-relay-cert.service` + `qanuvi-relay-cert.path`: ISPConfig keeps the Let's Encrypt key readable by root only;
  these copy the certificate and key into `/var/lib/qanuvi-relay/tls/` (owned by the relay's user, key 0600) at start
  and whenever ISPConfig renews them. Nothing in ISPConfig or nginx changes. Change the paths for another host.

## Setup

```sh
useradd --system --home /var/lib/qanuvi-relay --shell /usr/sbin/nologin qanuvi-relay
install -d -o qanuvi-relay -g qanuvi-relay /var/lib/qanuvi-relay
install -m 0755 qanuvi-relay /usr/local/bin/qanuvi-relay
cp qanuvi-relay.service qanuvi-relay-cert.service qanuvi-relay-cert.path /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now qanuvi-relay-cert.path
systemctl start qanuvi-relay-cert.service
systemctl enable --now qanuvi-relay
ufw allow 47443/tcp
```

Log (why each link opened and ended): `journalctl -u qanuvi-relay`.
