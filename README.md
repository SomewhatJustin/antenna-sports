# Antenna Sports

A tiny Rust dashboard showing live football and baseball games carried by Austin's ABC, CBS, FOX, or NBC stations for ZIP 78746. On TV Tonight's Austin antenna listings determine which games are available locally; ESPN supplies scores and game state. Regional games on the same national network are not assumed to air in Austin.

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

Local broadcasts and matchups come from On TV Tonight's Austin antenna guide, using a shared locale cookie jar for schedule and airing details. ESPN's publicly accessible scoreboard endpoints supply scores and game state. If the local schedule is unavailable, the page reports an error instead of falling back to the national slate. This project is not affiliated with or endorsed by these providers, the listed networks, leagues, or teams.

## License

MIT
