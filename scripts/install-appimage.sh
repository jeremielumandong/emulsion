#!/usr/bin/env bash
#
# install-appimage.sh — install the Emulsion AppImage into the desktop.
#
# Picks the newest AppImage in target/appimage (or builds one with --build),
# copies it to a stable path, and sets up a launcher command, a desktop entry
# with file associations, and icons. Reinstalling swaps the AppImage in place
# atomically and keeps a couple of backups.
#
# An installed Emulsion that is running is never killed silently: it may hold
# unsaved edits. The install stops and says so, unless --stop-running is given.
# Development builds (target/*/emulsion) are never touched.
#
# Your settings, recent files and API keys in the data directory are never
# touched. Only sessions/ (per-run assistant configs, regenerated on demand)
# is cleared.
#
# Usage:
#   scripts/install-appimage.sh                  # install the newest built AppImage
#   scripts/install-appimage.sh --build          # build it first
#   scripts/install-appimage.sh --force          # reinstall even if unchanged
#   scripts/install-appimage.sh --stop-running   # quit a running installed Emulsion first
#   scripts/install-appimage.sh --uninstall      # remove the AppImage and desktop integration
#   scripts/install-appimage.sh path/to/Emulsion-x.y.z-x86_64.AppImage
#
# Env:
#   EMULSION_APPIMAGE        destination (default ~/Applications/Emulsion.AppImage)
#   EMULSION_KEEP_BACKUPS    .bak-* copies to keep (default 2)
#
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_ID=app.emulsion.Emulsion

DEST="${EMULSION_APPIMAGE:-$HOME/Applications/Emulsion.AppImage}"
KEEP_BACKUPS="${EMULSION_KEEP_BACKUPS:-2}"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
DATA_DIR="$DATA_HOME/emulsion"
BIN_DIR="$HOME/.local/bin"
WRAPPER="$BIN_DIR/emulsion"
DESKTOP_FILE="$DATA_HOME/applications/$APP_ID.desktop"
ICON_ROOT="$DATA_HOME/icons/hicolor"
MARKER="X-Emulsion-Installer=install-appimage.sh"

DO_BUILD=0
FORCE=0
STOP=0
UNINSTALL=0
SOURCE=""
for arg in "$@"; do
  case "$arg" in
    --build) DO_BUILD=1 ;;
    --force) FORCE=1 ;;
    --stop-running) STOP=1 ;;
    --uninstall) UNINSTALL=1 ;;
    -h|--help) awk '/^set -euo pipefail$/ { exit } NR > 1 { sub(/^# ?/, ""); print }' "${BASH_SOURCE[0]}"; exit 0 ;;
    -*) echo "install-appimage: unknown option '$arg' (try --help)" >&2; exit 2 ;;
    *) SOURCE="$arg" ;;
  esac
done

