# Antenna Sports

A tiny Rust dashboard showing live football and baseball games carried by ABC, CBS, FOX, or NBC. It uses ESPN's scoreboard feeds for matchups, broadcast networks, scores, and game state instead of trying to infer live games from a TV-grid schedule.

The server binds to `127.0.0.1:8790` by design. Put it behind a private reverse proxy such as Tailscale Serve if you want to view it from other devices.

## Run locally

```bash
cargo run --release
```

Then open `http://127.0.0.1:8790`.

## Checks

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

## Production deployment

The repository includes:

- `.github/workflows/ci-deploy.yml` — tests every push and pull request; pushes to `main` deploy after CI passes.
- `scripts/deploy-local.sh` — builds an optimized binary, installs it atomically to `~/.local/bin/antenna-sports`, restarts the user service, health-checks it, and rolls back on failure.
- `deploy/antenna-sports.service` — example user-level systemd unit.

The deployment job expects a self-hosted GitHub Actions runner on the production machine with the label `antenna-sports-prod`. No application secrets are required.

## Data source

Game and broadcast data comes from ESPN's publicly accessible scoreboard endpoints. This project is not affiliated with or endorsed by ESPN, the listed networks, leagues, or teams.

## License

MIT
