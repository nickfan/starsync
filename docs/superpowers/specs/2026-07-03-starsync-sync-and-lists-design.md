# StarSync Sync Optimization And Lists Design

Date: 2026-07-03

Status: Draft for review

## Context

StarSync is a personal starred repository knowledge base. The source of truth is
split deliberately:

- GitHub starred repositories are the remote membership source.
- Local Markdown front matter and per-repo notes are the personal metadata source.
- Tantivy is a derived local search index and can be rebuilt.
- Catalog YAML/JSON/Markdown files are derived quick-read indexes for humans,
  scripts, grep, and lightweight clients.

The current sync path already handles the important correctness behavior:
repositories removed from GitHub stars are marked non-current while preserving
local metadata. For 1,000 to 10,000 starred repositories, the set diff itself is
not the main bottleneck. The expensive work is repeated full derived-output
generation when the fetched remote state did not materially change.

GitHub Star Lists are available through official GraphQL fields
(`User.lists -> UserList.items -> Repository`), while the REST starring API does
not expose list membership. Repository objects do not expose a direct reverse
`lists` field, so list membership must be built by enumerating lists and their
items, then joining that data against the local starred mirror.

## Goals

- Keep the existing personal-scale architecture: local files first, Tantivy as
  a derived index, no distributed coordination, no database as truth source.
- Make no-change syncs cheap by skipping catalog and Tantivy rebuilds when the
  merged remote state and local metadata have not changed.
- Add first-class list filtering without depending on GitHub Star Lists as a
  hard requirement.
- Let UI tag/list chips add filters to the current query quickly and visibly.
- Support GitHub Star Lists as optional enrichment, joined to currently synced
  starred repositories.
- Preserve compatibility with CLI, REST, MCP, local Markdown, Docker, Homebrew,
  and system services.

## Non-Goals

- No GitHub write support for assigning repositories to GitHub Star Lists in
  this iteration.
- No scraping of GitHub web pages for lists.
- No distributed lease, lock service, Redis, or multi-writer cloud control
  plane.
- No fallback database layer. Tantivy remains the search acceleration layer.
- No remote list state as mandatory truth. Local metadata must still be useful
  offline.

## Recommended Design

### 1. Sync Fast Path

Keep the current merge semantics, but add a changed-set planning phase before
writing derived outputs.

Inputs:

- Previous mirror state from `mirror.json`.
- Newly fetched GitHub starred repos.
- Existing local Markdown metadata.

Plan output:

- `added`: repositories not previously current.
- `updated`: repositories whose normalized remote fields changed.
- `removed`: repositories that were current before but are absent from the new
  fetch.
- `unchanged`: repositories with equivalent normalized remote fields.
- `changed_full_names`: union of added, updated, and removed names.
- `requires_derived_rebuild`: true when remote state changed, local meta changed,
  README enrichment changed, or catalog/search schema version changed.

The no-change path should:

- Update `last_sync_at` only when we want to record contact with GitHub.
- Avoid rewriting per-repo Markdown.
- Avoid rewriting catalog YAML/JSON/Markdown.
- Avoid rebuilding Tantivy.
- Emit a concise event such as `sync.no_changes`.

The change path should:

- Ensure per-repo Markdown only for new repos or repos whose identity/source
  block changed.
- Preserve tombstoned local meta for removed repos.
- Rebuild derived outputs once at the end.
- Report changed counts and timings in events.

For v1.5-scale optimization, a full Tantivy rebuild on real changes is still
acceptable for 10,000 repos. Incremental Tantivy updates can come later if
benchmarks show rebuilds dominate real workloads.

### 2. Remote Fingerprint

Add a stable digest over normalized remote membership data. The digest should
ignore fields that are generated locally, such as `last_sync_at`, and should be
stable across serialization order.

Suggested normalized fields:

- GitHub id
- owner
- name
- full name
- html url
- description
- language
- topics
- stars count
- forks count
- archived flag
- pushed/updated/starred timestamps when available
- current flag after merge

Store the digest in mirror state or a small state file under the state dir. This
lets the service detect no-change runs clearly and gives tests a simple
assertion boundary.

### 3. Lists Data Model

Introduce two separate list namespaces:

- `user.lists`: local user-maintained lists stored in Markdown front matter.
- `source.github_lists`: GitHub Star Lists imported through GraphQL enrichment.

Merged `RepoView` should expose both fields and optionally a combined
`lists_all` for display/search convenience.

Example Markdown front matter:

```yaml
user:
  tags:
    - rust
    - cli
  lists:
    - toolkit
    - search
source:
  github_lists:
    - toolkit
```

Rules:

- `user.lists` is editable through CLI, REST, MCP, and Markdown.
- `source.github_lists` is managed by enrichment and should not be edited by
  local meta commands unless explicitly requested by a repair/import command.
