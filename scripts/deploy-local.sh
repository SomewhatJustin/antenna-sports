#!/usr/bin/env bash
set -euo pipefail

APP=antenna-sports
BIN_DIR="${HOME}/.local/bin"
STATE_DIR="${HOME}/.local/share/${APP}"
LIVE_BIN="${BIN_DIR}/${APP}"
PREVIOUS_BIN="${STATE_DIR}/${APP}.previous"
CANDIDATE_BIN="${STATE_DIR}/${APP}.candidate"

mkdir -p "${BIN_DIR}" "${STATE_DIR}"

cargo build --release --locked
install -m 0755 "target/release/${APP}" "${CANDIDATE_BIN}"

if [[ -x "${LIVE_BIN}" ]]; then
  cp --preserve=mode,timestamps "${LIVE_BIN}" "${PREVIOUS_BIN}"
fi
install -m 0755 "${CANDIDATE_BIN}" "${LIVE_BIN}"

systemctl --user restart "${APP}.service"

healthy=0
for _ in {1..20}; do
  if curl --fail --silent --show-error --max-time 3 "http://127.0.0.1:8790/health" >/dev/null; then
    healthy=1
    break
  fi
  sleep 1
done

if [[ "${healthy}" != 1 ]]; then
  echo "Deployment health check failed" >&2
  systemctl --user status "${APP}.service" --no-pager >&2 || true
  journalctl --user -u "${APP}.service" -n 50 --no-pager >&2 || true
  if [[ -x "${PREVIOUS_BIN}" ]]; then
    install -m 0755 "${PREVIOUS_BIN}" "${LIVE_BIN}"
    systemctl --user restart "${APP}.service"
    echo "Rolled back to the previous binary" >&2
  fi
  exit 1
fi

if [[ -n "${GITHUB_SHA:-}" ]]; then
  printf '%s\n' "${GITHUB_SHA}" > "${STATE_DIR}/deployed-sha"
fi

rm -f "${CANDIDATE_BIN}"
echo "Deployed ${APP} successfully"
