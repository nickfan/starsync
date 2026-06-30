# StarSync

StarSync 是一个 local-first 的 Rust 服务，用来把你的 GitHub starred repositories 变成可检索的个人知识库。

它会镜像 GitHub starred repo 清单，把你的个人 meta 信息保存在 Markdown/YAML 中，同时提供 CLI、REST、OpenAPI、SSE 事件流和 stdio MCP Server。SQLite FTS 是可选的检索加速层，可以随时从 Markdown 和远程 mirror 状态重建。

> StarSync v1 不会在 GitHub 上执行 star 或 unstar。GitHub 是远程 star 清单的事实源；本地 Markdown 是 tags、notes、status、links 等个人 meta 的事实源。

## 功能

- 同步 GitHub starred repositories 到本地 mirror。
- 在 `~/.starsync/data/repos` 下用 Markdown front matter 保存个人 meta。
- 检索 GitHub repo 信息、本地 tags/notes/status、可选 README 摘要。
- 通过 CLI、REST API、OpenAPI 3.1、SSE events、stdio MCP Server 浏览和搜索。
- 默认使用本地文件存储，也支持 Git-backed metadata storage，方便分享或备份。
- SQLite FTS 是派生索引，可以随时 rebuild。

## 安装

```bash
cargo build --release
```

在当前 checkout 中运行：

```bash
cargo run -- --help
```

或者使用构建后的二进制：

```bash
./target/release/starsync --help
```

## GitHub Token

只有调用 GitHub API 时才需要 GitHub personal access token，例如 `sync` 和 README enrichment。

v1 推荐权限：

- Fine-grained personal access token
- Account permission：`Starring: read`
- 可选 repository permission：`Contents: read`，用于 enrichment 可访问 private repo 的 README
- Expiration：建议设置合理有效期，例如 90 天

创建预填好的只读 token：

[创建 StarSync 只读 PAT](https://github.com/settings/personal-access-tokens/new?name=StarSync&description=StarSync%20local-first%20starred%20repository%20sync&expires_in=90&starring=read&contents=read)

如果你想创建一个为未来 star/unstar 写能力预留的 token，可以用：

[创建 StarSync star 读写 PAT](https://github.com/settings/personal-access-tokens/new?name=StarSync%20Read%20Write&description=StarSync%20future%20star%20read-write%20token&expires_in=90&starring=write&contents=read)

即使 token 有 `Starring: write`，StarSync v1 也不会写 GitHub star/unstar 状态。

官方参考：

- [GitHub starring REST API](https://docs.github.com/en/rest/activity/starring?apiVersion=2022-11-28)
- [Managing personal access tokens](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)

## 配置 Token

容器化或本机运行时，优先推荐环境变量：

```bash
export STARSYNC_GITHUB_TOKEN=github_pat_xxx
```

也可以使用 `.env` 文件：

```dotenv
STARSYNC_GITHUB_TOKEN=github_pat_xxx
STARSYNC_DATA_DIR=/path/to/starsync/data
STARSYNC_STATE_DIR=/path/to/starsync/state
STARSYNC_BIND=127.0.0.1:8989
```

然后运行：

```bash
starsync --env-file .env sync
```

配置加载优先级：

1. CLI 参数
2. 进程环境变量
3. `.env` 文件
4. `config.toml`
5. 内置默认值

支持的环境变量：

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

`config.toml` 支持环境变量插值：

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

不要把 token 提交进 metadata Git 仓库。

## 快速开始

初始化本地目录：

```bash
starsync init
```

同步 GitHub starred repositories：

```bash
starsync sync
```

列出融合后的 repo + meta：

```bash
starsync list --limit 20
starsync list --language Rust --tag ai --sort updated --direction desc
```

搜索本地和远程字段：

```bash
starsync search retrieval
starsync search "agent framework" --archived true
```

只编辑本地 meta，不影响 GitHub：

```bash
starsync meta edit owner repo \
  --tag rust \
  --tag ai \
  --status evaluating \
  --summary "Worth tracking for local agent tooling"
```

归档本地 meta，不影响 GitHub：

```bash
starsync meta delete owner repo
```

从 Markdown 和 mirror 状态重建 SQLite FTS：

```bash
starsync index rebuild
```

为当前 starred repos 抓取 README 摘要：

```bash
starsync enrich readme --limit 50
```

## 数据目录

默认路径：

```text
~/.starsync/data/repos/INDEX.md
~/.starsync/data/repos/{owner}/{repo}/INDEX.md
~/.starsync/state/mirror.json
~/.starsync/state/starsync.db
```

单个 repo 的 `INDEX.md` 用 YAML front matter 保存本地 meta：

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

Markdown/YAML 是个人 meta 的事实源。SQLite 只是派生索引，可以重建。

## REST API

启动本地 REST 服务：

```bash
starsync serve
```

默认地址：

```text
http://127.0.0.1:8989
```

常用接口：

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

示例：

```bash
curl 'http://127.0.0.1:8989/repos?limit=20&language=Rust&sort=updated&direction=desc'
curl 'http://127.0.0.1:8989/search?q=retrieval&tag=ai'
```

导出 OpenAPI 3.1：

```bash
starsync openapi export --format yaml --output openapi.yaml
starsync openapi export --format json --output openapi.json
```

## MCP 和 Agent 用法

启动 stdio MCP Server：

```bash
starsync mcp
```

可用 MCP tools：

- `search_repos`
- `list_repos`
- `get_repo`
- `update_repo_meta`
- `sync_stars`
- `enrich_readme`
- `list_recent_events`

Resources：

- `starsync://index`
- `starsync://repo/{owner}/{repo}`

导出 Agent Skill：

```bash
starsync skill export --output ./starsync-skill
```

生成的 Skill 会引导 Agent 优先使用 MCP，必要时使用 CLI/REST，并明确禁止写 GitHub star 状态。

## Git-backed metadata storage

默认是本地文件存储。如果要通过 Git 同步 Markdown meta：

```bash
export STARSYNC_STORAGE_BACKEND=git
export STARSYNC_GIT_REMOTE=git@github.com:you/starsync-meta.git

starsync storage pull
starsync storage push
```

Git storage 命令只会 stage `repos/` 下的 metadata。Token 和 SQLite 派生状态不会参与 Git metadata sync。

## 开发

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build
```
