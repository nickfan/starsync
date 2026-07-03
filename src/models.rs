use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct RepoIdentity {
    pub owner: String,
    pub name: String,
}

impl RepoIdentity {
    pub fn new(owner: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            name: name.into(),
        }
    }

    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

impl fmt::Display for RepoIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RemoteRepo {
    pub github_id: i64,
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub language: Option<String>,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub stargazers_count: u64,
    #[serde(default)]
    pub forks_count: u64,
    pub default_branch: Option<String>,
    pub pushed_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub starred_at: Option<DateTime<Utc>>,
    #[serde(default = "default_true")]
    pub current: bool,
    #[serde(default)]
    pub archived: bool,
}

fn default_true() -> bool {
    true
}

impl RemoteRepo {
    pub fn identity(&self) -> RepoIdentity {
        RepoIdentity::new(self.owner.clone(), self.name.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RepoMeta {
    pub starsync: SchemaMeta,
    pub kind: String,
    pub repo: RepoIdentity,
    pub source: RepoSource,
    #[serde(default)]
    pub user: UserMeta,
    #[serde(default)]
    pub archived: bool,
}

impl RepoMeta {
    pub fn new(identity: RepoIdentity) -> Self {
        Self {
            starsync: SchemaMeta::default(),
            kind: "repo".to_string(),
            repo: identity,
            source: RepoSource::default(),
            user: UserMeta::default(),
            archived: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SchemaMeta {
    pub schema: String,
}

impl Default for SchemaMeta {
    fn default() -> Self {
        Self {
            schema: "starsync.repo.v1".to_string(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RepoSource {
    pub github_id: Option<i64>,
    pub html_url: Option<String>,
    #[serde(default)]
    pub github_lists: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UserMeta {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub lists: Vec<String>,
    pub status: Option<String>,
    pub summary: Option<String>,
    pub notes: Option<String>,
    #[serde(default)]
    pub links: Vec<UserLink>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UserLink {
    pub label: String,
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RepoView {
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub html_url: Option<String>,
    pub description: Option<String>,
    pub language: Option<String>,
    #[serde(default)]
    pub topics: Vec<String>,
    pub stargazers_count: Option<u64>,
    pub forks_count: Option<u64>,
    pub default_branch: Option<String>,
    pub pushed_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub starred_at: Option<DateTime<Utc>>,
    pub current: bool,
    pub archived: bool,
    pub user: UserMeta,
    #[serde(default)]
    pub github_lists: Vec<String>,
    pub readme_snippet: Option<String>,
}

impl RepoView {
    pub fn identity(&self) -> RepoIdentity {
        RepoIdentity::new(self.owner.clone(), self.name.clone())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadmeCacheEntry {
    pub owner: String,
    pub name: String,
    pub fetched_at: DateTime<Utc>,
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MirrorState {
    #[serde(default)]
    pub repos: Vec<RemoteRepo>,
    #[serde(default)]
    pub readmes: Vec<ReadmeCacheEntry>,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub last_etag: Option<String>,
    pub remote_digest: Option<String>,
    pub derived_digest: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RepoFilters {
    pub q: Option<String>,
    pub owner: Option<String>,
    pub language: Option<String>,
    pub topic: Option<String>,
    pub tag: Option<String>,
    pub list: Option<String>,
    pub user_list: Option<String>,
    pub github_list: Option<String>,
    pub status: Option<String>,
    pub archived: Option<bool>,
    pub sort: Option<RepoSort>,
    pub direction: Option<SortDirection>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub page: Option<usize>,
    pub per_page: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RepoSort {
    Created,
    #[default]
    Updated,
    Name,
    Stars,
    Forks,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    Asc,
    #[default]
    Desc,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListResponse<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub repo: RepoView,
    pub score: f32,
    pub matched_fields: Vec<String>,
    pub snippet: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SyncReport {
    pub added: usize,
    pub removed: usize,
    pub updated: usize,
    pub total_current: usize,
    #[serde(default)]
    pub no_changes: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GitHubListsEnrichmentReport {
    pub lists: usize,
    pub list_items: usize,
    pub matched_repos: usize,
    pub unmatched_items: usize,
    pub updated_repos: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackgroundJobAccepted {
    pub job_id: String,
    pub kind: String,
    pub accepted: bool,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StarSyncEvent {
    TaskStarted {
        job_id: String,
        kind: String,
    },
    TaskCompleted {
        job_id: String,
        kind: String,
        summary: String,
    },
    TaskFailed {
        job_id: String,
        kind: String,
        message: String,
    },
    SyncStarted {
        run_id: String,
    },
    RemoteAdded {
        repo: String,
    },
    RemoteRemoved {
        repo: String,
    },
    RemoteUpdated {
        repo: String,
    },
    MetaChanged {
        repo: String,
    },
    ReadmeEnriched {
        repo: String,
    },
    SyncCompleted {
        run_id: String,
        report: SyncReport,
    },
    SyncNoChanges {
        total_current: usize,
    },
    ListsEnriched {
        report: GitHubListsEnrichmentReport,
    },
    IndexRebuildStarted,
    IndexRebuildCompleted {
        repos: usize,
    },
    StorageChanged {
        action: String,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub id: String,
    pub name: String,
    pub emitted_at: DateTime<Utc>,
    pub event: StarSyncEvent,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EventSubscriptionCreate {
    pub url: String,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub secret: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EventSubscriptionPatch {
    pub url: Option<String>,
    pub events: Option<Vec<String>>,
    pub enabled: Option<bool>,
    pub secret: Option<Option<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventSubscriptionView {
    pub id: String,
    pub url: String,
    pub events: Vec<String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub failure_count: u64,
    pub last_delivery: Option<WebhookDeliveryState>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebhookDeliveryState {
    pub delivered_at: DateTime<Utc>,
    pub success: bool,
    pub status: Option<u16>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MetaPatch {
    pub tags: Option<Vec<String>>,
    pub lists: Option<Vec<String>>,
    pub status: Option<Option<String>>,
    pub summary: Option<Option<String>>,
    pub notes: Option<Option<String>>,
    pub links: Option<Vec<UserLink>>,
    pub archived: Option<bool>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub version: String,
}
