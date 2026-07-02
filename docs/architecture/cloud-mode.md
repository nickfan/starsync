# Cloudflare Cloud Mode

StarSync has two product modes:

- `local`: the current local-first design. Markdown/YAML and the GitHub mirror
  live on local paths, and Tantivy is the derived local search index.
- `cloudflare`: an optional Cloudflare-only mode for hosted personal use.

No extra provider abstraction is planned for now. Keeping the target to
Cloudflare avoids turning a personal knowledge tool into an infrastructure
framework.

## Local Mode

Local mode remains the reference implementation:

```text
~/.starsync/data/repos/      Markdown meta, catalog files
~/.starsync/state/mirror.json
~/.starsync/state/events.jsonl
~/.starsync/state/search/    Tantivy derived index
```

The source of truth is still:

- GitHub starred repositories for the remote star list.
- Markdown/YAML for personal tags, notes, status, links, and summaries.

Everything else is derived and rebuildable.

Local reads may use a small in-process memory cache before touching the
filesystem:

```text
read path:
  memory cache -> filesystem Markdown/mirror/catalog files

write path:
  filesystem write -> invalidate/update memory cache -> rebuild derived index
```

StarSync uses `moka` for this in-process cache. It is the Rust equivalent of
the Caffeine-style pattern in the Java/Spring world: cache hot parsed Markdown,
mirror snapshots, merged repo views, catalog data, and small README snippets;
keep the filesystem as the source of truth. If an external shared cache is ever
useful, it should remain optional and read-through/write-through, not a new
source of truth.

## Cloudflare Mode

Cloudflare mode should be a thin hosted variant, not a new product.

The cloud source of truth is object storage. For the Cloudflare target this
means R2, and the same source-of-truth rule also maps cleanly to
S3-compatible storage if that is ever used outside the Cloudflare deployment.
D1 is never the source of truth; it is a rebuildable derived index.

Cloudflare storage follows the same read-cache/write-source split:

- Reads may go through cache routes or a CDN for catalog artifacts, Markdown
  documents, README snapshots, UI assets, and other immutable or versioned
  objects.
- Writes go to the R2/S3-compatible object storage write endpoint. If latency
  matters, use the object store's write acceleration or nearest write ingress,
  but keep the write path authoritative and separate from the cache/CDN path.
- Cache invalidation follows successful writes or rebuilds. Cache entries may
  speed reads, but they never become the source of truth.

Suggested mapping:

| StarSync data | Cloudflare service | Role |
| --- | --- | --- |
| Markdown repo meta | R2 | Source-of-truth object storage |
| GitHub mirror snapshots | R2 | Source-of-truth mirror/cache objects |
| Catalog JSON/YAML | R2 + CDN | Rebuildable static catalog artifacts |
| Search and structured filters | D1 | Derived cloud search/index layer |
| Event log and webhook delivery queue | Queues | Optional async delivery path |
| Narrow per-user coordination | Durable Objects | Optional only if Cloudflare mode needs it |
| Web UI | Pages or R2 custom domain | Static UI hosting |
| REST/MCP runtime | Cloudflare Containers | Runs the existing Rust service shape |

Tantivy remains the local search implementation. In Cloudflare mode, D1 should
be the default cloud-side replacement for the derived Tantivy index:

```text
R2 object storage source of truth
  Markdown meta + GitHub mirror snapshots
  write path: direct object storage write endpoint
  read path: CDN/cache route when safe
  -> rebuild derived views
D1 repo table + D1 FTS table
  -> REST/UI/MCP cloud search
```

D1 is SQLite-based and supports FTS5, so it is a good fit for personal-scale
cloud search, structured filters, tags, topics, status, and pagination. To keep
Chinese and pinyin behavior close to local Tantivy, StarSync should write the
same derived fields into D1:

- normalized text;
- CJK n-grams;
- pinyin words;
- pinyin initials;
- tags/topics/status/language columns for structured filtering.

That keeps object storage Markdown/mirror data as the source of truth and makes
D1 rebuildable.
Durable Objects are not part of the default personal flow; they are only an
escape hatch if Cloudflare mode later needs one per-user coordinator.

## What We Should Not Add Now

- No generic provider abstraction.
- No generic catalog database abstraction.
- No multi-writer design.
- No Durable Objects in the default flow.

For personal scale, run one writer for sync and metadata updates. If Cloudflare
mode later needs stricter coordination, use one Durable Object for that narrow
job instead of introducing a general coordination layer.

## Implementation Shape

Phase 1 should keep code almost unchanged:

1. Keep `local` as the default and only fully supported mode.
2. Add config names that leave room for `cloudflare`, without implementing a
   broad provider layer.
3. Keep Kubernetes and Docker as local-mode deployment options.
4. Document Cloudflare mode as experimental until R2/D1/Containers wiring exists.

Phase 2 can add Cloudflare-specific commands or config:

```toml
[cloudflare]
enabled = true
account_id = "${CLOUDFLARE_ACCOUNT_ID}"
r2_bucket = "starsync"
d1_database = "starsync"
queue = "starsync-events"
```

The main rule: Cloudflare mode may change storage and hosting, but it must not
change StarSync's source-of-truth model. Object storage holds the truth; D1 and
static catalogs are derived.

## References

- Cloudflare storage options: https://developers.cloudflare.com/workers/platform/storage-options/
- Cloudflare R2: https://developers.cloudflare.com/r2/
- Cloudflare D1: https://developers.cloudflare.com/d1/
- Cloudflare Queues: https://developers.cloudflare.com/queues/
- Cloudflare Durable Objects: https://developers.cloudflare.com/durable-objects/
- Cloudflare Containers: https://developers.cloudflare.com/containers/
- Cloudflare Pages: https://developers.cloudflare.com/pages/
