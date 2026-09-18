#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root_dir=$(CDPATH= cd -- "$script_dir/.." && pwd)
version=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$root_dir/herdr-plugin.toml")

case "$(uname -s):$(uname -m)" in
    Darwin:arm64 | Darwin:aarch64) target=aarch64-apple-darwin ;;
    Darwin:x86_64 | Darwin:amd64) target=x86_64-apple-darwin ;;
    Linux:aarch64 | Linux:arm64) target=aarch64-unknown-linux-musl ;;
    Linux:x86_64 | Linux:amd64) target=x86_64-unknown-linux-musl ;;
    *)
        echo "herdr-tiling: unsupported platform: $(uname -s) $(uname -m)" >&2
        exit 1
        ;;
esac

if [ -z "$version" ]; then
    echo "herdr-tiling: could not read the plugin version" >&2
    exit 1
fi

if command -v curl >/dev/null 2>&1; then
    download() {
        curl --fail --location --retry 5 --retry-delay 2 --retry-all-errors \
            --output "$2" "$1"
    }
elif command -v wget >/dev/null 2>&1; then
    download() {
        wget --tries=5 --waitretry=2 --output-document="$2" "$1"
    }
else
    echo "herdr-tiling: curl or wget is required" >&2
    exit 1
fi

asset="herdr-tiling-$target"
base_url="https://github.com/jaeheonji/herdr-tiling/releases/download/v$version"
install_dir="$root_dir/target/release"
mkdir -p "$install_dir"
tmp_dir=$(mktemp -d "$install_dir/.herdr-tiling.XXXXXX")
trap 'rm -rf "$tmp_dir"' 0 HUP INT TERM

if ! download "$base_url/$asset" "$tmp_dir/$asset" ||
    ! download "$base_url/SHA256SUMS" "$tmp_dir/SHA256SUMS"; then
    echo "herdr-tiling: prebuilt binary download failed" >&2
    echo "Install Rust and run 'cargo build --release' to build from source." >&2
    exit 1
fi

expected=$(awk -v asset="$asset" '$2 == asset || $2 == "*" asset { print $1; exit }' \
    "$tmp_dir/SHA256SUMS")
if [ -z "$expected" ]; then
    echo "herdr-tiling: checksum not found for $asset" >&2
    exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$tmp_dir/$asset" | awk '{ print $1 }')
elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$tmp_dir/$asset" | awk '{ print $1 }')
else
    echo "herdr-tiling: sha256sum or shasum is required" >&2
    exit 1
fi

if [ "$actual" != "$expected" ]; then
    echo "herdr-tiling: checksum verification failed for $asset" >&2
    exit 1
fi

chmod +x "$tmp_dir/$asset"
mv "$tmp_dir/$asset" "$install_dir/herdr-tiling"
echo "Installed herdr-tiling $version for $target"
