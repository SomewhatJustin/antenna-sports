#!/usr/bin/env bash
set -euo pipefail

APP=antenna-sports
BIN_DIR="${HOME}/.local/bin"
STATE_DIR="${HOME}/.local/share/${APP}"
LIVE_BIN="${BIN_DIR}/${APP}"
PREVIOUS_BIN="${STATE_DIR}/${APP}.previous"
CANDIDATE_BIN="${STATE_DIR}/${APP}.candidate"

# Build only this script's checkout, never a caller's stale working directory.
SOURCE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "${SOURCE_DIR}"
SOURCE_SHA="$(git rev-parse HEAD)"
if [[ -n "$(git status --porcelain --untracked-files=normal)" ]]; then
  echo "Refusing to deploy a dirty source checkout: ${SOURCE_DIR}" >&2
  exit 1
fi
if [[ -n "${GITHUB_SHA:-}" && "${SOURCE_SHA}" != "${GITHUB_SHA}" ]]; then
  echo "Source HEAD does not match the requested GitHub SHA" >&2
  exit 1
fi
# Explicit target directory also prevents environment overrides selecting an old binary.
BUILD_DIR="${SOURCE_DIR}/target/production-deploy"
mkdir -p "${BIN_DIR}" "${STATE_DIR}"
cargo build --release --locked --target-dir "${BUILD_DIR}"
install -m 0755 "${BUILD_DIR}/release/${APP}" "${CANDIDATE_BIN}"
CANDIDATE_SHA256="$(sha256sum "${CANDIDATE_BIN}" | cut -d ' ' -f 1)"

if [[ -x "${LIVE_BIN}" ]]; then
  cp --preserve=mode,timestamps "${LIVE_BIN}" "${PREVIOUS_BIN}"
fi
install -m 0755 "${CANDIDATE_BIN}" "${LIVE_BIN}"

systemctl --user restart "${APP}.service"

healthy=0
for _ in {1..20}; do
  PID="$(systemctl --user show "${APP}.service" --property MainPID --value)"
  RUNNING_SHA256="$(sha256sum "/proc/${PID}/exe" 2>/dev/null | cut -d ' ' -f 1)" || RUNNING_SHA256=""
  if curl --fail --silent --show-error --max-time 3 "http://127.0.0.1:8790/health" >/dev/null &&
     [[ "${RUNNING_SHA256}" == "${CANDIDATE_SHA256}" ]]; then
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

printf '%s\n' "${SOURCE_SHA}" > "${STATE_DIR}/deployed-sha"
printf '%s  %s\n' "${CANDIDATE_SHA256}" "${SOURCE_SHA}" > "${STATE_DIR}/deployed-binary-sha256"
echo "Verified running executable ${CANDIDATE_SHA256} from ${SOURCE_SHA} (${SOURCE_DIR})"

rm -f "${CANDIDATE_BIN}"
echo "Deployed ${APP} successfully"