log()  { printf '\033[36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[33mwarn:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

[[ "$DEST" == /* ]] || die "EMULSION_APPIMAGE must be an absolute path (got '$DEST')"

# ---- running instances -------------------------------------------------------
# An installed Emulsion runs from the AppImage's mount (/tmp/.mount_*), or from a
# path that no longer exists once the runtime unmounted. A development build's
# executable is a real file outside any mount, so it never matches.
installed_pids() {
  local pid exe
  for pid in $(pgrep -x emulsion 2>/dev/null || true); do
    exe=$(readlink "/proc/$pid/exe" 2>/dev/null) || continue
    if [[ -e "$exe" ]]; then
      case "$exe" in /tmp/.mount_*) echo "$pid" ;; esac
    else
      echo "$pid"
    fi
  done
}

# Assistant processes run with their working directory under sessions/.
session_pids() {
  local pid cwd
  [[ -d "$DATA_DIR/sessions" ]] || return 0
  for pid in $(pgrep -u "$(id -u)" . 2>/dev/null || true); do
    cwd=$(readlink "/proc/$pid/cwd" 2>/dev/null) || continue
    case "$cwd" in "$DATA_DIR/sessions/"*) echo "$pid" ;; esac
  done
}

stop_pids() {
  local label="$1"; shift
  local pids=("$@") pid i alive
  (( ${#pids[@]} )) || return 0
  log "Stopping $label (pid ${pids[*]})"
  for pid in "${pids[@]}"; do kill -TERM "$pid" 2>/dev/null || true; done
  for (( i = 0; i < 20; i++ )); do
    alive=0
    for pid in "${pids[@]}"; do kill -0 "$pid" 2>/dev/null && alive=1; done
    (( alive )) || return 0
    sleep 0.5
  done
  for pid in "${pids[@]}"; do
    kill -0 "$pid" 2>/dev/null || continue
    warn "$label pid $pid ignored SIGTERM for 10 s; sending SIGKILL"
    kill -KILL "$pid" 2>/dev/null || true
  done
}

guard_running() {
  local pids
  mapfile -t pids < <(installed_pids)
  (( ${#pids[@]} )) || return 0
  if (( ! STOP )); then
    die "Emulsion is running from the installed AppImage (pid ${pids[*]}). Quit it first, since it may have unsaved changes, or rerun with --stop-running."
  fi
  stop_pids "Emulsion" "${pids[@]}"
  mapfile -t pids < <(session_pids)
  stop_pids "assistant sessions" "${pids[@]}"
  for stale in /tmp/.mount_Emulsi*; do
    [[ -d "$stale" ]] && rmdir "$stale" 2>/dev/null || true
  done
}

refresh_caches() {
  command -v update-desktop-database >/dev/null && update-desktop-database "$DATA_HOME/applications" &>/dev/null || true
  command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$ICON_ROOT" &>/dev/null || true
}

# ---- uninstall ---------------------------------------------------------------
if (( UNINSTALL )); then
  guard_running
  removed=0
  if [[ -f "$DEST" ]]; then rm -f "$DEST" "$DEST".bak-*; log "Removed $DEST and its backups"; removed=1; fi
  if [[ -f "$WRAPPER" ]] && grep -q "Launch the Emulsion AppImage" "$WRAPPER"; then rm -f "$WRAPPER"; log "Removed $WRAPPER"; removed=1; fi
  if [[ -f "$DESKTOP_FILE" ]] && grep -qx "$MARKER" "$DESKTOP_FILE"; then rm -f "$DESKTOP_FILE"; log "Removed $DESKTOP_FILE"; removed=1; fi
  shopt -s nullglob
  for icon in "$ICON_ROOT"/*/apps/emulsion.png "$ICON_ROOT"/scalable/apps/emulsion.svg; do rm -f "$icon"; removed=1; done
  shopt -u nullglob
  refresh_caches
  (( removed )) || log "Nothing was installed"
  log "Your settings and recent files stay in $DATA_DIR; delete that folder yourself if you want them gone."
  exit 0
fi

# ---- source ------------------------------------------------------------------
if (( DO_BUILD )); then
  [[ -z "$SOURCE" ]] || die "give either --build or a path, not both"
  bash "$ROOT_DIR/scripts/build-appimage.sh"
fi

if [[ -z "$SOURCE" ]]; then
  shopt -s nullglob
  candidates=("$ROOT_DIR"/target/appimage/Emulsion-*.AppImage)
  shopt -u nullglob
  (( ${#candidates[@]} )) || die "no AppImage in target/appimage — run with --build, or build with scripts/build-appimage.sh"
  for f in "${candidates[@]}"; do
    [[ -z "$SOURCE" || "$f" -nt "$SOURCE" ]] && SOURCE="$f"
  done
fi
[[ -s "$SOURCE" ]] || die "$SOURCE is missing or empty"
SOURCE="$(readlink -f "$SOURCE")"
log "Source: $SOURCE ($(du -h "$SOURCE" | cut -f1))"

# Must be an AppImage for this machine, and must start.
head -c 4 "$SOURCE" | grep -q $'\x7fELF' || die "$SOURCE is not an ELF AppImage"
if ! version=$(APPIMAGE_EXTRACT_AND_RUN=1 "$SOURCE" --version 2>/dev/null); then
  die "$SOURCE does not run ($SOURCE --version failed)"
fi
log "Checked: $version"

SKIP_COPY=0
if [[ -f "$DEST" ]] && (( ! FORCE )) && cmp -s "$SOURCE" "$DEST"; then
  log "$DEST is already this build; skipping the copy (--force to reinstall)"
  SKIP_COPY=1
fi

# Checked only now, so a bad source never costs anyone a running session.
(( SKIP_COPY )) || guard_running

# ---- cache -------------------------------------------------------------------
# Only regenerated state, by allowlist. settings.json and recent.json are user
# data and are never removed here.
if [[ -d "$DATA_DIR/sessions" ]]; then
  rm -rf "${DATA_DIR:?}/sessions"
  log "Cleared stale assistant sessions"
fi

# ---- install -----------------------------------------------------------------
mkdir -p "$(dirname "$DEST")"
if (( ! SKIP_COPY )); then
  if [[ -f "$DEST" ]]; then
    backup="$DEST.bak-$(date +%Y%m%d)"
    [[ -e "$backup" ]] && backup="$backup-$(date +%H%M%S)"
    mv -f "$DEST" "$backup"
    log "Backed up the previous install to $(basename "$backup")"
    mapfile -t backups < <(ls -1t "$DEST".bak-* 2>/dev/null || true)
    for (( i = KEEP_BACKUPS; i < ${#backups[@]}; i++ )); do
      rm -f "${backups[i]}"
    done
  fi
  # Copy beside the destination, then rename: atomic, never half-written.
  tmp="$DEST.new-$$"
  trap 'rm -f "$tmp"' EXIT
  cp -f "$SOURCE" "$tmp"
  chmod 755 "$tmp"
  mv -f "$tmp" "$DEST"
  trap - EXIT
  log "Installed $DEST"
fi

# ---- launcher command ----------------------------------------------------------
if [[ -e "$WRAPPER" ]] && ! grep -q "Launch the Emulsion AppImage" "$WRAPPER" 2>/dev/null; then
  warn "$WRAPPER exists and was not written by this script; leaving it alone"
else
  mkdir -p "$BIN_DIR"
  cat >"$WRAPPER" <<EOF
#!/usr/bin/env sh
# Launch the Emulsion AppImage. Written by scripts/install-appimage.sh.
APPIMAGE="\${EMULSION_APPIMAGE:-$DEST}"
if [ ! -x "\$APPIMAGE" ]; then
  echo "emulsion: \$APPIMAGE is missing or not executable" >&2
  exit 1
fi
exec "\$APPIMAGE" "\$@"
EOF
  chmod 755 "$WRAPPER"
  log "Launcher command: $WRAPPER"
fi

# ---- desktop entry -------------------------------------------------------------
# Named after the Wayland app id, which is how compositors match the window to
# its entry and icon. Rewritten on each install unless it has been edited by
# hand (the marker line is gone).
if [[ -e "$DESKTOP_FILE" ]] && ! grep -qx "$MARKER" "$DESKTOP_FILE"; then
  warn "$DESKTOP_FILE was edited by hand; leaving it alone"
else
  mkdir -p "$(dirname "$DESKTOP_FILE")"
  sed -e "s|^Exec=.*|Exec=$WRAPPER %F|" -e "s|^TryExec=.*|TryExec=$WRAPPER|" \
    "$ROOT_DIR/packaging/linux/$APP_ID.desktop" >"$DESKTOP_FILE"
  echo "$MARKER" >>"$DESKTOP_FILE"
  if command -v desktop-file-validate >/dev/null && ! desktop-file-validate "$DESKTOP_FILE" >/dev/null 2>&1; then
    warn "desktop-file-validate reports problems with $DESKTOP_FILE"
  fi
  log "Desktop entry: $DESKTOP_FILE"
fi

# ---- icons -----------------------------------------------------------------------
# From the AppImage itself, so the installed icon always matches the build.
extract_dir="$(mktemp -d)"
trap 'rm -rf "$extract_dir"' EXIT
( cd "$extract_dir" && "$SOURCE" --appimage-extract 'usr/share/icons/*' >/dev/null 2>&1 ) || true
icons="$extract_dir/squashfs-root/usr/share/icons/hicolor"
if [[ -d "$icons" ]]; then
  while IFS= read -r f; do
    install -Dm644 "$f" "$ICON_ROOT/${f#"$icons"/}"
  done < <(find "$icons" -type f -name 'emulsion.*')
  log "Icons installed"
else
  warn "could not extract icons from the AppImage; the launcher may show a generic icon"
fi
rm -rf "$extract_dir"
trap - EXIT

refresh_caches

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) warn "$BIN_DIR is not on your PATH, so the 'emulsion' command will not resolve in a terminal" ;;
esac
log "Done. Start Emulsion from your launcher, or run 'emulsion [file]'."
