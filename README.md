# StarSync

StarSync is a local-first Rust service for turning your GitHub starred repositories into a searchable personal knowledge base.

It mirrors your GitHub starred repository list, keeps your personal metadata in Markdown/YAML, exposes CLI + REST + MCP interfaces, and builds a Tantivy-derived local search index with Chinese tokenization and pinyin support.

> StarSync v1 never stars or unstars repositories on GitHub. GitHub is the remote source for the star list; local Markdown is the source of truth for your tags, notes, status, and links.

## Features

- Sync GitHub starred repositories into a local mirror.
- Store personal metadata in Markdown front matter under `~/.starsync/data/repos`.
- Search merged GitHub repo facts, local tags/notes/status, and optional README snippets.
- Browse and search through CLI, REST API, OpenAPI 3.1, SSE events, and a stdio MCP server.
- Use local filesystem storage by default, with Git-backed metadata storage available for sharing or backup.
- Use a small Moka in-process read cache while keeping Markdown and mirror files as the source of truth.
- Rebuild the derived Tantivy search index from Markdown and mirror state at any time.

## Install

### Homebrew / Linuxbrew

After the release workflow updates the tap, install from Homebrew or Linuxbrew:

```bash
brew install nickfan/tap/starsync
starsync --help
```

StarSync is published through the shared `nickfan/homebrew-tap` repository, which maps to `nickfan/tap/starsync` in Homebrew output.

Run it as a Homebrew/Linuxbrew background service:

```bash
mkdir -p "$HOME/.config/starsync"
printf 'STARSYNC_GITHUB_TOKEN=github_pat_xxx\n' > "$HOME/.config/starsync/.env"
brew services start starsync
open http://127.0.0.1:8989/ui/
```

Stop or restart the service:

```bash
brew services stop starsync
brew services restart starsync
```

If you installed from the old single-project tap, switch to the shared tap:

```bash
brew uninstall starsync
brew untap nickfan/starsync
brew install nickfan/tap/starsync
```

### Docker

StarSync publishes container images to GHCR, and can also publish to Docker Hub when the repository variables and secrets are configured.

```bash
docker pull ghcr.io/nickfan/starsync:latest
# Docker Hub, when DOCKERHUB_USERNAME=nickfan is configured:
docker pull docker.io/nickfan/starsync:latest
```

The container defaults are:

```text
STARSYNC_DATA_DIR=/data
STARSYNC_STATE_DIR=/state
STARSYNC_SEARCH_INDEX_DIR=/state/search
STARSYNC_UI_DIR=/ui
STARSYNC_BIND=0.0.0.0:8989
```

Run the REST service with persistent host paths:

```bash
mkdir -p "$HOME/.starsync/data" "$HOME/.starsync/state" "$HOME/.starsync/ui"

docker run --rm -it \
  --name starsync \
  --env-file .env \
  -p 8989:8989 \
  -v "$HOME/.starsync/data:/data" \
  -v "$HOME/.starsync/state:/state" \
  -v "$HOME/.starsync/ui:/ui" \
  ghcr.io/nickfan/starsync:latest
```

Run one-shot CLI commands in the same mounted knowledge base:

```bash
docker run --rm -it \
  --env-file .env \
  -v "$HOME/.starsync/data:/data" \
  -v "$HOME/.starsync/state:/state" \
  -v "$HOME/.starsync/ui:/ui" \
  ghcr.io/nickfan/starsync:latest sync

docker run --rm -it \
  --env-file .env \
  -v "$HOME/.starsync/data:/data" \
  -v "$HOME/.starsync/state:/state" \
  -v "$HOME/.starsync/ui:/ui" \
  ghcr.io/nickfan/starsync:latest search rust
```

Your `.env` file can stay minimal for Docker:

```dotenv
STARSYNC_GITHUB_TOKEN=github_pat_xxx
```

Or use the checked-in Compose file:

```bash
cp .env.example .env
# Edit .env and set STARSYNC_GITHUB_TOKEN.
docker compose up -d
open http://127.0.0.1:8989/ui/
```

The Compose service mounts these host paths by default:

```text
$HOME/.starsync/data  -> /data
$HOME/.starsync/state -> /state
$HOME/.starsync/ui    -> /ui
```

Override them when needed:

