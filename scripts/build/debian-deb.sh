#!/usr/bin/env bash
# Build a Debian package from a staged rootfs tree.
#   debian-deb.sh <version> <amd64|arm64> <stage-dir> <out-file>
set -euo pipefail

version="${1:?version required}"
arch="${2:?arch required}"
stage="${3:?stage dir required}"
out="${4:?output file required}"

[ -d "$stage" ] || {
  echo "error: stage dir not found: $stage" >&2
  exit 1
}

tmpdir="$(mktemp -d)"
trap 'rm -rf "${tmpdir:?}"' EXIT
root="$tmpdir/root"
mkdir -p "$root/DEBIAN"
cp -a "$stage"/. "$root"/
chmod 0755 "$root/usr/bin/gtw"

size="$(du -sk "$root/usr" | cut -f1)"
cat > "$root/DEBIAN/control" <<EOF
Package: gtw
Version: $version
Section: utils
Priority: optional
Architecture: $arch
Installed-Size: $size
Maintainer: prjctimg <prjctimg@outlook.com>
Homepage: https://github.com/craftty-org/gtw
Depends: libc6 (>= 2.36), libasound2 (>= 1.0.16)
Recommends: fonts-noto-color-emoji
Description: Wikipedia in the terminal with cached reading and rich media
 gtw is a keyboard- and mouse-driven Wikipedia reader for the terminal. It
 renders articles with responsive wrapping, syntax highlighting, inline
 images, audio and video previews, LaTeX math, a compressed offline cache,
 bookmarks, history, and reading lists.
EOF

mkdir -p "$(dirname "$out")"
dpkg-deb --build --root-owner-group "$root" "$out" >/dev/null
echo "built $out ($(du -h "$out" | cut -f1))"
