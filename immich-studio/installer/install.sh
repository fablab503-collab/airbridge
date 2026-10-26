#!/bin/bash
# Gimmich installer: turns this Mac (or Linux PC) into your always-on photo home.
#
#   curl -fsSL https://raw.githubusercontent.com/fablab503-collab/airbridge/main/immich-studio/installer/install.sh | bash
#
# What it does (it asks before anything big):
#   1. installs Docker and Tailscale if they are missing
#   2. downloads Immich + Gimmich Studio into ~/gimmich and starts them
#      (if Immich is already on this computer, it adds Gimmich Studio to that Immich instead)
#   3. keeps the computer awake and restarts it after a power cut (Mac)
#   4. gives Gimmich a private https:// address with Tailscale and shows a QR code to connect your devices
#
# Options (put them before "bash"):  GIMMICH_DIR=~/gimmich  PHOTOS_DIR=/Volumes/Big/Photos  GIMMICH_PORT=2283
#                                    GIMMICH_REF=main  SKIP_TAILSCALE=1  YES=1 (don't ask)
set -euo pipefail

REPO="fablab503-collab/airbridge"
FALLBACK_REF="claude/jolly-edison-ugpm94"   # where Gimmich lives until it is merged into main
GIMMICH_DIR="${GIMMICH_DIR:-$HOME/gimmich}"
PHOTOS_DIR="${PHOTOS_DIR:-$GIMMICH_DIR/library}"
GIMMICH_PORT="${GIMMICH_PORT:-2283}"
OS="$(uname -s)"

bold() { printf '\n\033[1m%s\033[0m\n' "$*"; }
info() { printf '  %s\n' "$*"; }
ok()   { printf '  \033[32m✓\033[0m %s\n' "$*"; }
fail() { printf '\n  \033[31m✗ %s\033[0m\n\n' "$*" >&2; exit 1; }
ask()  { [ "${YES:-}" = 1 ] && return 0; read -r -p "  $1 [Y/n] " a </dev/tty || true; [[ -z "$a" || "$a" =~ ^[Yy] ]]; }
have() { command -v "$1" >/dev/null 2>&1; }

bold "Gimmich installer"
info "Photos will be stored in: $PHOTOS_DIR"
info "Program files in:         $GIMMICH_DIR"
[ "$OS" = Darwin ] || [ "$OS" = Linux ] || fail "This installer is for macOS and Linux."

# ---------------------------------------------------------------- 1. tools ---
bold "1/4  Checking Docker and Tailscale"
if [ "$OS" = Darwin ] && ! have brew && { ! have docker || { [ -z "${SKIP_TAILSCALE:-}" ] && ! have tailscale; }; }; then
  ask "Homebrew (the Mac app installer) is needed. Install it?" || fail "Stopped."
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
  eval "$(/opt/homebrew/bin/brew shellenv 2>/dev/null || /usr/local/bin/brew shellenv)"
fi

if ! have docker; then
  ask "Docker is needed to run Gimmich. Install it?" || fail "Stopped."
  if [ "$OS" = Darwin ]; then brew install --cask docker; else curl -fsSL https://get.docker.com | sh; fi
fi
if [ "$OS" = Darwin ] && ! docker info >/dev/null 2>&1; then
  info "Starting Docker Desktop. If it asks, accept its terms and finish its welcome screen."
  open -a Docker
fi
for _ in $(seq 1 120); do docker info >/dev/null 2>&1 && break; sleep 3; done
docker info >/dev/null 2>&1 || fail "Docker didn't start. Open Docker Desktop, then run this installer again."
docker compose version >/dev/null 2>&1 || fail "Docker Compose is missing. Update Docker and try again."
ok "Docker is running"

if [ -z "${SKIP_TAILSCALE:-}" ] && ! have tailscale; then
  if ask "Tailscale gives Gimmich a private https:// address for your phone and laptop. Install it?"; then
    if [ "$OS" = Darwin ]; then
      brew install tailscale && sudo brew services start tailscale
    else
      curl -fsSL https://tailscale.com/install.sh | sh
    fi
  else
    SKIP_TAILSCALE=1
  fi
fi

