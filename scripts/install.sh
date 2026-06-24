#!/usr/bin/env bash
set -euo pipefail

# Usage: curl -fsSL https://github.com/agentserver/agentx/releases/latest/download/install.sh | sh

VERSION="${AGENTX_VERSION:-latest}"
INSTALL_DIR="${AGENTX_INSTALL_DIR:-/usr/local/bin}"

uname_s=$(uname -s)
uname_m=$(uname -m)

case "$uname_s-$uname_m" in
  Darwin-arm64)        target=aarch64-apple-darwin ;;
  Darwin-x86_64)       target=x86_64-apple-darwin ;;
  Linux-x86_64)        target=x86_64-unknown-linux-musl ;;
  Linux-aarch64|Linux-arm64) target=aarch64-unknown-linux-musl ;;
  *) echo "agentx: unsupported platform $uname_s-$uname_m" >&2; exit 1 ;;
esac

base="https://github.com/agentserver/agentx/releases/${VERSION}/download"
if [[ "$VERSION" == "latest" ]]; then
  base="https://github.com/agentserver/agentx/releases/latest/download"
fi

tarball="agentx-$target.tar.gz"
url="$base/$tarball"
checksum_url="$url.sha256"

tmp=$(mktemp -d)
trap "rm -rf $tmp" EXIT

echo "agentx: downloading $url"
curl -fsSL "$url" -o "$tmp/$tarball"
curl -fsSL "$checksum_url" -o "$tmp/$tarball.sha256"

(cd "$tmp" && sha256sum -c "$tarball.sha256")

tar -xzf "$tmp/$tarball" -C "$tmp"

src="$tmp/agentx-$target/agentx"
if [[ ! -x "$src" ]]; then
  echo "agentx: extracted tarball missing executable at $src" >&2
  exit 1
fi

if [[ -w "$INSTALL_DIR" ]]; then
  install -m 0755 "$src" "$INSTALL_DIR/agentx"
else
  echo "agentx: installing to $INSTALL_DIR (needs sudo)"
  sudo install -m 0755 "$src" "$INSTALL_DIR/agentx"
fi

# Linux: install bwrap sandbox helper next to agentx if present in the tarball.
# Note: the bwrap helper is not included in v0.0.1 Linux tarballs because the
# bubblewrap C sources are not yet vendored. agentx will run without the sandbox
# helper; a future release will add it.
if [[ -x "$tmp/agentx-$target/bwrap" ]]; then
  if [[ -w "$INSTALL_DIR" ]]; then
    install -m 0755 "$tmp/agentx-$target/bwrap" "$INSTALL_DIR/bwrap"
  else
    sudo install -m 0755 "$tmp/agentx-$target/bwrap" "$INSTALL_DIR/bwrap"
  fi
fi

echo "agentx: installed to $INSTALL_DIR/agentx"
$INSTALL_DIR/agentx --version 2>/dev/null || true
