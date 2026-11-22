# Your own Qanuvi relay

A relay passes sealed messages between your Qanuvi computers and the devices that reach them. It cannot read them:
pairing uses the code on your screen, and everything after that is sealed end to end. Qanuvi's own relay is used by
default; run your own when you want your traffic on your own server.

## What you need

- A Linux server with systemd (any VPS, or a hosting server that also runs websites).
- An address the devices can reach: a public IP is enough. It can be shared with other services; no domain, no
  web server, no port 80 or 443 needed. A home server works with a forwarded port (and a dynamic DNS name if the
  IP changes). A connection with no public address at all (CGNAT) cannot host a relay.

## Set it up

```sh
tar xzf qanuvi-relay-kit-linux-x86_64.tar.gz
cd qanuvi-relay-kit
sudo ./setup.sh
```

It prints the relay's address, like `wss://203.0.113.5:47443#pin=...`. Paste the whole line in Qanuvi: Settings,
Reach this computer from anywhere, Relay · Change, Your own relay. The `#pin=` part is how your devices know it is
your relay (it stands for the relay's own certificate), so keep it.

Options: `--host <IP or name>` (the address devices use, when the one found is wrong), `--port <n>` (47443 by
default), `--cert <file> --key <file>` (a public certificate you already have; then the address has no pin),
`--cap-per-day <MB>` (a daily share per computer, slowed after it). `sudo ./setup.sh --help` lists them.

If Qanuvi cannot reach it, open the port in your hosting panel's or cloud provider's firewall as well.

## Update, remove

- Update: unpack a newer kit and run `sudo ./setup.sh` again. The address stays the same.
- Remove: `sudo ./setup.sh --remove` (the data and certificate are kept, so setting up again gives the same address),
  or `--remove --purge` to remove everything.
- Log: `journalctl -u qanuvi-relay`.
