#!/bin/sh
# Qanuvi relay kit: sets up your own relay on this Linux server with one command.
#
#   sudo ./setup.sh                       a relay on port 47443 with its own certificate (no domain needed)
#   sudo ./setup.sh --host 203.0.113.5    say the address devices use (behind NAT, or a name)
#   sudo ./setup.sh --port 47500          another port
#   sudo ./setup.sh --cert fullchain.pem --key key.pem   a public certificate you already have (renewals picked up)
#   sudo ./setup.sh --cap-per-day 2048    a daily share per computer in MB (slowed after it); none by default
#   sudo ./setup.sh --remove [--purge]    stop and remove it (--purge also removes its data and user)
#
# Run it again with a newer qanuvi-relay beside it to update; the address stays the same. Nothing else on the server
# is touched: no web server, no port 80 or 443, no domain.
set -eu

NAME=qanuvi-relay
DATA=/var/lib/$NAME
BIN=/usr/local/bin/$NAME
UNIT=/etc/systemd/system/$NAME.service
CERT_UNIT=/etc/systemd/system/$NAME-cert.service
CERT_PATH=/etc/systemd/system/$NAME-cert.path
HERE=$(cd "$(dirname "$0")" && pwd)

HOST="" PORT=47443 CERT="" KEY="" CAP="" REMOVE=0 PURGE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --host) HOST=${2:-}; shift 2 ;;
    --port) PORT=${2:-}; shift 2 ;;
    --cert) CERT=${2:-}; shift 2 ;;
    --key) KEY=${2:-}; shift 2 ;;
    --cap-per-day) CAP=${2:-}; shift 2 ;;
    --remove) REMOVE=1; shift ;;
    --purge) PURGE=1; shift ;;
    -h|--help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unknown option: $1 (see ./setup.sh --help)"; exit 1 ;;
  esac
done

stop() { echo "Stopped: $*"; exit 1; }
say() { echo "- $*"; }

[ "$(uname -s)" = Linux ] || stop "the relay kit runs on Linux."
[ "$(id -u)" = 0 ] || stop "run it as root: sudo ./setup.sh"
command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ] || stop "this server has no systemd."

firewall_open() {
  if command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | grep -q "Status: active"; then
    ufw allow "$1/tcp" >/dev/null && say "firewall (ufw): port $1 open"
  elif command -v firewall-cmd >/dev/null 2>&1 && firewall-cmd --state >/dev/null 2>&1; then
    firewall-cmd --quiet --permanent --add-port="$1/tcp" && firewall-cmd --quiet --reload && say "firewall (firewalld): port $1 open"
  else
    say "no active ufw or firewalld found; if this server has another firewall, open TCP port $1 there"
  fi
}

firewall_close() {
  if command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | grep -q "Status: active"; then
    ufw delete allow "$1/tcp" >/dev/null 2>&1 || true
  elif command -v firewall-cmd >/dev/null 2>&1 && firewall-cmd --state >/dev/null 2>&1; then
    firewall-cmd --quiet --permanent --remove-port="$1/tcp" 2>/dev/null && firewall-cmd --quiet --reload || true
  fi
}

# The port the installed relay uses (to close it again on --remove).
installed_port() { sed -n 's/.*--tls-listen [^ ]*:\([0-9][0-9]*\).*/\1/p' "$UNIT" 2>/dev/null | head -n 1; }

if [ "$REMOVE" = 1 ]; then
  OLD=$(installed_port)
  systemctl disable --now "$NAME" "$NAME-cert.path" >/dev/null 2>&1 || true
  rm -f "$UNIT" "$CERT_UNIT" "$CERT_PATH" "$BIN" "$BIN.prev"
  systemctl daemon-reload
  [ -n "$OLD" ] && firewall_close "$OLD"
  if [ "$PURGE" = 1 ]; then
    rm -rf "$DATA"
    id "$NAME" >/dev/null 2>&1 && userdel "$NAME" 2>/dev/null || true
    echo "The relay is removed, with its data and its certificate (its address will not work again)."
  else
    echo "The relay is removed. Its data and certificate are kept in $DATA (setup again = the same address)."
  fi
  exit 0
fi

[ -x "$HERE/$NAME" ] || stop "$NAME is not beside this script (unpack the whole kit and run it from there)."
case "$PORT" in ''|*[!0-9]*) stop "--port needs a number." ;; esac
[ "$PORT" -ge 1 ] && [ "$PORT" -le 65535 ] || stop "--port must be between 1 and 65535."
[ -z "$CAP" ] || case "$CAP" in *[!0-9]*) stop "--cap-per-day needs a number of MB." ;; esac
if [ -n "$CERT$KEY" ]; then
  [ -n "$CERT" ] && [ -n "$KEY" ] || stop "give both --cert and --key, or neither."
  [ -f "$CERT" ] && [ -f "$KEY" ] || stop "the certificate or key file is not there."
  CERT=$(readlink -f "$CERT"); KEY=$(readlink -f "$KEY")
fi

# The port must be free, unless it is our own relay holding it (an update).
if ! systemctl is-active --quiet "$NAME" || [ "$(installed_port)" != "$PORT" ]; then
  if command -v ss >/dev/null 2>&1 && ss -Hltn "sport = :$PORT" 2>/dev/null | grep -q .; then
    stop "port $PORT is already in use here; pick another with --port."
  fi
fi