```bash
STARSYNC_HOST_DATA_DIR=/srv/starsync/data \
STARSYNC_HOST_STATE_DIR=/srv/starsync/state \
STARSYNC_HOST_UI_DIR=/srv/starsync/ui \
docker compose up -d
```

Build the image locally without installing Rust on the host:

```bash
docker buildx build --load -t starsync:dev .
docker run --rm starsync:dev --version
```

The Dockerfile is multi-stage: `cargo-chef` prepares a dependency recipe,
dependency compilation is cached in a separate layer, the final binary is built
inside `rust:${RUST_VERSION}-bookworm` with `RUST_VERSION=1` by default, and the
runtime image is Debian slim with only CA certificates and the `starsync` binary.

Pin or override the Rust toolchain used inside Docker:

```bash
docker buildx build --load \
  --build-arg RUST_VERSION=1 \
  -t starsync:dev .
```

If Docker Hub access is slow or blocked, point the base image pulls at a mirror
that preserves Docker Hub's `library/` image names:

```bash
docker buildx build --load \
  --build-arg BASE_IMAGE_REGISTRY=mirror.gcr.io/library/ \
  -t starsync:dev .
```

For faster repeated local builds, export a BuildKit cache directory:

```bash
docker buildx build --load -t starsync:dev \
  --cache-from type=local,src=.buildx-cache \
  --cache-to type=local,dest=.buildx-cache-new,mode=max .

rm -rf .buildx-cache
mv .buildx-cache-new .buildx-cache
```

### Cargo

```bash
cargo build --release
```

Run from the checkout:

```bash
cargo run -- --help
```

### Kubernetes / Helm

Kubernetes demos live under `deploy/k8s/`, and the Helm chart lives under
`deploy/helm/starsync/`.

```bash
helm upgrade --install starsync deploy/helm/starsync \
  --namespace starsync \
  --create-namespace \
  --set secret.existingSecret=starsync-secret
```

See [docs/deployment/kubernetes.md](docs/deployment/kubernetes.md) for the
plain manifests, Helm values, storage layout, and scheduled sync jobs.

For the trimmed Cloudflare-only cloud mode sketch, see
[docs/architecture/cloud-mode.md](docs/architecture/cloud-mode.md).

Or use the built binary:

```bash
./target/release/starsync --help
```

## GitHub token

StarSync needs a GitHub personal access token only for GitHub API calls such as `sync` and README enrichment.

Recommended for v1:

- Fine-grained personal access token
- Account permission: `Starring: read`
- Optional repository permission: `Contents: read`, useful when enriching README text for private repositories that the token can access
- Expiration: choose a short or reasonable lifetime, such as 90 days

Create a pre-filled read-only token:

