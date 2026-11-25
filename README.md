# Antenna Sports

A tiny Rust dashboard showing live football and baseball games carried by Austin's ABC, CBS, FOX, or NBC stations for ZIP 78746. On TV Tonight's Austin antenna listings determine which games are available locally; ESPN supplies scores and game state. Regional games on the same national network are not assumed to air in Austin.

The server binds to `127.0.0.1:8790` by design. Put it behind a private reverse proxy such as Tailscale Serve if you want to view it from other devices.

## Run locally

```bash
cargo run --release
```

Then open `http://127.0.0.1:8790`.

## Watch live in Jellyfin

Set any of the following environment variables to the HTTPS URL of that station's channel page in Jellyfin Web. Use the address reachable from the devices that open Antenna Sports. Live cards for configured stations show a **Watch live** link that opens the channel in the browser. Sign in to Jellyfin if prompted, then press **Play**.

| Austin station | Environment variable |
| --- | --- |
| FOX KTBC 7.1 | `ANTENNA_SPORTS_JELLYFIN_KTBC_URL` |
| ABC KVUE 24.1 | `ANTENNA_SPORTS_JELLYFIN_KVUE_URL` |
| NBC KXAN 36.1 | `ANTENNA_SPORTS_JELLYFIN_KXAN_URL` |
| CBS KEYE 42.1 | `ANTENNA_SPORTS_JELLYFIN_KEYE_URL` |

For a systemd deployment, set the variables in a user service drop-in and reload the unit before restarting it.

## Loading and refresh behavior

Pages cached for less than 120 seconds are served immediately. When the cache is empty or stale, the server immediately returns the styled Antenna Sports shell with accessible skeleton cards, then JavaScript makes one asynchronous request to `/sports/refresh` and replaces the main content in-place. No frontend dependencies are required. All page and refresh responses use `Cache-Control: no-store`.

Concurrent visitors share one detached refresh task, which continues if a browser disconnects. Refreshes have a 30-second server deadline and a 35-second browser deadline. Failures remain visible without polling or automatic retry; **Retry refresh** explicitly sends a POST to `/sports/refresh`. Concurrent retries join an existing refresh. A successful result starts a new 120-second cache window. The normal cached page retains its existing five-minute refresh behavior; loading/error shells do not auto-reload. Without JavaScript, reload the page after the background refresh completes.

## Checks

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

The deterministic refresh test uses a semaphore-gated fake loader (no network or sleeps) to cover immediate empty/stale responses, concurrent visitors, disconnected waiters, shared failures, explicit retry, and fresh-cache reuse. An optional test-only browser fixture serves a four-second loading state, an initial failure, then successful retries:

```bash
cargo test --bin antenna-sports browser_fixture_server -- --ignored --nocapture
# Open http://127.0.0.1:18790; stop with Ctrl-C.
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