# The address devices use: given, or this server's own address from its routes (nothing is asked of anyone outside).
is_private() {
  case "$1" in
    10.*|192.168.*|127.*|169.254.*) return 0 ;;
    172.1[6-9].*|172.2[0-9].*|172.3[01].*) return 0 ;;
    100.6[4-9].*|100.[7-9][0-9].*|100.1[01][0-9].*|100.12[0-7].*) return 0 ;;
    fc*|fd*|fe80*) return 0 ;;
  esac
  return 1
}
BEHIND_NAT=0
if [ -z "$HOST" ]; then
  HOST=$(ip -4 route get 1.1.1.1 2>/dev/null | sed -n 's/.* src \([0-9.]*\).*/\1/p' | head -n 1)
  if [ -z "$HOST" ] || is_private "$HOST"; then
    V6=$(ip -6 route get 2606:4700:4700::1111 2>/dev/null | sed -n 's/.* src \([0-9a-fA-F:]*\).*/\1/p' | head -n 1)
    if [ -n "$V6" ] && ! is_private "$V6"; then HOST=$V6; else BEHIND_NAT=1; fi
  fi
  [ -n "$HOST" ] || stop "this server's address was not found; give it with --host <IP or name>."
fi
case "$HOST" in *:*) [ "${HOST#\[}" = "$HOST" ] && HOST_URL="[$HOST]" || HOST_URL=$HOST ;; *) HOST_URL=$HOST ;; esac

echo "Setting up the Qanuvi relay on port $PORT..."
id "$NAME" >/dev/null 2>&1 || useradd --system --home "$DATA" --shell /usr/sbin/nologin "$NAME"
install -d -o "$NAME" -g "$NAME" -m 0750 "$DATA"
[ -f "$BIN" ] && cp -p "$BIN" "$BIN.prev"
install -m 0755 "$HERE/$NAME" "$BIN"
say "program: $BIN$( [ -f "$BIN.prev" ] && echo " (the one before kept as $NAME.prev)")"

FLAGS="--tls-listen 0.0.0.0:$PORT --data $DATA"
[ -n "$CAP" ] && FLAGS="$FLAGS --cap-per-day $CAP"
AFTER=network-online.target
if [ -n "$CERT" ]; then
  # A certificate the server already has: copied to the relay's folder now and whenever it is renewed.
  cat > "$CERT_UNIT" <<EOF
[Unit]
Description=Copy the certificate for the Qanuvi relay to where it can read it

[Service]
Type=oneshot
ExecStart=/usr/bin/install -D -o $NAME -g $NAME -m 0644 $CERT $DATA/tls/fullchain.pem
ExecStart=/usr/bin/install -D -o $NAME -g $NAME -m 0600 $KEY $DATA/tls/key.pem
EOF
  cat > "$CERT_PATH" <<EOF
[Unit]
Description=Watch the Qanuvi relay's certificate for renewals

[Path]
PathChanged=$CERT
PathChanged=$KEY
Unit=$NAME-cert.service

[Install]
WantedBy=multi-user.target
EOF
  FLAGS="$FLAGS --cert $DATA/tls/fullchain.pem --key $DATA/tls/key.pem"
  AFTER="$AFTER $NAME-cert.service"
  systemctl daemon-reload
  systemctl start "$NAME-cert.service"
  systemctl enable --now "$NAME-cert.path" >/dev/null 2>&1
  ADDRESS="wss://$HOST_URL:$PORT"
  say "certificate: yours ($CERT), renewals picked up"
else
  rm -f "$CERT_UNIT" "$CERT_PATH"
  systemctl disable --now "$NAME-cert.path" >/dev/null 2>&1 || true
  FLAGS="$FLAGS --tls-self"
  ADDRESS=$("$BIN" address --host "$HOST" --port "$PORT" --data "$DATA") || stop "the relay could not make its certificate."
  chown -R "$NAME:$NAME" "$DATA"
  say "certificate: its own (kept in $DATA/tls-self; devices check it by the pin in the address)"
fi

cat > "$UNIT" <<EOF
[Unit]
Description=Qanuvi relay (passes sealed messages between Qanuvi devices; it cannot read them)
After=$AFTER
Wants=$AFTER

[Service]
User=$NAME
Group=$NAME
ExecStart=$BIN $FLAGS
Restart=always
RestartSec=3
LimitNOFILE=262144
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
ReadWritePaths=$DATA

[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable "$NAME" >/dev/null 2>&1
systemctl restart "$NAME"
say "service: $NAME (log: journalctl -u $NAME)"
firewall_open "$PORT"

sleep 2
systemctl is-active --quiet "$NAME" || stop "the relay did not start; see journalctl -u $NAME"
if "$BIN" check "$ADDRESS" >/dev/null 2>&1; then
  say "check: the relay answers at its address"
elif [ -z "$CERT" ] && "$BIN" check "wss://127.0.0.1:$PORT#${ADDRESS#*#}" >/dev/null 2>&1; then
  say "check: the relay answers here, but not at $HOST from this server; check the address (--host)"
else
  say "check: the relay does not answer yet; see journalctl -u $NAME"
fi

echo
echo "Your relay:"
echo
echo "  $ADDRESS"
echo
echo "In Qanuvi: Settings, Reach this computer from anywhere, Relay · Change, Your own relay, paste it."
echo "Qanuvi tests it when it is pasted; if it is not reached from there, open TCP port $PORT in your hosting"
echo "panel's or cloud provider's firewall too (they sit in front of this server)."
if [ "$BEHIND_NAT" = 1 ]; then
  echo
  echo "Note: this server has only a private address ($HOST), so it is behind NAT. Forward TCP port $PORT on the"
  echo "router to it and run setup again with --host <your public IP or a dynamic DNS name>. Without a public"
  echo "address (CGNAT) a relay cannot be reached here; Qanuvi's relay or a free relay works instead."
fi
