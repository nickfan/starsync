#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-}"

usage() {
  cat <<'USAGE'
Usage:
  scripts/bump-version.sh VERSION [--no-commit] [--no-push]

Examples:
  scripts/bump-version.sh 0.1.1
  scripts/bump-version.sh v0.1.1
  scripts/bump-version.sh 0.1.1 --no-push

By default this updates Cargo.toml and Cargo.lock, commits the version bump,
and pushes the current branch. After GitHub CI passes on master, the
auto-release workflow creates tag vVERSION and dispatches release.yml.
USAGE
}

if [[ -z "${VERSION}" || "${VERSION}" == "-h" || "${VERSION}" == "--help" ]]; then
  usage
  [[ -z "${VERSION}" ]] && exit 1 || exit 0
fi

VERSION="${VERSION#v}"
DO_COMMIT=true
DO_PUSH=true

shift || true
for arg in "$@"; do
  case "${arg}" in
    --no-commit)
      DO_COMMIT=false
      DO_PUSH=false
      ;;
    --no-push)
      DO_PUSH=false
      ;;
    *)
      echo "unsupported argument: ${arg}" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ ! "${VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]]; then
  echo "version must look like 0.1.1 or v0.1.1" >&2
  exit 1
fi

cd "${ROOT}"

if [[ "${DO_COMMIT}" == true && -n "$(git status --porcelain)" ]]; then
  echo "working tree must be clean before committing a version bump" >&2
  exit 1
fi

current_version="$(python3 - <<'PY'
import tomllib
with open("Cargo.toml", "rb") as fh:
    print(tomllib.load(fh)["package"]["version"])
PY
)"

if [[ "${current_version}" == "${VERSION}" ]]; then
  echo "Cargo.toml is already at ${VERSION}" >&2
  exit 1
fi

python3 - "${VERSION}" <<'PY'
import re
import sys
from pathlib import Path

version = sys.argv[1]
path = Path("Cargo.toml")
raw = path.read_text()
pattern = re.compile(r'(^\[package\]\s.*?^version\s*=\s*")[^"]+(")', re.M | re.S)
updated, count = pattern.subn(rf"\g<1>{version}\2", raw, count=1)
if count != 1:
    raise SystemExit("failed to update package.version in Cargo.toml")
path.write_text(updated)

chart = Path("deploy/helm/starsync/Chart.yaml")
if chart.exists():
    raw = chart.read_text()
    raw = re.sub(r'(?m)^version:\s*"?[^"\n]+"?$', f"version: {version}", raw, count=1)
    raw = re.sub(r'(?m)^appVersion:\s*"?[^"\n]+"?$', f'appVersion: "{version}"', raw, count=1)
    chart.write_text(raw)
PY

cargo check

echo "bumped starsync to ${VERSION}"

if [[ "${DO_COMMIT}" == true ]]; then
  git add Cargo.toml Cargo.lock
  if [[ -f deploy/helm/starsync/Chart.yaml ]]; then
    git add deploy/helm/starsync/Chart.yaml
  fi
  git commit -m "chore: bump version to ${VERSION}"
fi

if [[ "${DO_PUSH}" == true ]]; then
  branch="$(git branch --show-current)"
  if [[ -z "${branch}" ]]; then
    echo "cannot push from detached HEAD" >&2
    exit 1
  fi
  if [[ "${branch}" != "master" ]]; then
    echo "automatic release only runs from master; use --no-push or switch to master" >&2
    exit 1
  fi
  git push origin "${branch}"
fi
