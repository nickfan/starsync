use crate::{
    api,
    config::{Config, ConfigOverrides, StorageBackendKind},
    mcp,
    models::{MetaPatch, RepoFilters, RepoIdentity, SortDirection},
    openapi,
    service::StarSyncService,
    storage,
};
use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "starsync",
    version,
    about = "Local-first GitHub starred repository knowledge sync"
)]
pub struct Cli {
    #[command(flatten)]
    pub config: ConfigArgs,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args, Debug, Default)]
pub struct ConfigArgs {
    #[arg(long, global = true, env = "STARSYNC_CONFIG")]
    pub config: Option<PathBuf>,
    #[arg(long, global = true)]
    pub env_file: Option<PathBuf>,
    #[arg(long, global = true, env = "STARSYNC_DATA_DIR")]
    pub data_dir: Option<PathBuf>,
    #[arg(long, global = true, env = "STARSYNC_STATE_DIR")]
    pub state_dir: Option<PathBuf>,
    #[arg(long, global = true, env = "STARSYNC_SEARCH_INDEX_DIR")]
    pub search_index_dir: Option<PathBuf>,
    #[arg(long, global = true, env = "STARSYNC_UI_DIR")]
    pub ui_dir: Option<PathBuf>,
    #[arg(long, global = true, env = "STARSYNC_GITHUB_TOKEN")]
    pub github_token: Option<String>,
    #[arg(long, global = true, env = "STARSYNC_BIND")]
    pub bind: Option<String>,
    #[arg(long, global = true, env = "STARSYNC_STORAGE_BACKEND")]
    pub storage_backend: Option<String>,
    #[arg(long, global = true, env = "STARSYNC_GIT_REMOTE")]
    pub git_remote: Option<String>,
    #[arg(long, global = true, env = "STARSYNC_UI_ENABLED")]
    pub ui_enabled: Option<bool>,
    #[arg(long, global = true, env = "STARSYNC_UI_AUTO_EXTRACT")]
    pub ui_auto_extract: Option<bool>,
    #[arg(long, global = true, env = "STARSYNC_UI_OVERWRITE")]
    pub ui_overwrite: Option<bool>,
    #[arg(long, global = true, env = "STARSYNC_UI_BACKUP")]
    pub ui_backup: Option<bool>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Init,
    Sync,
    Enrich {
        #[command(subcommand)]
        command: EnrichCommand,
    },
    List(FilterArgs),
    Search(SearchArgs),
    Meta {
        #[command(subcommand)]
        command: MetaCommand,
    },
    Serve(ServeArgs),
    Mcp,
    Storage {
        #[command(subcommand)]
        command: StorageCommand,
    },
    Openapi {
        #[command(subcommand)]
        command: OpenApiCommand,
    },
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },
    Index {
        #[command(subcommand)]
        command: IndexCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum EnrichCommand {
    Readme {
        #[arg(long)]
        limit: Option<usize>,
    },
}

#[derive(Args, Debug, Default)]
pub struct FilterArgs {
    #[arg(long)]
    pub q: Option<String>,
    #[arg(long)]
    pub owner: Option<String>,
    #[arg(long)]
    pub language: Option<String>,
    #[arg(long)]
    pub topic: Option<String>,
    #[arg(long)]
    pub tag: Option<String>,
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long)]
    pub archived: Option<bool>,
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long)]
    pub cursor: Option<String>,
    #[arg(long)]
    pub page: Option<usize>,
    #[arg(long)]
    pub per_page: Option<usize>,
    #[arg(long)]
    pub sort: Option<String>,
    #[arg(long)]
    pub direction: Option<String>,
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    pub query: String,
    #[command(flatten)]
    pub filters: FilterArgs,
}

#[derive(Subcommand, Debug)]
pub enum MetaCommand {
    Edit {
        owner: String,
        repo: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        archived: Option<bool>,
    },
    Delete {
        owner: String,
        repo: String,
    },
}

#[derive(Args, Debug, Default)]
pub struct ServeArgs {
    #[arg(long)]
    pub no_ui: bool,
    #[arg(long)]
    pub ui_dir: Option<PathBuf>,
    #[arg(long)]
    pub no_ui_extract: bool,
    #[arg(long)]
    pub no_ui_overwrite: bool,
    #[arg(long)]
    pub no_ui_backup: bool,
}

#[derive(Subcommand, Debug)]
pub enum StorageCommand {
    Pull,
    Push,
}

