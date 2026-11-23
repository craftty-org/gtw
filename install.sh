#!/usr/bin/env bash
# gtw installer — https://github.com/craftty-org/gtw
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/craftty-org/gtw/main/install.sh | bash
#   ./install.sh --version 0.1.0
#   ./install.sh --prefix /usr/local
#
# When run from inside a release archive (bin/gtw next to this script), the
# bundled assets are installed directly. Otherwise the matching archive is
# downloaded from the GitHub release, checksum-verified, and installed.
set -euo pipefail

REPO="craftty-org/gtw"
VERSION=""
PREFIX="${PREFIX:-$HOME/.local}"
ASSUME_YES=0

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
warn() { printf "${RED}!${NC} %s\n" "$*" >&2; }
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
gtw installer — https://github.com/${REPO}

Usage: install.sh [options]

Options:
  -h, --help            Show this help
  -v, --version <ver>   Install a specific release (for example 0.1.0)
  -p, --prefix <dir>    Install prefix (default: \$HOME/.local)
  -y, --yes             Never prompt
USAGE_EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    -h | --help)
      usage
      exit 0
      ;;
    -v | --version)
      [ "$#" -ge 2 ] || die "--version needs a value"
      VERSION="${2#v}"
      shift 2
      ;;
    --version=*)
      VERSION="${1#--version=}"
      VERSION="${VERSION#v}"
      shift
      ;;
    -p | --prefix)
      [ "$#" -ge 2 ] || die "--prefix needs a value"
      PREFIX="$2"
      shift 2
      ;;
    --prefix=*)
      PREFIX="${1#--prefix=}"
      shift
      ;;
    -y | --yes)
      ASSUME_YES=1
      shift
      ;;
    *)
      die "unknown argument: $1 (try --help)"
      ;;
  esac
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd 2>/dev/null || true)"

detect_platform() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  if [ "$(uname -o 2>/dev/null)" = "Android" ] || [ -n "${TERMUX_VERSION:-}" ]; then
    os="android"
  fi
  case "$os" in
    linux | darwin | android) ;;
    *) die "unsupported OS: $os (supported: Linux, macOS, Termux)" ;;
  esac

  arch="$(uname -m)"
  case "$arch" in
    x86_64 | amd64) arch="x86_64" ;;
    aarch64 | arm64) arch="aarch64" ;;
    *) die "unsupported architecture: $arch (supported: x86_64, aarch64)" ;;
  esac

  if [ "$os" = "android" ]; then
    [ "$arch" = "aarch64" ] || die "Android releases are aarch64 only"
    PLATFORM="aarch64-android"
  elif [ "$os" = "darwin" ]; then
    [ "$arch" = "aarch64" ] || die "macOS releases are Apple Silicon only"
    PLATFORM="aarch64-darwin"
  elif [ -f /etc/arch-release ] || command -v pacman >/dev/null 2>&1; then
    PLATFORM="arch-${arch}"
  elif [ -f /etc/alpine-release ] \
    || { command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; }; then
    PLATFORM="${arch}-linux-musl"
  else
    PLATFORM="debian-12-${arch}"
  fi
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "requires '$1' — install it first"
}

download() {
  curl -fsSL --connect-timeout 15 --retry 3 --retry-delay 2 "$1" -o "$2"
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    return 1
  fi
}

verify_checksum() {
  local dir="$1" archive="$2" expected actual
  if [ ! -f "$dir/checksums.txt" ]; then
    warn "release has no checksums.txt; skipping checksum verification"
    return 0
  fi
  expected="$(awk -v name="$archive" '$2 == name || $2 == "*"name {print $1; exit}' "$dir/checksums.txt")"
  if [ -z "$expected" ]; then
    warn "checksums.txt does not list $archive; skipping checksum verification"
    return 0
  fi
  if ! actual="$(sha256_of "$dir/$archive")"; then
    warn "no SHA-256 tool available; skipping checksum verification"
    return 0
  fi
  [ "$expected" = "$actual" ] || die "checksum mismatch for $archive"
  ok "checksum verified"
}

