#!/usr/bin/env sh
set -eu

REPO_URL="${RUNE_REPO_URL:-https://github.com/ghostrider0470/rune}"
BRANCH="${RUNE_BRANCH:-main}"
INSTALL_DIR="${RUNE_INSTALL_DIR:-$HOME/.local/bin}"
CARGO_HOME_DEFAULT="${CARGO_HOME:-$HOME/.cargo}"
CARGO_BIN="$CARGO_HOME_DEFAULT/bin/cargo"
BINARY_REPO="${RUNE_BINARY_REPO:-$REPO_URL}"
OS="$(uname -s 2>/dev/null || echo unknown)"
ARCH_RAW="$(uname -m 2>/dev/null || echo unknown)"

need_cmd() {
  command -v "$1" >/dev/null 2>&1
}

say() {
  printf '%s\n' "$*"
}

die() {
  say "$*"
  exit 1
}

normalize_os() {
  case "$1" in
    Linux) printf 'linux' ;;
    Darwin) printf 'darwin' ;;
    *) printf 'unknown' ;;
  esac
}

normalize_arch() {
  case "$1" in
    x86_64|amd64) printf 'x86_64' ;;
    aarch64|arm64) printf 'aarch64' ;;
    *) printf 'unknown' ;;
  esac
}

ensure_download_tool() {
  if need_cmd curl; then
    printf 'curl'
    return 0
  fi
  if need_cmd wget; then
    printf 'wget'
    return 0
  fi
  return 1
}

download_to() {
  tool="$1"
  url="$2"
  output="$3"
  case "$tool" in
    curl) curl -fsSL "$url" -o "$output" ;;
    wget) wget -qO "$output" "$url" ;;
    *) return 1 ;;
  esac
}

ensure_rust() {
  if need_cmd cargo; then
    return 0
  fi
  say "[rune-install] cargo not found; installing Rust via rustup"
  if ! need_cmd curl; then
    die "[rune-install] curl is required to install Rust"
  fi
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  if [ -x "$CARGO_BIN" ]; then
    PATH="$CARGO_HOME_DEFAULT/bin:$PATH"
    export PATH
  fi
  if ! need_cmd cargo; then
    die "[rune-install] cargo still not available after rustup install"
  fi
}

ensure_git() {
  if need_cmd git; then
    return 0
  fi
  die "[rune-install] git is required for source fallback"
}

release_asset_name() {
  os="$1"
  arch="$2"
  case "$os-$arch" in
    linux-x86_64) printf 'rune-linux-x86_64.tar.gz' ;;
    linux-aarch64) printf 'rune-linux-aarch64.tar.gz' ;;
    darwin-x86_64) printf 'rune-darwin-x86_64.tar.gz' ;;
    darwin-aarch64) printf 'rune-darwin-aarch64.tar.gz' ;;
    *) return 1 ;;
  esac
}

try_install_prebuilt() {
  os_norm="$(normalize_os "$OS")"
  arch_norm="$(normalize_arch "$ARCH_RAW")"
  asset="$(release_asset_name "$os_norm" "$arch_norm" 2>/dev/null || true)"
  [ -n "$asset" ] || return 1

  downloader="$(ensure_download_tool 2>/dev/null || true)"
  [ -n "$downloader" ] || return 1
  need_cmd tar || return 1

  tmpdir=$(mktemp -d 2>/dev/null || mktemp -d -t rune-install)
  trap 'rm -rf "$tmpdir"' EXIT INT TERM

  version="${RUNE_VERSION:-latest}"
  case "$version" in
    latest) url="$BINARY_REPO/releases/latest/download/$asset" ;;
    *) url="$BINARY_REPO/releases/download/$version/$asset" ;;
  esac

  say "[rune-install] trying prebuilt binary: $url"
  if ! download_to "$downloader" "$url" "$tmpdir/$asset"; then
    say "[rune-install] prebuilt binary unavailable; falling back to source build"
    rm -rf "$tmpdir"
    trap - EXIT INT TERM
    return 1
  fi

  mkdir -p "$tmpdir/unpack" "$INSTALL_DIR"
  tar -xzf "$tmpdir/$asset" -C "$tmpdir/unpack"

  [ -f "$tmpdir/unpack/rune" ] || die "[rune-install] archive missing rune binary"
  [ -f "$tmpdir/unpack/rune-gateway" ] || die "[rune-install] archive missing rune-gateway binary"

  install "$tmpdir/unpack/rune" "$INSTALL_DIR/rune"
  install "$tmpdir/unpack/rune-gateway" "$INSTALL_DIR/rune-gateway"
  say "[rune-install] installed prebuilt binaries to $INSTALL_DIR"
  rm -rf "$tmpdir"
  trap - EXIT INT TERM
  return 0
}

build_and_install() {
  tmpdir=$(mktemp -d 2>/dev/null || mktemp -d -t rune-install)
  trap 'rm -rf "$tmpdir"' EXIT INT TERM
  say "[rune-install] cloning $REPO_URL ($BRANCH)"
  git clone --depth 1 --branch "$BRANCH" "$REPO_URL" "$tmpdir/rune"
  cd "$tmpdir/rune"
  say "[rune-install] building rune + rune-gateway"
  cargo build --release --bin rune --bin rune-gateway
  mkdir -p "$INSTALL_DIR"
  install "target/release/rune" "$INSTALL_DIR/rune"
  install "target/release/rune-gateway" "$INSTALL_DIR/rune-gateway"
  say "[rune-install] installed to $INSTALL_DIR"
}

print_next_steps() {
  cat <<EOF

Rune installed.

Next steps:
  export PATH="$INSTALL_DIR:\$PATH"
  rune setup --path ~/.rune --api-key "<YOUR_API_KEY>"

Or if Ollama is already running locally:
  rune setup --path ~/.rune

Optional background service install after setup:
  rune service install --target systemd --name rune-gateway --workdir ~/.rune --config ~/.rune/config.toml --enable --start
  # macOS: rune service install --target launchd --name rune-gateway --workdir ~/.rune --config ~/.rune/config.toml --enable --start

Optional zero-config Docker evaluation:
  docker compose -f docker-compose.zero-config.yml up --build -d
EOF
}

if ! try_install_prebuilt; then
  ensure_git
  ensure_rust
  build_and_install
fi
print_next_steps