[Create StarSync read-only PAT](https://github.com/settings/personal-access-tokens/new?name=StarSync&description=StarSync%20local-first%20starred%20repository%20sync&expires_in=90&starring=read&contents=read)

If you intentionally want a token that is ready for future star/unstar write features, use:

[Create StarSync star read/write PAT](https://github.com/settings/personal-access-tokens/new?name=StarSync%20Read%20Write&description=StarSync%20future%20star%20read-write%20token&expires_in=90&starring=write&contents=read)

StarSync v1 still does not write star/unstar state even if the token has `Starring: write`.

### Using GitHub CLI

GitHub CLI does not currently create a fine-grained PAT from the terminal. For least-privilege StarSync usage, open the fine-grained PAT page above and create a token with `Starring: read`.

You can still use `gh` in two useful ways.

Open the token creation page from the terminal:

```bash
gh browse 'https://github.com/settings/personal-access-tokens/new?name=StarSync&description=StarSync%20local-first%20starred%20repository%20sync&expires_in=90&starring=read&contents=read'
```

Or reuse the GitHub CLI OAuth token for StarSync:

```bash
gh auth login --web
gh auth status
export STARSYNC_GITHUB_TOKEN="$(gh auth token)"
starsync sync
```

The `gh auth token` path is convenient, but it is not a dedicated StarSync fine-grained PAT. GitHub CLI stores an OAuth token for the active account; `gh auth login` has its own minimum scopes, and `gh auth refresh --scopes ...` can request more OAuth scopes. Prefer the fine-grained PAT link above when you want the narrowest StarSync token.

Official references:

- [GitHub starring REST API](https://docs.github.com/en/rest/activity/starring?apiVersion=2022-11-28)
- [Managing personal access tokens](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)
- [GitHub CLI auth login](https://cli.github.com/manual/gh_auth_login)
- [GitHub CLI auth token](https://cli.github.com/manual/gh_auth_token)

## Configure the token

Prefer environment variables for container-friendly deployment:

```bash
export STARSYNC_GITHUB_TOKEN=github_pat_xxx
```

You can also use a `.env` file:

```dotenv
STARSYNC_GITHUB_TOKEN=github_pat_xxx
STARSYNC_DATA_DIR=/path/to/starsync/data
STARSYNC_STATE_DIR=/path/to/starsync/state
STARSYNC_BIND=127.0.0.1:8989
```

Then run:

```bash
starsync --env-file .env sync
```

Config loading priority is:

1. CLI options
2. Process environment variables
3. `.env` file
4. `config.toml`
5. Built-in defaults

Supported environment variables:

```text
STARSYNC_DATA_DIR
STARSYNC_STATE_DIR
STARSYNC_CONFIG
STARSYNC_GITHUB_TOKEN
STARSYNC_BIND
STARSYNC_STORAGE_BACKEND
STARSYNC_GIT_REMOTE
STARSYNC_SEARCH_INDEX_DIR
STARSYNC_UI_ENABLED
STARSYNC_UI_DIR
STARSYNC_UI_AUTO_EXTRACT
STARSYNC_UI_OVERWRITE
STARSYNC_UI_BACKUP
```

`config.toml` may reference environment variables:

```toml
data_dir = "~/.starsync/data"
state_dir = "~/.starsync/state"
ui_dir = "~/.starsync/ui"
bind = "127.0.0.1:8989"

[github]
token = "${STARSYNC_GITHUB_TOKEN}"

[storage]
backend = "local"
# backend = "git"
# git_remote = "git@github.com:you/starsync-meta.git"

[search]
index_dir = "~/.starsync/state/search"

[ui]
enabled = true
dir = "~/.starsync/ui"
auto_extract = true
overwrite = true
backup = true
```

Never commit tokens into the metadata Git repository.

## Quick start

### Manual CLI and Web UI

Initialize local folders:

```bash
starsync init
```

Sync your GitHub starred repositories:

```bash
starsync sync
```

List merged repo + meta records:

```bash
starsync list --limit 20
starsync list --language Rust --tag ai --sort updated --direction desc
```

Search local and remote fields:

```bash
starsync search retrieval
starsync search "agent framework" --archived true
starsync search 'owner:nickfan AND name:^T'
starsync search '(language:Rust AND topic:cli) OR tag:agent'
starsync search 'language:Rust -topic:web stars:>=1000'
starsync search '中文搜索'
starsync search 'notes:向量数据库'
```

Search query syntax follows GitHub-style qualifiers where possible:

- Boolean operators: `AND`, `OR`, `NOT`, parentheses, and implicit `AND` between adjacent terms.
- Negation shorthand: `-topic:web` is the same as `NOT topic:web`.
- Qualifiers: `owner:`, `user:`, `org:`, `name:`, `repo:`, `language:`, `topic:`, `tag:`, `status:`, `archived:`, `current:`, `is:`, `stars:`, `forks:`, `description:`, `summary:`, `notes:`, `readme:`.
- Local prefix matching: `name:^T` or `name:T*`.
- Equality forms: `owner:nickfan`, `owner=nickfan`, and `owner:=nickfan`.
- Numeric comparisons for stars and forks: `stars:>=1000`, `stars:<500`, `stars:100..500`, `forks:>=100`.

GitHub's own starred list endpoint has only basic pagination/sort filters, so StarSync evaluates these richer expressions locally against the synced mirror plus Markdown meta.

Sorting is separate from the query expression: filters decide which repos match, and `--sort` / `--direction` decide result order. Supported sort fields are `created` (the time you starred the repo), `updated` (repository updated time), `name` (full repo name), `stars` (GitHub stargazer count), and `forks` (GitHub fork count). In the Web UI these are exposed as simple presets such as `Recently starred`, `Recent updated`, `Most stars`, and `Most forked`.

Typical search/list cases:

```bash
# Most-starred Rust repos that are not web-topic repos
starsync search 'language:Rust -topic:web' --sort stars --direction desc --limit 20

# Most-forked CLI/topic repos, no keyword required
starsync list --topic cli --sort forks --direction desc --limit 20

# Recently starred repos whose name starts with T
starsync search 'name:^T' --sort created --direction desc --limit 20

# Alphabetical slice for one owner
starsync list --owner nickfan --sort name --direction asc --limit 50

# Page through local meta and GitHub topic matches
starsync search 'topic:cli OR tag:agent' --sort updated --direction desc --page 2 --per-page 25

# Chinese/CJK fuzzy local search through notes, summaries, README snippets, and metadata
starsync search '中文搜索'
starsync search 'summary:本地知识库'
```

Edit local metadata only:

```bash
starsync meta edit owner repo \
  --tag rust \
  --tag ai \
  --status evaluating \
  --summary "Worth tracking for local agent tooling"
```

Archive local metadata without touching GitHub:

```bash
starsync meta delete owner repo
```

Rebuild the derived Tantivy search index from Markdown and mirror state:

```bash
starsync index rebuild
```

This also refreshes the engine-independent local catalogs under `repos/`:

- `INDEX.md` - human-readable top-level index with YAML front matter.
- `catalog.yaml` and `catalog.json` - machine-readable fused repo + meta catalog.
- `INDEX.by-repo.md` - Markdown index grouped by repository-name initial.
- `INDEX.by-owner.md` - Markdown index grouped by owner initial.

Fetch README snippets for current starred repositories:

```bash
starsync enrich readme --limit 50
```

Start the REST service and built-in Web UI:

```bash
starsync serve
```

The terminal prints both entry points:

```text
StarSync REST API listening on http://127.0.0.1:8989
StarSync Web UI available at http://127.0.0.1:8989/ui/
```

By default the compiled binary carries a small static UI bundle. `serve`
extracts the bundled UI into `~/.starsync/ui` and serves it from `/ui/`.
When the existing UI marker does not match the embedded bundle fingerprint,
StarSync refreshes the default UI because `ui.overwrite = true` by default.
Before refreshing, it backs up the previous UI directory to a sibling path such
as `ui.bak-20260703T120000Z` when `ui.backup = true`.

Set `ui.overwrite = false` or `STARSYNC_UI_OVERWRITE=false` when you manage a
fully custom frontend. Set `ui.backup = false` or `STARSYNC_UI_BACKUP=false`
when you want refreshes without creating backup directories.

Useful UI flags:

```bash
starsync serve --ui-dir ~/.starsync/ui
starsync serve --no-ui
starsync serve --no-ui-extract
starsync serve --no-ui-overwrite
starsync serve --no-ui-backup
```

### Homebrew / Linuxbrew Service

After `brew install nickfan/tap/starsync`, put long-running service settings in
`~/.config/starsync/.env`:

```bash
mkdir -p "$HOME/.config/starsync"
printf 'STARSYNC_GITHUB_TOKEN=github_pat_xxx\n' > "$HOME/.config/starsync/.env"
brew services start starsync
```

The service runs `starsync serve` and keeps using the same default data paths:
`~/.starsync/data`, `~/.starsync/state`, and `~/.starsync/ui`.

### systemd --user Service

Linux users who do not want Homebrew services can install the user unit:

```bash
mkdir -p "$HOME/.config/systemd/user" "$HOME/.config/starsync"
cp deploy/systemd/starsync.service "$HOME/.config/systemd/user/starsync.service"
printf 'STARSYNC_GITHUB_TOKEN=github_pat_xxx\n' > "$HOME/.config/starsync/.env"
systemctl --user daemon-reload
systemctl --user enable --now starsync.service
```

Check logs and status:

```bash
systemctl --user status starsync.service
journalctl --user -u starsync.service -f
```

### Docker Compose

```bash
cp .env.example .env
# Edit .env and set STARSYNC_GITHUB_TOKEN.
docker compose up -d
open http://127.0.0.1:8989/ui/
```

Compose uses the published image and mounts local data/state/UI paths under
`$HOME/.starsync` by default. Stop it with:

```bash
docker compose down
```

## Data layout

Default paths:

```text
~/.starsync/data/repos/INDEX.md
~/.starsync/data/repos/catalog.yaml
~/.starsync/data/repos/catalog.json
~/.starsync/data/repos/INDEX.by-repo.md
~/.starsync/data/repos/INDEX.by-owner.md
~/.starsync/data/repos/{owner}/{repo}/INDEX.md
~/.starsync/ui/index.html
~/.starsync/ui/assets/
~/.starsync/state/mirror.json
~/.starsync/state/search/
~/.starsync/state/events.jsonl
~/.starsync/state/event-subscriptions.json
```

Top-level catalog files are derived data and are rebuilt by `sync`, `meta edit`,
`meta delete`, `enrich readme`, and `index rebuild`. They make quick local
lookup possible without the Tantivy index or a running REST service:

```bash
grep -R "keepers" ~/.starsync/data/repos
jq '.items[] | select(.owner == "nickfan") | .full_name' ~/.starsync/data/repos/catalog.json
jq '.items[] | select(.current == false or .archived == true) | .full_name' ~/.starsync/data/repos/catalog.json
```

The per-repo `INDEX.md` stores local metadata in YAML front matter:

```markdown
---
starsync:
  schema: starsync.repo.v1
kind: repo
repo:
  owner: owner
  name: repo
source:
  github_id: 123
  html_url: https://github.com/owner/repo
user:
  tags:
    - rust
    - ai
  status: evaluating
  summary: Worth tracking
  notes: Local notes are searchable.
  links: []
archived: false
---
# owner/repo

Long-form notes go here.
```

Markdown/YAML is the personal metadata source of truth. The Tantivy directory
under `~/.starsync/state/search/` is a derived index and can be rebuilt.

Search indexes fused GitHub repo facts, Markdown meta, README snippets, topics,
tags, and status into Tantivy. StarSync registers `tantivy-jieba` for Chinese
tokenization and also writes derived CJK n-grams, pinyin words, and pinyin
initials into the index. Structured qualifiers and explicit sorting are still
evaluated against the fused repo records so the CLI, REST API, Web UI, and MCP
server return the same local view.

## REST API

Start the local REST service:

```bash
starsync serve
```

Default bind address:

```text
http://127.0.0.1:8989
```

The Web UI is served from:

```text
http://127.0.0.1:8989/ui/
```

Useful endpoints:

```text
GET  /health
GET  /repos
GET  /repos/{owner}/{repo}
GET  /repos/{owner}/{repo}/meta
PATCH /repos/{owner}/{repo}/meta
DELETE /repos/{owner}/{repo}/meta
GET  /search
POST /sync
POST /enrich/readme
GET  /events
GET  /events/recent
GET  /event-subscriptions
POST /event-subscriptions
PATCH /event-subscriptions/{id}
DELETE /event-subscriptions/{id}
GET  /openapi.yaml
GET  /openapi.json
```

`POST /sync` and `POST /enrich/readme` enqueue background tasks and return
`202 Accepted` with a `job_id`. Watch `GET /events`, `GET /events/recent`, or
webhook subscriptions for `task.started`, `task.completed`, and `task.failed`.

Example:

```bash
curl 'http://127.0.0.1:8989/repos?limit=20&language=Rust&sort=updated&direction=desc'
curl 'http://127.0.0.1:8989/search?q=retrieval&tag=ai'
curl 'http://127.0.0.1:8989/search?q=language:Rust%20-topic:web&sort=stars&direction=desc&limit=20'
curl 'http://127.0.0.1:8989/repos?topic=cli&sort=forks&direction=desc&limit=20'
curl 'http://127.0.0.1:8989/repos?owner=nickfan&sort=name&direction=asc&limit=50'
curl -X POST 'http://127.0.0.1:8989/sync'
curl 'http://127.0.0.1:8989/events/recent?limit=20'
curl -X POST 'http://127.0.0.1:8989/event-subscriptions' \
  -H 'content-type: application/json' \
  -d '{"url":"https://example.com/starsync-hook","events":["repo.added","meta.changed"],"secret":"change-me"}'
```

Export OpenAPI 3.1:

```bash
starsync openapi export --format yaml --output openapi.yaml
starsync openapi export --format json --output openapi.json
```

## MCP and agent usage

Run the stdio MCP server:

```bash
starsync mcp
```

Available MCP tools include:

- `search_repos`
- `list_repos`
- `get_repo`
- `update_repo_meta`
- `sync_stars`
- `enrich_readme`
- `list_recent_events`
- `list_event_subscriptions`
- `create_event_subscription`
- `update_event_subscription`
- `delete_event_subscription`

Resources:

- `starsync://index`
- `starsync://repo/{owner}/{repo}`

Export an agent Skill:

```bash
starsync skill export --output ./starsync-skill
```

The generated Skill tells agents to prefer MCP tools when available, fall back to CLI/REST, and never write GitHub star state.

## Git-backed metadata storage

Local storage is the default. To sync Markdown metadata through Git:

```bash
export STARSYNC_STORAGE_BACKEND=git
export STARSYNC_GIT_REMOTE=git@github.com:you/starsync-meta.git

starsync storage pull
starsync storage push
```

Only metadata under `repos/` is staged by the Git storage command. Tokens, event logs, webhook secrets, and derived search index state are not part of the Git metadata sync.

## Release automation

This repository includes GitHub Actions for CI and tagged releases:

- `.github/workflows/ci.yml` runs format, tests, and clippy on `master`, pull requests, and manual dispatch.
- `.github/workflows/release.yml` runs on tags like `v0.1.0` or manual dispatch.
- The release workflow creates or updates the GitHub Release, uploads a Linux binary tarball, uploads a vendored source tarball, publishes GHCR images, optionally publishes Docker Hub images, and optionally updates a Homebrew/Linuxbrew tap formula.

Create the next release with one local command from a clean `master` checkout:

```bash
scripts/bump-version.sh 0.1.1
```

The script updates `Cargo.toml` and `Cargo.lock`, commits the bump, and pushes
`master`. After CI passes, `.github/workflows/auto-release.yml` creates the
matching `v0.1.1` tag if it does not already exist and dispatches
`.github/workflows/release.yml`. The release workflow then publishes GitHub
Release assets, GHCR images, Docker Hub images when configured, and the
Homebrew/Linuxbrew tap formula.

Manual tag releases still work when the tag matches `Cargo.toml`:

```bash
git tag v0.1.1
git push origin v0.1.1
```

Or republish an existing Cargo version manually:

```bash
gh workflow run release.yml -f version=v0.1.0
```

GHCR publishing uses the built-in `GITHUB_TOKEN`. To publish to Docker Hub, configure:

```text
Repository variable: DOCKERHUB_USERNAME
Repository variable: DOCKER_PLATFORMS=linux/amd64
Repository secret:   DOCKERHUB_TOKEN
```

Docker images are built through the multi-stage Dockerfile, so release image
builds do not depend on the GitHub runner's Rust version. The workflow pushes
GHCR and Docker Hub tags from one Buildx build when Docker Hub credentials are
available, and uses GitHub Actions layer cache for Cargo and Docker layers.

`DOCKER_PLATFORMS` defaults to `linux/amd64`. Set it to `linux/amd64,linux/arm64` when you want multi-architecture Docker images; the first multi-arch build takes longer because Rust is compiled inside Docker for each target platform.

To update a Homebrew/Linuxbrew tap, create or reuse the shared tap repository `nickfan/homebrew-tap` and configure:

```text
Repository variable: HOMEBREW_TAP_REPO=nickfan/homebrew-tap
Repository secret:   HOMEBREW_TAP_TOKEN=<PAT with contents write access to the tap repository>
```

After these values are configured, the tap is maintained by `.github/workflows/release.yml`; do not edit `Formula/starsync.rb` by hand for normal releases.

The generated formula builds from the release vendored source tarball with `cargo install --locked --offline`, which keeps Homebrew/Linuxbrew builds reproducible and independent from the live crates.io index. It also declares a `service do` block, so `brew services start starsync` runs `starsync serve` as a background service.

Useful references:

- [GitHub Actions: publishing Docker images](https://docs.github.com/en/actions/use-cases-and-examples/publishing-packages/publishing-docker-images)
- [Docker build-push-action](https://github.com/docker/build-push-action)
- [Homebrew Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)
- [How to create and maintain a tap](https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap)

## Development

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build
```
