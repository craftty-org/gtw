#!/usr/bin/env bash
# gtw uninstaller — removes the binary, man page, completions, and optionally
# the local cache and configuration.
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
PURGE=0

if [ -t 2 ]; then
  NC=$'\033[0m'
  MUTED=$'\033[0;2m'
  RED=$'\033[0;31m'
  GREEN=$'\033[0;32m'
  BOLD=$'\033[1m'
else
  NC="" MUTED="" RED="" GREEN="" BOLD=""
fi

info() { printf "${MUTED}%s${NC}\n" "$*" >&2; }
log() { printf "${NC}%s\n" "$*" >&2; }
ok() { printf "${GREEN}✔${NC} %s\n" "$*" >&2; }
fail() { printf "${RED}✘${NC} %s\n" "$*" >&2; }
die() {
  printf "${RED}%s${NC}\n" "$*" >&2
  exit 1
}
stage() { printf "${BOLD}%s${NC}\n" "$*" >&2; }
stage_ok() { printf " ${GREEN}✔${NC}\n" >&2; }
stage_fail() { printf " ${RED}✘${NC}\n" >&2; }

usage() {
  cat <<USAGE_EOF
gtw uninstaller

Usage: uninstall.sh [options]

Options:
  -h, --help         Show this help
  -p, --prefix <dir> Prefix used at install time (default: \$HOME/.local)
      --purge        Also delete the cache, media cache, and configuration
USAGE_EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    -h | --help) usage; exit 0 ;;
    -p | --prefix) [ "$#" -ge 2 ] || die "--prefix needs a value"; PREFIX="$2"; shift 2 ;;
    --prefix=*) PREFIX="${1#--prefix=}"; shift ;;
    --purge) PURGE=1; shift ;;
    *) die "unknown argument: $1" ;;
  esac
done

datadir="${DATADIR:-$PREFIX/share}"
xdg_data="${XDG_DATA_HOME:-$HOME/.local/share}"
xdg_config="${XDG_CONFIG_HOME:-$HOME/.config}"
bash_comp_dir="${BASH_COMPLETION_DIR:-$xdg_data/bash-completion/completions}"
zsh_comp_dir="${ZSH_COMPLETION_DIR:-${ZDOTDIR:-$HOME}/.zsh/completions}"
fish_comp_dir="${FISH_COMPLETION_DIR:-$xdg_config/fish/completions}"
elvish_comp_dir="${ELVISH_COMPLETION_DIR:-$HOME/.elvish/lib}"
powershell_comp_dir="${POWERSHELL_COMPLETION_DIR:-$xdg_data/powershell/Modules}"

remove() {
  local path="$1"
  if [ -e "$path" ] || [ -L "$path" ]; then
    rm -f "$path"
    ok "removed ${path/#$HOME/~}"
  fi
}

stage "removing gtw"
remove "$PREFIX/bin/gtw"
remove "$datadir/man/man1/gtw.1"
remove "$datadir/licenses/gtw/LICENSE"
remove "$bash_comp_dir/gtw"
remove "$zsh_comp_dir/_gtw"
remove "$fish_comp_dir/gtw.fish"
remove "$elvish_comp_dir/gtw.elv"
remove "$powershell_comp_dir/gtw.ps1"
rmdir "$datadir/licenses/gtw" 2>/dev/null || true

if [ "$PURGE" = 1 ]; then
  stage "purging cache and configuration"
  remove "${XDG_CACHE_HOME:-$HOME/.cache}/gtw/cache.db"
  remove "${XDG_CACHE_HOME:-$HOME/.cache}/gtw/cache.db-wal"
  remove "${XDG_CACHE_HOME:-$HOME/.cache}/gtw/cache.db-shm"
  remove "${XDG_CACHE_HOME:-$HOME/.cache}/gtw/gtw.db"
  remove "${XDG_CACHE_HOME:-$HOME/.cache}/gtw/gtw.log"
  rm -rf "${XDG_CACHE_HOME:-$HOME/.cache}/gtw/media"
  rmdir "${XDG_CACHE_HOME:-$HOME/.cache}/gtw" 2>/dev/null || true
  remove "$xdg_config/gtw/config.toml"
  rmdir "$xdg_config/gtw" 2>/dev/null || true
  ok "cache and configuration removed"
else
  printf '\nThe cache and configuration were kept. Remove them with: %s --purge\n' "$0" >&2
fi

ok "gtw uninstalled"
