#!/usr/bin/env sh
set -eu

REPO_URL="${RUNE_REPO_URL:-https://github.com/ghostrider0470/rune}"
BRANCH="${RUNE_BRANCH:-main}"
INSTALL_DIR="${RUNE_INSTALL_DIR:-$HOME/.local/bin}"
CARGO_HOME_DEFAULT="${CARGO_HOME:-$HOME/.cargo}"
CARGO_BIN="$CARGO_HOME_DEFAULT/bin/cargo"

need_cmd() {
  command -v "$1" >/dev/null 2>&1
}

say() {
  printf '%s\n' "$*"
}

ensure_rust() {
  if need_cmd cargo; then
    return 0
  fi
  say "[rune-install] cargo not found; installing Rust via rustup"
  if ! need_cmd curl; then
    say "[rune-install] curl is required to install Rust"
    exit 1
  fi
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  if [ -x "$CARGO_BIN" ]; then
    PATH="$CARGO_HOME_DEFAULT/bin:$PATH"
    export PATH
  fi
  if ! need_cmd cargo; then
    say "[rune-install] cargo still not available after rustup install"
    exit 1
  fi
}

ensure_git() {
  if need_cmd git; then
    return 0
  fi
  say "[rune-install] git is required"
  exit 1
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
EOF
}

ensure_git
ensure_rust
build_and_install
print_next_steps
