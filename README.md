# StarSync

StarSync is a local-first Rust service for turning your GitHub starred repositories into a searchable personal knowledge base.

It mirrors your GitHub starred repository list, keeps your personal metadata in Markdown/YAML, exposes CLI + REST + MCP interfaces, and can optionally build a SQLite FTS index for faster search.

> StarSync v1 never stars or unstars repositories on GitHub. GitHub is the remote source for the star list; local Markdown is the source of truth for your tags, notes, status, and links.

## Features

- Sync GitHub starred repositories into a local mirror.
- Store personal metadata in Markdown front matter under `~/.starsync/data/repos`.
- Search merged GitHub repo facts, local tags/notes/status, and optional README snippets.
- Browse and search through CLI, REST API, OpenAPI 3.1, SSE events, and a stdio MCP server.
- Use local filesystem storage by default, with Git-backed metadata storage available for sharing or backup.
- Rebuild optional SQLite FTS from Markdown and mirror state at any time.

## Install

```bash
cargo build --release
```

Run from the checkout:

```bash
cargo run -- --help
```

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
STARSYNC_SQLITE_ENABLED
```

`config.toml` may reference environment variables:

```toml
data_dir = "~/.starsync/data"
state_dir = "~/.starsync/state"
bind = "127.0.0.1:8989"
sqlite_enabled = true

[github]
token = "${STARSYNC_GITHUB_TOKEN}"

[storage]
backend = "local"
# backend = "git"
# git_remote = "git@github.com:you/starsync-meta.git"
```

Never commit tokens into the metadata Git repository.

## Quick start

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

Refresh the optional SQLite FTS index from Markdown and mirror state:

```bash
starsync index rebuild
```

Fetch README snippets for current starred repositories:

```bash
starsync enrich readme --limit 50
```

## Data layout

Default paths:

```text
~/.starsync/data/repos/INDEX.md
~/.starsync/data/repos/{owner}/{repo}/INDEX.md
~/.starsync/state/mirror.json
~/.starsync/state/starsync.db
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

Markdown/YAML is the personal metadata source of truth. SQLite is an optional derived index and can be rebuilt.

## REST API

Start the local REST service:

```bash
starsync serve
```

Default bind address:

```text
http://127.0.0.1:8989
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
GET  /openapi.yaml
GET  /openapi.json
```

Example:

```bash
curl 'http://127.0.0.1:8989/repos?limit=20&language=Rust&sort=updated&direction=desc'
curl 'http://127.0.0.1:8989/search?q=retrieval&tag=ai'
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

Only metadata under `repos/` is staged by the Git storage command. Tokens and derived SQLite state are not part of the Git metadata sync.

## Development

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build
```
