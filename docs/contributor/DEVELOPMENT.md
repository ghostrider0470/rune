# Development

This document holds the detailed development and local-run workflow that does not belong on the public README landing page.

## Prerequisites

- Rust 1.80+
- `build-essential`, `pkg-config` on Linux

Rust install example:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Build

```bash
cargo build --release
```

Primary binaries:
- `target/release/rune`
- `target/release/rune-gateway`

## Configure

```bash
cp config.example.toml config.toml
```

Then fill in the required provider, auth, and channel settings. For multi-instance work, also configure `[instance]` (name, advertised address, peers) so `/api/v1/instance/health` exposes a complete capability manifest. The CLI mirrors that surface with `rune gateway instance-health`, and `rune gateway delegation-plan --strategy least_busy` / `--strategy named --peer-id <peer>` lets you validate peer selection, lifecycle contract, timeout semantics, and conflict-prevention requirements before wiring actual task handoff.
For federation rollouts, treat `capability_hash` changes as an explicit compatibility checkpoint, keep instance IDs stable across restarts so peers recognize rejoining nodes, and verify `/api/v1/instance/peer-health-alerts` before enabling failover-sensitive automation. Rune only marks work absorption as required for unreachable peers; degraded peers stay non-failover to reduce split-brain risk during partitions.

## Run locally

```bash
cargo run --release --bin rune-gateway -- --config config.toml
```

Or run the built binary directly:

```bash
./target/release/rune-gateway --config config.toml
```

After startup, open `http://127.0.0.1:8787/dashboard`.

If `gateway.auth_token` is configured, the dashboard uses the same bearer-token protection as the protected gateway routes.

## Local service-style operation

```bash
systemctl --user start rune-gateway
systemctl --user stop rune-gateway
journalctl --user -u rune-gateway -f
systemctl --user status rune-gateway
```

Rebuild + restart example:

```bash
cargo build --release --bin rune-gateway && systemctl --user restart rune-gateway
```

### One-time example service install

```bash
mkdir -p ~/.config/systemd/user
cat > ~/.config/systemd/user/rune-gateway.service << 'EOF'
[Unit]
Description=Rune Gateway

[Service]
Type=simple
WorkingDirectory=%h/Development/rune
ExecStart=%h/Development/rune/target/release/rune-gateway --config config.toml
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
EOF

systemctl --user daemon-reload
```

## Tests and quality checks

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

## Release flow

Tag-driven via GitHub Actions:

```bash
git tag v0.5.0 && git push origin v0.5.0
```

## Related docs

- [`../INDEX.md`](../INDEX.md)
- [`../operator/DEPLOYMENT.md`](../operator/DEPLOYMENT.md)
- [`../operator/DATABASES.md`](../operator/DATABASES.md)
- [`../reference/CRATE-LAYOUT.md`](../reference/CRATE-LAYOUT.md)