# -------------------------------------------- Immich already on this computer? ---
# A second Immich would take over the first one's containers, so Gimmich is added to the Immich
# that is already here instead: same photos, albums, people and logins.
EXISTING_DIR="" EXISTING_FILES="" EXISTING_PROJECT=""
found="$(docker ps -a --filter label=com.docker.compose.service=immich-server \
  --format '{{.Label "com.docker.compose.project.working_dir"}}|{{.Label "com.docker.compose.project.config_files"}}|{{.Label "com.docker.compose.project"}}' | head -1)"
if [ -n "$found" ]; then
  IFS='|' read -r EXISTING_DIR EXISTING_FILES EXISTING_PROJECT <<<"$found"
  if [ -f "$EXISTING_DIR/.env.gimmich" ]; then
    GIMMICH_DIR="$EXISTING_DIR"           # installed by this installer before: just update it
    EXISTING_DIR=""
  else
    bold "Found your Immich"
    info "Folder: $EXISTING_DIR"
    info "Gimmich Studio will be added to it. Your photos, albums, people and logins stay as they are,"
    info "and Immich keeps its address (port $GIMMICH_PORT); Immich itself is not updated or changed."
    ask "Add Gimmich Studio to this Immich?" || fail "Stopped. Nothing was changed."
    GIMMICH_DIR="$EXISTING_DIR"
  fi
elif (echo >"/dev/tcp/127.0.0.1/$GIMMICH_PORT") 2>/dev/null; then
  # something else (maybe an Immich not run by Docker) already answers on that port
  for p in $(seq $((GIMMICH_PORT + 1)) $((GIMMICH_PORT + 20))); do
    (echo >"/dev/tcp/127.0.0.1/$p") 2>/dev/null || { info "Port $GIMMICH_PORT is busy on this computer; Gimmich will use $p."; GIMMICH_PORT="$p"; break; }
  done
fi

# --------------------------------------------------------- 2. download & run ---
bold "2/4  Downloading Gimmich Studio${EXISTING_DIR:+ (keeping your Immich)}"
cd "$GIMMICH_DIR" 2>/dev/null || { mkdir -p "$GIMMICH_DIR" && cd "$GIMMICH_DIR"; }
if [ -z "$EXISTING_DIR" ]; then
  mkdir -p "$PHOTOS_DIR"
  curl -fsSL -o docker-compose.yml https://github.com/immich-app/immich/releases/latest/download/docker-compose.yml
  if [ ! -f .env ]; then  # keep the existing password on re-install / update
    curl -fsSL -o .env https://github.com/immich-app/immich/releases/latest/download/example.env
    PASS="$(LC_ALL=C tr -dc 'A-Za-z0-9' </dev/urandom | head -c 24 || true)"
    sed -i.bak -e "s|^UPLOAD_LOCATION=.*|UPLOAD_LOCATION=$PHOTOS_DIR|" -e "s|^DB_PASSWORD=.*|DB_PASSWORD=$PASS|" .env && rm -f .env.bak
  fi
fi
echo "GIMMICH_PORT=$GIMMICH_PORT" > .env.gimmich

fetch_studio() {
  local ref="$1" tmp
  tmp="$(mktemp -d)"
  curl -fsSL "https://codeload.github.com/$REPO/tar.gz/refs/heads/$ref" | tar xz -C "$tmp" 2>/dev/null || return 1
  local src
  src="$(find "$tmp" -mindepth 2 -maxdepth 2 -type d -name immich-studio | head -1)"
  [ -n "$src" ] || return 1
  rm -rf immich-studio && mv "$src" . && rm -rf "$tmp"
}
fetch_studio "${GIMMICH_REF:-main}" || fetch_studio "$FALLBACK_REF" || fail "Couldn't download Gimmich Studio."
ok "Downloaded"

if [ -n "$EXISTING_DIR" ]; then
  # your own Immich files, exactly as they are, plus the Studio on top
  COMPOSE=(docker compose -p "$EXISTING_PROJECT")
  IFS=',' read -r -a files <<<"$EXISTING_FILES"
  for f in "${files[@]}"; do COMPOSE+=(-f "$f"); done
  COMPOSE+=(-f immich-studio/docker-compose.studio.yml)
  [ -f .env ] && COMPOSE+=(--env-file .env)
  COMPOSE+=(--env-file .env.gimmich)
  info "Starting (the first time builds the Studio: 15-30 minutes)..."
  "${COMPOSE[@]}" up -d --build