#[derive(Subcommand, Debug)]
pub enum OpenApiCommand {
    Export {
        #[arg(long, value_enum, default_value_t = OpenApiFormat::Yaml)]
        format: OpenApiFormat,
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum OpenApiFormat {
    Json,
    Yaml,
}

#[derive(Subcommand, Debug)]
pub enum SkillCommand {
    Export {
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub enum IndexCommand {
    Rebuild,
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut config = Config::load(cli.config.into_overrides()?)?;
    if let Command::Serve(args) = &cli.command {
        args.apply_to_config(&mut config);
    }
    let service = StarSyncService::new(config.clone());

    match cli.command {
        Command::Init => {
            service.init()?;
            print_json(&serde_json::json!({
                "data_dir": config.data_dir,
                "repos_dir": config.repos_dir(),
                "search_index_dir": config.search_index_dir(),
                "ui_dir": config.ui_dir,
                "state_dir": config.state_dir
            }))?;
        }
        Command::Sync => {
            print_json(&service.sync().await?)?;
        }
        Command::Enrich {
            command: EnrichCommand::Readme { limit },
        } => {
            print_json(&serde_json::json!({ "updated": service.enrich_readmes(limit).await? }))?;
        }
        Command::List(filters) => {
            print_json(&service.list_repos(filters.into_repo_filters(None)?)?)?;
        }
        Command::Search(args) => {
            print_json(&service.search_repos(args.filters.into_repo_filters(Some(args.query))?)?)?;
        }
        Command::Meta { command } => match command {
            MetaCommand::Edit {
                owner,
                repo,
                tags,
                status,
                summary,
                notes,
                archived,
            } => {
                let patch = MetaPatch {
                    tags: (!tags.is_empty()).then_some(tags),
                    status: status.map(Some),
                    summary: summary.map(Some),
                    notes: notes.map(Some),
                    archived,
                    ..MetaPatch::default()
                };
                print_json(&service.patch_meta(&RepoIdentity::new(owner, repo), patch)?)?;
            }
            MetaCommand::Delete { owner, repo } => {
                print_json(&service.delete_meta(&RepoIdentity::new(owner, repo))?)?;
            }
        },
        Command::Serve(_) => api::serve(service).await?,
        Command::Mcp => mcp::run_stdio(service).await?,
        Command::Storage { command } => match command {
            StorageCommand::Pull => {
                print_json(&serde_json::json!({ "message": storage::pull(&config).await? }))?
            }
            StorageCommand::Push => {
                print_json(&serde_json::json!({ "message": storage::push(&config).await? }))?
            }
        },
        Command::Openapi {
            command: OpenApiCommand::Export { format, output },
        } => {
            let content = match format {
                OpenApiFormat::Json => serde_json::to_string_pretty(&openapi::openapi_json())?,
                OpenApiFormat::Yaml => openapi::openapi_yaml()?,
            };
            write_or_print(output, content)?;
        }
        Command::Skill {
            command: SkillCommand::Export { output },
        } => {
            let dir = output.unwrap_or_else(|| PathBuf::from("starsync-skill"));
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join("SKILL.md"), skill_markdown())?;
            print_json(&serde_json::json!({ "skill_dir": dir }))?;
        }
        Command::Index {
            command: IndexCommand::Rebuild,
        } => {
            service.rebuild_index()?;
            print_json(&serde_json::json!({ "rebuilt": true }))?;
        }
    }
    Ok(())
}

impl ConfigArgs {
    fn into_overrides(self) -> Result<ConfigOverrides> {
        Ok(ConfigOverrides {
            config_path: self.config,
            env_file: self.env_file,
            data_dir: self.data_dir,
            state_dir: self.state_dir,
            search_index_dir: self.search_index_dir,
            ui_dir: self.ui_dir,
            bind: self.bind,
            github_token: self.github_token,
            storage_backend: self
                .storage_backend
                .as_deref()
                .map(StorageBackendKind::parse)
                .transpose()?,
            git_remote: self.git_remote,
            ui_enabled: self.ui_enabled,
            ui_auto_extract: self.ui_auto_extract,
            ui_overwrite: self.ui_overwrite,
            ui_backup: self.ui_backup,
        })
    }
}

impl ServeArgs {
    fn apply_to_config(&self, config: &mut Config) {
        if self.no_ui {
            config.ui_enabled = false;
        }
        if let Some(ui_dir) = &self.ui_dir {
            config.ui_dir = ui_dir.clone();
        }
        if self.no_ui_extract {
            config.ui_auto_extract = false;
        }
        if self.no_ui_overwrite {
            config.ui_overwrite = false;
        }
        if self.no_ui_backup {
            config.ui_backup = false;
        }
    }
}

impl FilterArgs {
    fn into_repo_filters(self, q: Option<String>) -> Result<RepoFilters> {
        Ok(RepoFilters {
            q: q.or(self.q),
            owner: self.owner,
            language: self.language,
            topic: self.topic,
            tag: self.tag,
            status: self.status,
            archived: self.archived,
            limit: self.limit,
            cursor: self.cursor,
            page: self.page,
            per_page: self.per_page,
            sort: self.sort.as_deref().map(parse_sort).transpose()?,
            direction: self.direction.as_deref().map(parse_direction).transpose()?,
        })
    }
}

fn parse_sort(value: &str) -> Result<crate::models::RepoSort> {
    match value {
        "created" => Ok(crate::models::RepoSort::Created),
        "updated" => Ok(crate::models::RepoSort::Updated),
        "name" => Ok(crate::models::RepoSort::Name),
        "stars" => Ok(crate::models::RepoSort::Stars),
        other => anyhow::bail!("unsupported sort: {other}"),
    }
}

fn parse_direction(value: &str) -> Result<SortDirection> {
    match value {
        "asc" => Ok(SortDirection::Asc),
        "desc" => Ok(SortDirection::Desc),
        other => anyhow::bail!("unsupported direction: {other}"),
    }
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn write_or_print(output: Option<PathBuf>, content: String) -> Result<()> {
    if let Some(path) = output {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(path, content)?;
    } else {
        println!("{content}");
    }
    Ok(())
}

fn skill_markdown() -> &'static str {
    r#"---
name: starsync
description: Use local StarSync CLI, REST, or MCP to search and maintain a personal GitHub starred repository knowledge base.
---

# StarSync Skill

Use StarSync when the user asks to search, browse, summarize, tag, or maintain their local GitHub starred repository knowledge base.

## Rules

- Prefer MCP tools when available: `search_repos`, `list_repos`, `get_repo`, `update_repo_meta`, `sync_stars`, `enrich_readme`.
- Use the CLI when MCP is unavailable: `starsync search`, `starsync list`, `starsync meta edit`, `starsync sync`.
- Never star or unstar GitHub repositories. StarSync writes local Markdown meta only.
- Treat Markdown/YAML files under the configured data directory as the source of truth for personal tags, notes, status, and links.
- Use REST OpenAPI from `GET /openapi.yaml` or `starsync openapi export` for integration work.
"#
}
