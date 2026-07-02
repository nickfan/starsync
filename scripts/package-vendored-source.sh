#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-}"

if [[ -z "${VERSION}" ]]; then
  VERSION="$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')"
fi

VERSION="${VERSION#v}"
DIST_DIR="${ROOT}/dist"
PACKAGE_NAME="starsync-v${VERSION}-source"
PACKAGE_DIR="${DIST_DIR}/${PACKAGE_NAME}"
ARCHIVE="${DIST_DIR}/starsync-v${VERSION}-vendored-source.tar.gz"

rm -rf "${PACKAGE_DIR}" "${ARCHIVE}"
mkdir -p "${DIST_DIR}" "${PACKAGE_DIR}"

rsync -a \
  --exclude '.git' \
  --exclude 'target' \
  --exclude '/dist' \
  --exclude 'Formula' \
  --include '.env.example' \
  --exclude '.env' \
  --exclude '.env.*' \
  "${ROOT}/" "${PACKAGE_DIR}/"

mkdir -p "${PACKAGE_DIR}/.cargo"
(
  cd "${PACKAGE_DIR}"
  cargo vendor --locked vendor > .cargo/config.toml
)

tar -C "${DIST_DIR}" -czf "${ARCHIVE}" "${PACKAGE_NAME}"

SHA256="$(shasum -a 256 "${ARCHIVE}" | awk '{print $1}')"
printf '%s  %s\n' "${SHA256}" "${ARCHIVE}"