else
  COMPOSE=(docker compose -f docker-compose.yml -f immich-studio/docker-compose.studio.yml --env-file .env --env-file .env.gimmich)
  info "Starting (the first time downloads about 3 GB and builds the Studio: 15-30 minutes)..."
  "${COMPOSE[@]}" up -d --build --remove-orphans
fi
for _ in $(seq 1 200); do curl -fs "http://localhost:$GIMMICH_PORT/api/server/ping" >/dev/null 2>&1 && break; sleep 3; done
curl -fs "http://localhost:$GIMMICH_PORT/api/server/ping" >/dev/null 2>&1 || fail "Gimmich didn't start. See: cd $GIMMICH_DIR && ${COMPOSE[*]} logs"
ok "Gimmich is running at http://localhost:$GIMMICH_PORT"

# ------------------------------------------------------------ 3. always on ---
bold "3/4  Keeping this computer ready"
if [ "$OS" = Darwin ]; then
  if ask "Keep this Mac awake and switch it back on after a power cut? (needs your password)"; then
    sudo pmset -a sleep 0 disksleep 0 autorestart 1 && ok "Sleep off, restart after power cut on"
  fi
  osascript -e 'tell application "System Events" to make login item at end with properties {path:"/Applications/Docker.app", hidden:true}' \
    >/dev/null 2>&1 && ok "Docker starts at login (Gimmich starts with it)"
else
  have systemctl && sudo systemctl enable docker >/dev/null 2>&1 && ok "Docker starts at boot (Gimmich starts with it)"
fi

# ------------------------------------------------------------- 4. address ---
bold "4/4  Your Gimmich address"
ADDRESS="http://$( (ipconfig getifaddr en0 || ipconfig getifaddr en1 || hostname -I | awk '{print $1}') 2>/dev/null | head -1):$GIMMICH_PORT"
if [ -z "${SKIP_TAILSCALE:-}" ] && have tailscale; then
  if ! tailscale status >/dev/null 2>&1; then
    info "Log in to Tailscale in the browser window that opens (use the same account on your phone later)."
    sudo tailscale up || tailscale up || true
  fi
  if tailscale serve --bg "$GIMMICH_PORT" >/dev/null 2>&1 || sudo tailscale serve --bg "$GIMMICH_PORT" >/dev/null 2>&1; then
    # this computer's name comes first in the status ("Self"), before the other devices
    NAME="$(tailscale status --json 2>/dev/null | grep -o '"DNSName": *"[^"]*"' | head -1 | sed 's/.*"\([^"]*\)"$/\1/; s/\.$//')"
    [ -n "$NAME" ] && ADDRESS="https://$NAME"
  else
    info "Tailscale HTTPS is off. Turn on 'MagicDNS' and 'HTTPS Certificates' at https://login.tailscale.com/admin/dns"
    info "then run:  tailscale serve --bg $GIMMICH_PORT"
  fi
fi
echo "$ADDRESS" > "$GIMMICH_DIR/ADDRESS.txt"
printf 'GIMMICH_PORT=%s\nGIMMICH_PUBLIC_URL=%s\n' "$GIMMICH_PORT" "$ADDRESS" > .env.gimmich
"${COMPOSE[@]}" up -d immich-studio >/dev/null 2>&1 || true  # Studio's "Connect a device" page shows this address

bold "Done! 🎉"
info "Gimmich:          $ADDRESS"
info "Connect a device: $ADDRESS/studio/connect/  (open it here and scan the QR code)"
if have qrencode; then qrencode -t ANSIUTF8 "$ADDRESS/studio/connect/"; fi
info ""
info "First time: open the address, create your admin account. Update later by running this installer again."
[ "$OS" = Darwin ] && open "http://localhost:$GIMMICH_PORT/studio/connect/" 2>/dev/null || true
