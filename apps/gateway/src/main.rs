//! Thin gateway binary — loads config and starts the Rune gateway daemon.
//!
//! Full service wiring (model providers, store repos, runtime engine) will be
//! fleshed out as those subsystems gain real implementations. For now this
//! demonstrates the binary scaffold and config → gateway path.

fn main() {
    // TODO(wave-5+): Wire real services from config and start gateway.
    // Blocked on: real store backend (embedded PG), real model provider config.
    eprintln!("rune-gateway: service wiring not yet implemented — see apps/gateway/src/main.rs");
    std::process::exit(1);
}
