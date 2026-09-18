#!/bin/sh
set -eu

source_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
version=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$source_root/herdr-plugin.toml")
tmp_root=$(mktemp -d "${TMPDIR:-/tmp}/herdr-tiling-test.XXXXXX")
trap 'rm -rf "$tmp_root"' 0 HUP INT TERM
fake_bin="$tmp_root/bin"
release_dir="$tmp_root/release"
mkdir -p "$fake_bin" "$release_dir"

cat > "$fake_bin/uname" <<'EOF'
#!/bin/sh
case "$1" in
    -s) printf '%s\n' "$TEST_UNAME_S" ;;
    -m) printf '%s\n' "$TEST_UNAME_M" ;;
    *) exit 1 ;;
esac
EOF

cat > "$fake_bin/curl" <<'EOF'
#!/bin/sh
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output) output=$2; shift 2 ;;
        http*) url=$1; shift ;;
        *) shift ;;
    esac
done
printf '%s\n' "$url" >> "$TEST_DOWNLOAD_LOG"
cp "$TEST_RELEASE_DIR/${url##*/}" "$output"
EOF

cat > "$fake_bin/cargo" <<'EOF'
#!/bin/sh
touch "$TEST_CARGO_CALLED"
exit 1
EOF
chmod +x "$fake_bin/uname" "$fake_bin/curl" "$fake_bin/cargo"

prepare_case() {
    case_root="$tmp_root/$1"
    mkdir -p "$case_root/scripts"
    cp "$source_root/herdr-plugin.toml" "$case_root/herdr-plugin.toml"
    cp "$source_root/scripts/install-binary.sh" "$case_root/scripts/install-binary.sh"
}

checksum() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{ print $1 }'
    else
        shasum -a 256 "$1" | awk '{ print $1 }'
    fi
}

run_success() {
    name=$1
    TEST_UNAME_S=$2
    TEST_UNAME_M=$3
    target=$4
    prepare_case "$name"
    asset="herdr-tiling-$target"
    printf '%s\n' "$target" > "$release_dir/$asset"
    printf '%s  %s\n' "$(checksum "$release_dir/$asset")" "$asset" > "$release_dir/SHA256SUMS"
    TEST_DOWNLOAD_LOG="$case_root/download.log"
    export TEST_UNAME_S TEST_UNAME_M TEST_DOWNLOAD_LOG
    PATH="$fake_bin:$PATH" /bin/sh "$case_root/scripts/install-binary.sh"
    cmp "$release_dir/$asset" "$case_root/target/release/herdr-tiling"
    grep -q "/v$version/$asset$" "$TEST_DOWNLOAD_LOG"
}

export TEST_RELEASE_DIR="$release_dir"
export TEST_CARGO_CALLED="$tmp_root/cargo-called"
run_success linux-x64 Linux x86_64 x86_64-unknown-linux-musl
run_success linux-arm Linux aarch64 aarch64-unknown-linux-musl
run_success macos-x64 Darwin x86_64 x86_64-apple-darwin
run_success macos-arm Darwin arm64 aarch64-apple-darwin

prepare_case bad-checksum
TEST_UNAME_S=Linux
TEST_UNAME_M=x86_64
TEST_DOWNLOAD_LOG="$case_root/download.log"
export TEST_UNAME_S TEST_UNAME_M TEST_DOWNLOAD_LOG
printf 'new binary\n' > "$release_dir/herdr-tiling-x86_64-unknown-linux-musl"
printf 'invalid  herdr-tiling-x86_64-unknown-linux-musl\n' > "$release_dir/SHA256SUMS"
mkdir -p "$case_root/target/release"
printf 'old binary\n' > "$case_root/target/release/herdr-tiling"
if PATH="$fake_bin:$PATH" /bin/sh "$case_root/scripts/install-binary.sh"; then
    echo "expected checksum failure" >&2
    exit 1
fi
grep -q '^old binary$' "$case_root/target/release/herdr-tiling"

prepare_case missing-asset
rm -f "$release_dir/herdr-tiling-x86_64-unknown-linux-musl"
TEST_DOWNLOAD_LOG="$case_root/download.log"
export TEST_DOWNLOAD_LOG
mkdir -p "$case_root/target/release"
printf 'old binary\n' > "$case_root/target/release/herdr-tiling"
if PATH="$fake_bin:$PATH" /bin/sh "$case_root/scripts/install-binary.sh"; then
    echo "expected missing asset failure" >&2
    exit 1
fi
grep -q '^old binary$' "$case_root/target/release/herdr-tiling"

prepare_case unsupported
TEST_UNAME_S=FreeBSD
TEST_UNAME_M=x86_64
TEST_DOWNLOAD_LOG="$case_root/download.log"
export TEST_UNAME_S TEST_UNAME_M TEST_DOWNLOAD_LOG
if PATH="$fake_bin:$PATH" /bin/sh "$case_root/scripts/install-binary.sh"; then
    echo "expected unsupported platform failure" >&2
    exit 1
fi

test ! -e "$TEST_CARGO_CALLED"
echo "install-binary tests passed"
