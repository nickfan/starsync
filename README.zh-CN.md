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

### Homebrew / Linuxbrew

发布 workflow 更新 tap 后，可以用 Homebrew 或 Linuxbrew 安装：

```bash
brew tap nickfan/starsync
brew install starsync
starsync --help
```

推荐的公开 tap 仓库名是 `nickfan/homebrew-starsync`，对应 `brew tap nickfan/starsync`。

### Docker

StarSync 会发布容器镜像到 GHCR；如果仓库 variables 和 secrets 配好了，也可以同步发布到 Docker Hub。

```bash
docker pull ghcr.io/nickfan/starsync:latest
# Docker Hub，配置 DOCKERHUB_USERNAME=nickfan 后：
docker pull docker.io/nickfan/starsync:latest
```

容器默认配置：

```text
STARSYNC_DATA_DIR=/data
STARSYNC_STATE_DIR=/state
STARSYNC_BIND=0.0.0.0:8989
```

用宿主机路径持久化运行 REST 服务：

```bash
mkdir -p "$HOME/.starsync/data" "$HOME/.starsync/state"

docker run --rm -it \
  --name starsync \
  --env-file .env \
  -p 8989:8989 \
  -v "$HOME/.starsync/data:/data" \
  -v "$HOME/.starsync/state:/state" \
  ghcr.io/nickfan/starsync:latest
```

同一份挂载数据也可以跑一次性 CLI 命令：

```bash
docker run --rm -it \
  --env-file .env \
  -v "$HOME/.starsync/data:/data" \
  -v "$HOME/.starsync/state:/state" \
  ghcr.io/nickfan/starsync:latest sync

docker run --rm -it \
  --env-file .env \
  -v "$HOME/.starsync/data:/data" \
  -v "$HOME/.starsync/state:/state" \
  ghcr.io/nickfan/starsync:latest search rust
```

Docker 场景下 `.env` 可以很简单：

```dotenv
STARSYNC_GITHUB_TOKEN=github_pat_xxx
```

### Cargo

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

### 使用 GitHub CLI

GitHub CLI 目前不能在终端里直接创建 fine-grained PAT。为了给 StarSync 使用最小权限 token，推荐打开上面的 fine-grained PAT 创建页，创建只有 `Starring: read` 权限的 token。

`gh` 仍然有两种有用方式。

从终端打开 token 创建页：

```bash
gh browse 'https://github.com/settings/personal-access-tokens/new?name=StarSync&description=StarSync%20local-first%20starred%20repository%20sync&expires_in=90&starring=read&contents=read'
```

或者复用 GitHub CLI 当前账号的 OAuth token：

```bash
gh auth login --web
gh auth status
export STARSYNC_GITHUB_TOKEN="$(gh auth token)"
starsync sync
```

`gh auth token` 这条路径很方便，但它不是 StarSync 专用的 fine-grained PAT。GitHub CLI 会为当前账号保存 OAuth token；`gh auth login` 有自己的最小 scopes，`gh auth refresh --scopes ...` 可以额外申请 OAuth scopes。如果你更在意最小权限，优先使用上面的 fine-grained PAT 链接。

官方参考：

- [GitHub starring REST API](https://docs.github.com/en/rest/activity/starring?apiVersion=2022-11-28)
- [Managing personal access tokens](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)
- [GitHub CLI auth login](https://cli.github.com/manual/gh_auth_login)
- [GitHub CLI auth token](https://cli.github.com/manual/gh_auth_token)

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
starsync search 'owner:nickfan AND name:^T'
starsync search '(language:Rust AND topic:cli) OR tag:agent'
starsync search 'language:Rust -topic:web stars:>=1000'
```

搜索语法尽量贴近 GitHub qualifier 风格：

- 布尔操作：`AND`、`OR`、`NOT`、括号，以及相邻条件的隐式 `AND`。
- 否定简写：`-topic:web` 等价于 `NOT topic:web`。
- Qualifiers：`owner:`、`user:`、`org:`、`name:`、`repo:`、`language:`、`topic:`、`tag:`、`status:`、`archived:`、`current:`、`is:`、`stars:`、`description:`、`summary:`、`notes:`、`readme:`。
- 本地前缀匹配：`name:^T` 或 `name:T*`。
- 等值写法：`owner:nickfan`、`owner=nickfan`、`owner:=nickfan`。
- stars 数值比较：`stars:>=1000`、`stars:<500`、`stars:100..500`。

GitHub 官方 starred list endpoint 本身只提供基础分页和 sort/direction，所以这些更丰富的表达式由 StarSync 在本地 synced mirror 加 Markdown meta 上执行。

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

## 发布自动化

仓库已经包含 GitHub Actions：

- `.github/workflows/ci.yml`：在 `master`、pull request、手动触发时运行 format、tests、clippy。
- `.github/workflows/release.yml`：在 `v0.1.0` 这种 tag 或手动触发时运行。
- Release workflow 会创建或更新 GitHub Release，上传 Linux binary tarball、vendored source tarball，发布 GHCR 镜像；如果 variables/secrets 配好，也会发布 Docker Hub 镜像并更新 Homebrew/Linuxbrew tap formula。

发下一版时，推一个和 `Cargo.toml` 匹配的版本 tag：

```bash
git tag v0.1.1
git push origin v0.1.1
```

也可以手动重发当前 Cargo 版本：

```bash
gh workflow run release.yml -f version=v0.1.0
```

GHCR 使用内置 `GITHUB_TOKEN` 发布。发布 Docker Hub 需要配置：

```text
Repository variable: DOCKERHUB_USERNAME
Repository variable: DOCKER_PLATFORMS=linux/amd64
Repository secret:   DOCKERHUB_TOKEN
```

`DOCKER_PLATFORMS` 默认是 `linux/amd64`。如果需要多架构镜像，可以改成 `linux/amd64,linux/arm64`；第一次 multi-arch 构建会更慢，因为 Rust 会在 Docker 里按目标平台分别编译。

更新 Homebrew/Linuxbrew tap 时，先创建类似 `nickfan/homebrew-starsync` 的 tap 仓库，然后配置：

```text
Repository variable: HOMEBREW_TAP_REPO=nickfan/homebrew-starsync
Repository secret:   HOMEBREW_TAP_TOKEN=<有 tap 仓库 contents write 权限的 PAT>
```

生成的 formula 会从 GitHub Release 的 vendored source tarball 构建，并使用 `cargo install --locked --offline`，这样 Homebrew/Linuxbrew 构建不依赖实时 crates.io index，复现性更好。

参考资料：

- [GitHub Actions: publishing Docker images](https://docs.github.com/en/actions/use-cases-and-examples/publishing-packages/publishing-docker-images)
- [Docker build-push-action](https://github.com/docker/build-push-action)
- [Homebrew Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)
- [How to create and maintain a tap](https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap)

## 开发

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build
```
