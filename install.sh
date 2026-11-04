#!/bin/sh
# Installs swipe on macOS or Linux:
#   curl -fsSL https://raw.githubusercontent.com/octanelabsdev/swipe/main/install.sh | sh
# SWIPE_VERSION picks a release tag (default: the latest). SWIPE_INSTALL_DIR picks the folder
# (default: ~/.local/bin).
set -eu

# Everything runs from main, so a download cut off part way runs nothing.
main() {
  repo="octanelabsdev/swipe"
  dir="${SWIPE_INSTALL_DIR:-$HOME/.local/bin}"
  version="${SWIPE_VERSION:-latest}"

  fail() {
    echo "swipe: $1" >&2
    exit 1
  }

  case "$(uname -s)" in
    Darwin) os="apple-darwin" ;;
    Linux) os="unknown-linux-musl" ;;
    *) fail "this script supports macOS and Linux; on Windows, use install.ps1" ;;
  esac
  case "$(uname -m)" in
    x86_64 | amd64) arch="x86_64" ;;
    arm64 | aarch64) arch="aarch64" ;;
    *) fail "there is no swipe build for $(uname -m)" ;;
  esac
  asset="swipe-$arch-$os.tar.gz"

  if [ "$version" = "latest" ]; then
    url="https://github.com/$repo/releases/latest/download"
  else
    url="https://github.com/$repo/releases/download/$version"
  fi

  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  echo "swipe: downloading $asset ($version)"
  curl -fsSL "$url/$asset" -o "$tmp/$asset" || fail "could not download $url/$asset"
  curl -fsSL "$url/$asset.sha256" -o "$tmp/$asset.sha256" || fail "could not download the checksum"

  expected="$(cut -d ' ' -f 1 < "$tmp/$asset.sha256")"
  if command -v sha256sum > /dev/null; then
    actual="$(sha256sum "$tmp/$asset" | cut -d ' ' -f 1)"
  else
    actual="$(shasum -a 256 "$tmp/$asset" | cut -d ' ' -f 1)"
  fi
  [ "$expected" = "$actual" ] || fail "the checksum does not match, so nothing was installed"

  tar -xzf "$tmp/$asset" -C "$tmp"
  mkdir -p "$dir"
  cp "$tmp/swipe" "$dir/swipe.new"
  chmod 755 "$dir/swipe.new"
  mv "$dir/swipe.new" "$dir/swipe"
  echo "swipe: installed $("$dir/swipe" --version) to $dir/swipe"

  case ":$PATH:" in
    *":$dir:"*) ;;
    *) echo "swipe: $dir is not on your PATH; add it in your shell's startup file:
    export PATH=\"$dir:\$PATH\"" ;;
  esac
  echo "swipe: next, add the shell hook: https://swipe.octanelabs.dev/install/#2-add-the-shell-hook"
}

main "$@"