- Filtering by `list:toolkit` matches either namespace.
- Filtering by `user_list:toolkit` matches only `user.lists`.
- Filtering by `github_list:toolkit` matches only `source.github_lists`.

### 4. GitHub Star Lists Enrichment

Add a best-effort official GraphQL enrichment command/job:

- CLI: `starsync enrich lists`
- REST: `POST /enrich/lists`
- MCP: `enrich_lists`

Fetch strategy:

1. Query `viewer.lists(first: 100)`.
2. For each list, page through `items(first: 100)`.
3. Keep only `Repository` nodes.
4. Build `repository_id/nameWithOwner -> [list_slug]`.
5. Join against the local starred mirror.
6. Write joined slugs into `source.github_lists`.
7. Emit summary events:
   - total lists
   - total list items
   - matched starred repos
   - unmatched repos
   - duration

If a listed repository is not in the local starred mirror, do not add it to the
main catalog automatically. Record it as an enrichment warning or optional
diagnostic, because StarSync's repo catalog is intentionally the synced starred
mirror plus preserved local tombstones.

Token scope:

- Existing GitHub token used for starred repo sync should be reused.
- Public lists should work with normal authenticated GraphQL access.
- Private list behavior should be documented after verification against token
  scopes; failures should degrade to a clear warning event.

### 5. Query And Filter Semantics

Extend structured search with list fields:

- `list:toolkit`
- `user_list:toolkit`
- `github_list:toolkit`

These filters should compose with the existing query language:

- `owner:nickfan AND list:toolkit`
- `tag:rust OR list:cli`
- `name:T* AND github_list:toolkit`

API filters should mirror the query language:

- `GET /repos?list=toolkit`
- `GET /repos?user_list=toolkit`
- `GET /repos?github_list=toolkit`
- `GET /search?q=list:toolkit%20rust`

Sorting must apply to the full filtered result set before pagination, not only
the current page. This preserves the existing expectation that changing sort or
filters is a dataset operation.

### 6. UI Behavior

Tags and lists should be interactive chips:

- Clicking a tag adds `tag:<value>` to the active filters.
- Clicking a user list adds `user_list:<value>`.
- Clicking a GitHub list adds `github_list:<value>`.
- A combined list chip can add `list:<value>` when the UI does not need to show
  provenance.
- Active filters should appear as removable pills near the query controls.

The sort control should stay as a simple preset dropdown:

- Recent updated
- Recently starred
- Most stars
- Most forked
- Name A-Z
- Name Z-A

The UI should send query/filter/sort changes to the backend instead of sorting
only currently visible rows.

### 7. Events And Background Jobs

Reuse the existing async background task model for sync and README enrichment.
Add list enrichment as another job type.

Recommended events:

- `sync.started`
- `sync.no_changes`
- `sync.completed`
- `sync.failed`
- `enrich.lists.started`
- `enrich.lists.completed`
- `enrich.lists.failed`
- `index.rebuild.started`
- `index.rebuild.completed`

The UI should subscribe to events and show short-lived notifications plus a
compact status area for the latest job.

### 8. Tests

Sync:

- No-change sync skips catalog write and Tantivy rebuild.
- Added repo creates/ensures Markdown and triggers one derived rebuild.
- Updated repo triggers one derived rebuild.
- Removed repo is marked non-current and local metadata remains searchable.
- Remote fingerprint is stable across input ordering.

Lists:

- `user.lists` front matter round trips.
- `source.github_lists` enrichment round trips without overwriting user lists.
- `list:` matches both user and GitHub lists.
- `user_list:` and `github_list:` are namespace-specific.
- List filters compose with owner, tag, status, language, and sorting.

GitHub GraphQL:

- Mock paginated `viewer.lists`.
- Mock paginated `UserList.items`.
- Ignore non-Repository union nodes defensively.
- Join list items to local mirror by GitHub id first, then nameWithOwner.

UI/API:

- Chip click adds the expected filter.
- Sorting applies to full filtered dataset before pagination.
- REST and MCP expose list enrichment consistently.

## Rollout Plan

1. Add data model fields and Markdown round-trip tests.
2. Add search/query list filters and REST parameters.
3. Add UI filter chips and active filter pills.
4. Add GraphQL client support for `viewer.lists`.
5. Add async `enrich lists` job for CLI, REST, and MCP.
6. Add sync changed-set/fingerprint fast path.
7. Benchmark 1,000 and 10,000 repo fixtures before deciding whether incremental
   Tantivy updates are necessary.

## Open Questions

- Whether `source.github_lists` should be written into each per-repo `INDEX.md`
  or into the remote mirror/catalog only. The recommended default is per-repo
  front matter because it keeps grep/export useful, but it means enrichment can
  touch many Markdown files.
- Whether private GitHub Star Lists need additional token guidance beyond the
  existing authenticated GraphQL token.
- Whether local `user.lists` should get dedicated CLI subcommands
  (`meta list add/remove`) or be folded into existing meta edit workflows first.