install_completions() {
  local src base dest
  for src in completions/*; do
    [ -f "$src" ] || continue
    base="$(basename "$src")"
    case "$base" in
      gtw.bash) dest="$bash_comp_dir" ;;
      _gtw) dest="$zsh_comp_dir" ;;
      gtw.fish) dest="$fish_comp_dir" ;;
      gtw.elv) dest="$elvish_comp_dir" ;;
      gtw.ps1) dest="$powershell_comp_dir" ;;
      *) continue ;;
    esac
    mkdir -p "$dest"
    install -m 0644 "$src" "$dest/$base"
  done
}

add_to_path() {
  local bindir="$1" file
  if echo ":$PATH:" | grep -q ":$bindir:"; then
    return 0
  fi
  info "$bindir is not in \$PATH"
  for file in "$HOME/.bashrc" "$HOME/.zshrc"; do
    [ -f "$file" ] || continue
    grep -qF "$bindir" "$file" 2>/dev/null && continue
    {
      printf '\n# Added by the gtw installer\n'
      printf 'export PATH="%s:$PATH"\n' "$bindir"
    } >>"$file"
    ok "added $bindir to ${file/#$HOME/~}"
  done
  local fish_config="$HOME/.config/fish/config.fish"
  if [ -f "$fish_config" ] && ! grep -qF "fish_add_path $bindir" "$fish_config" 2>/dev/null; then
    {
      printf '\n# Added by the gtw installer\n'
      printf 'fish_add_path %s\n' "$bindir"
    } >>"$fish_config"
    ok "added $bindir to ~/.config/fish/config.fish"
  fi
  info "restart your shell to pick up the PATH change"
}

install_from_archive() {
  local root="$1"
  local datadir="${DATADIR:-$PREFIX/share}"
  local bindir="${BINDIR:-$PREFIX/bin}"
  local mandir="${MANDIR:-$datadir/man/man1}"
  local xdg_data="${XDG_DATA_HOME:-$HOME/.local/share}"
  local xdg_config="${XDG_CONFIG_HOME:-$HOME/.config}"
  local bash_comp_dir="${BASH_COMPLETION_DIR:-$xdg_data/bash-completion/completions}"
  local zsh_comp_dir="${ZSH_COMPLETION_DIR:-${ZDOTDIR:-$HOME}/.zsh/completions}"
  local fish_comp_dir="${FISH_COMPLETION_DIR:-$xdg_config/fish/completions}"
  local elvish_comp_dir="${ELVISH_COMPLETION_DIR:-$HOME/.elvish/lib}"
  local powershell_comp_dir="${POWERSHELL_COMPLETION_DIR:-$xdg_data/powershell/Modules}"

  [ -f "$root/bin/gtw" ] || die "archive is missing bin/gtw"

  stage "installing gtw into $PREFIX"
  mkdir -p "$bindir"
  install -m 0755 "$root/bin/gtw" "$bindir/gtw"
  ok "binary: $bindir/gtw"

  if compgen -G "$root/man/man1/*.1" >/dev/null; then
    mkdir -p "$mandir"
    install -m 0644 "$root"/man/man1/*.1 "$mandir/"
    ok "man page: $mandir/gtw.1"
  fi

  if [ -d "$root/completions" ]; then
    (
      cd "$root"
      install_completions
    )
    ok "shell completions"
  fi

  if [ -f "$root/LICENSE" ]; then
    mkdir -p "$datadir/licenses/gtw"
    install -m 0644 "$root/LICENSE" "$datadir/licenses/gtw/LICENSE"
  fi

  if [ "${PREFIX#"$HOME"}" != "$PREFIX" ] && [ "$ASSUME_YES" != 1 ]; then
    add_to_path "$bindir"
  fi

  printf '\n' >&2
  ok "gtw installed"
  info "run: gtw --doctor"
  info "run: gtw"
}

bootstrap() {
  need curl
  need tar

  detect_platform
  local tag
  if [ -n "$VERSION" ]; then
    tag="v${VERSION}"
  else
    stage "resolving the latest release"
    local api
    api="$(curl -fsSL --connect-timeout 15 --retry 3 \
      "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null || true)"
    tag="$(printf '%s' "$api" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)"
    [ -n "$tag" ] || die "could not resolve the latest release; retry or pass --version <ver>"
    VERSION="${tag#v}"
    ok "latest release: $tag"
  fi

  local archive="gtw-${PLATFORM}.tar.gz"
  local url="https://github.com/${REPO}/releases/download/${tag}/${archive}"
  local checksum_url="https://github.com/${REPO}/releases/download/${tag}/checksums.txt"
  local tmp
  tmp="$(mktemp -d)" || die "mktemp failed"
  trap 'rm -rf "${tmp:?}"' EXIT

  stage "downloading $archive"
  download "$url" "$tmp/$archive" || die "download failed: $url"
  download "$checksum_url" "$tmp/checksums.txt" 2>/dev/null || true
  verify_checksum "$tmp" "$archive"
  ok "downloaded $archive"

  stage "extracting"
  tar -xzf "$tmp/$archive" -C "$tmp" || die "could not extract $archive"
  local root="$tmp/${archive%.tar.gz}"
  [ -d "$root" ] || die "archive did not contain ${archive%.tar.gz}/"

  install_from_archive "$root"
}

if [ -n "$script_dir" ] && [ -f "$script_dir/bin/gtw" ]; then
  install_from_archive "$script_dir"
else
  bootstrap
fi
