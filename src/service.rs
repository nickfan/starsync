use crate::{
    config::Config,
    events::EventBus,
    github::GitHubClient,
    markdown::{MarkdownStore, RepoMetaDocument},
    models::{
        ListResponse, MetaPatch, MirrorState, ReadmeCacheEntry, RemoteRepo, RepoFilters,
        RepoIdentity, RepoMeta, RepoView, SearchResult, SortDirection, StarSyncEvent, SyncReport,
    },
    search,
    sqlite_index::SqliteIndex,
};
use anyhow::{anyhow, Result};
use chrono::Utc;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Clone)]
pub struct StarSyncService {
    config: Config,
    store: MarkdownStore,
    events: EventBus,
    sqlite: Option<SqliteIndex>,
}

impl StarSyncService {
    pub fn new(config: Config) -> Self {
        let store = MarkdownStore::new(config.repos_dir(), config.state_dir.clone());
        let sqlite = config
            .sqlite_enabled
            .then(|| SqliteIndex::new(config.sqlite_file()));
        Self {
            config,
            store,
            events: EventBus::new(),
            sqlite,
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn events(&self) -> EventBus {
        self.events.clone()
    }

    pub fn init(&self) -> Result<()> {
        self.store.init()?;
        if !self.config.mirror_file().exists() {
            self.store.write_mirror(&MirrorState::default())?;
        }
        Ok(())
    }

    pub fn list_repos(&self, filters: RepoFilters) -> Result<ListResponse<RepoView>> {
        let repos = self.merged_repos()?;
        Ok(search::list_repos(repos, &filters))
    }

    pub fn search_repos(&self, filters: RepoFilters) -> Result<ListResponse<SearchResult>> {
        let requires_merged_search =
            filters.sort.is_some() || search::query_uses_structured_syntax(filters.q.as_deref());
        if !requires_merged_search {
            if let Some(sqlite) = &self.sqlite {
                if let Ok(Some(result)) = sqlite.search(&filters) {
                    return Ok(result);
                }
            }
        }
        let repos = self.merged_repos()?;
        Ok(search::search_repos(repos, &filters))
    }

    pub fn get_repo(&self, identity: &RepoIdentity) -> Result<Option<RepoView>> {
        Ok(self
            .merged_repos()?
            .into_iter()
            .find(|repo| repo.owner == identity.owner && repo.name == identity.name))
    }

    pub fn get_meta(&self, identity: &RepoIdentity) -> Result<RepoMetaDocument> {
        self.store.ensure_meta(identity)
    }

    pub fn patch_meta(
        &self,
        identity: &RepoIdentity,
        patch: MetaPatch,
    ) -> Result<RepoMetaDocument> {
        let mut document = self.store.ensure_meta(identity)?;
        apply_meta_patch(&mut document.meta, patch);
        self.store.write_meta(&document)?;
        self.events.emit(StarSyncEvent::MetaChanged {
            repo: identity.full_name(),
        });
        self.rebuild_index().ok();
        Ok(document)
    }

    pub fn delete_meta(&self, identity: &RepoIdentity) -> Result<RepoMetaDocument> {
        let document = self.store.mark_meta_archived(identity)?;
        self.events.emit(StarSyncEvent::MetaChanged {
            repo: identity.full_name(),
        });
        self.rebuild_index().ok();
        Ok(document)
    }

    pub async fn sync(&self) -> Result<SyncReport> {
        let token =
            self.config.github_token.clone().ok_or_else(|| {
                anyhow!("STARSYNC_GITHUB_TOKEN or github.token is required for sync")
            })?;
        let run_id = Uuid::new_v4().to_string();
        self.events.emit(StarSyncEvent::SyncStarted {
            run_id: run_id.clone(),
        });
        let client = GitHubClient::new(token)?;
        let fetched = client
            .fetch_starred(crate::models::RepoSort::Updated, SortDirection::Desc)
            .await?;
        let report = self.apply_remote_repos(fetched)?;
        self.events.emit(StarSyncEvent::SyncCompleted {
            run_id,
            report: report.clone(),
        });
        Ok(report)
    }

    pub fn apply_remote_repos(&self, fetched: Vec<RemoteRepo>) -> Result<SyncReport> {
        self.init()?;
        let mut state = self.store.read_mirror()?;
        let old: BTreeMap<String, RemoteRepo> = state
            .repos
            .into_iter()
            .map(|repo| (repo.full_name.clone(), repo))
            .collect();
        let fetched_names: BTreeSet<String> =
            fetched.iter().map(|repo| repo.full_name.clone()).collect();
        let mut merged = BTreeMap::new();
        let mut report = SyncReport::default();

        for mut repo in fetched {
            repo.current = true;
            match old.get(&repo.full_name) {
                None => {
                    report.added += 1;
                    self.events.emit(StarSyncEvent::RemoteAdded {
                        repo: repo.full_name.clone(),
                    });
                }
                Some(previous) if !previous.current => {
                    report.added += 1;
                    self.events.emit(StarSyncEvent::RemoteAdded {
                        repo: repo.full_name.clone(),
                    });
                }
                Some(previous) if previous != &repo => {
                    report.updated += 1;
                    self.events.emit(StarSyncEvent::RemoteUpdated {
                        repo: repo.full_name.clone(),
                    });
                }
                Some(_) => {}
            }
            self.ensure_remote_meta(&repo)?;
            merged.insert(repo.full_name.clone(), repo);
        }

        for (name, mut previous) in old {
            if fetched_names.contains(&name) {
                continue;
            }
            if previous.current {
                report.removed += 1;
                self.events
                    .emit(StarSyncEvent::RemoteRemoved { repo: name.clone() });
            }
            previous.current = false;
            merged.insert(name, previous);
        }

        report.total_current = merged.values().filter(|repo| repo.current).count();
        state.repos = merged.into_values().collect();
        state.last_sync_at = Some(Utc::now());
        self.store.write_mirror(&state)?;
        self.rebuild_index()?;
        Ok(report)
    }

    pub async fn enrich_readmes(&self, limit: Option<usize>) -> Result<usize> {
        let token = self.config.github_token.clone().ok_or_else(|| {
            anyhow!("STARSYNC_GITHUB_TOKEN or github.token is required for README enrichment")
        })?;
        let client = GitHubClient::new(token)?;
        let mut state = self.store.read_mirror()?;
        let mut updated = 0;
        let current: Vec<RemoteRepo> = state
            .repos
            .iter()
            .filter(|repo| repo.current)
            .take(limit.unwrap_or(usize::MAX))
            .cloned()
            .collect();
        for repo in current {
            if let Some(text) = client.fetch_readme(&repo).await? {
                upsert_readme(&mut state, &repo, text);
                updated += 1;
                self.events.emit(StarSyncEvent::ReadmeEnriched {
                    repo: repo.full_name.clone(),
                });
            }
        }
        self.store.write_mirror(&state)?;
        self.rebuild_index()?;
        Ok(updated)
    }

    pub fn rebuild_index(&self) -> Result<()> {
        let repos = self.merged_repos()?;
        self.store.write_catalog(&repos)?;
        if let Some(sqlite) = &self.sqlite {
            sqlite.rebuild(&repos)?;
        }
        Ok(())
    }

    pub fn merged_repos(&self) -> Result<Vec<RepoView>> {
        self.init()?;
        let state = self.store.read_mirror()?;
        let meta_docs = self.store.list_meta()?;
        let mut metas: BTreeMap<String, RepoMetaDocument> = meta_docs
            .into_iter()
            .map(|document| (document.meta.repo.full_name(), document))
            .collect();
        let readmes: BTreeMap<String, String> = state
            .readmes
            .into_iter()
            .map(|entry| (format!("{}/{}", entry.owner, entry.name), entry.text))
            .collect();

        let mut views = Vec::new();
        for remote in state.repos {
            let key = remote.full_name.clone();
            let meta = metas
                .remove(&key)
                .map(|document| document.meta)
                .unwrap_or_else(|| RepoMeta::new(remote.identity()));
            views.push(view_from_remote(remote, meta, readmes.get(&key)));
        }

        for document in metas.into_values() {
            views.push(view_from_meta_only(document.meta, document.body));
        }

        views.sort_by(|a, b| a.full_name.cmp(&b.full_name));
        Ok(views)
    }

    fn ensure_remote_meta(&self, repo: &RemoteRepo) -> Result<()> {
        let identity = repo.identity();
        let mut document = self.store.ensure_meta(&identity)?;
        let mut changed = false;
        if document.meta.source.github_id != Some(repo.github_id) {
            document.meta.source.github_id = Some(repo.github_id);
            changed = true;
        }
        if document.meta.source.html_url.as_deref() != Some(repo.html_url.as_str()) {
            document.meta.source.html_url = Some(repo.html_url.clone());
            changed = true;
        }
        if changed {
            self.store.write_meta(&document)?;
        }
        Ok(())
    }
}

fn apply_meta_patch(meta: &mut RepoMeta, patch: MetaPatch) {
    if let Some(tags) = patch.tags {
        meta.user.tags = tags;
    }
    if let Some(status) = patch.status {
        meta.user.status = status;
    }
    if let Some(summary) = patch.summary {
        meta.user.summary = summary;
    }
    if let Some(notes) = patch.notes {
        meta.user.notes = notes;
    }
    if let Some(links) = patch.links {
        meta.user.links = links;
    }
    if let Some(archived) = patch.archived {
        meta.archived = archived;
    }
}

fn view_from_remote(remote: RemoteRepo, meta: RepoMeta, readme: Option<&String>) -> RepoView {
    RepoView {
        owner: remote.owner,
        name: remote.name,
        full_name: remote.full_name,
        html_url: Some(remote.html_url),
        description: remote.description,
        language: remote.language,
        topics: remote.topics,
        stargazers_count: Some(remote.stargazers_count),
        default_branch: remote.default_branch,
        pushed_at: remote.pushed_at,
        updated_at: remote.updated_at,
        starred_at: remote.starred_at,
        current: remote.current,
        archived: meta.archived || !remote.current,
        user: meta.user,
        readme_snippet: readme.map(|text| truncate(text, 8_000)),
    }
}

fn view_from_meta_only(meta: RepoMeta, body: String) -> RepoView {
    RepoView {
        owner: meta.repo.owner.clone(),
        name: meta.repo.name.clone(),
        full_name: meta.repo.full_name(),
        html_url: meta.source.html_url,
        current: false,
        archived: true,
        user: meta.user,
        readme_snippet: (!body.trim().is_empty()).then_some(truncate(&body, 8_000)),
        ..RepoView::default()
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn upsert_readme(state: &mut MirrorState, repo: &RemoteRepo, text: String) {
    if let Some(entry) = state
        .readmes
        .iter_mut()
        .find(|entry| entry.owner == repo.owner && entry.name == repo.name)
    {
        entry.text = text;
        entry.fetched_at = Utc::now();
    } else {
        state.readmes.push(ReadmeCacheEntry {
            owner: repo.owner.clone(),
            name: repo.name.clone(),
            fetched_at: Utc::now(),
            text,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn test_service() -> (tempfile::TempDir, StarSyncService) {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            data_dir: dir.path().join("data"),
            state_dir: dir.path().join("state"),
            sqlite_enabled: true,
            ..Config::defaults()
        };
        let service = StarSyncService::new(config);
        (dir, service)
    }

    fn remote(name: &str) -> RemoteRepo {
        RemoteRepo {
            github_id: 1,
            owner: "alice".to_string(),
            name: name.to_string(),
            full_name: format!("alice/{name}"),
            html_url: format!("https://github.com/alice/{name}"),
            current: true,
            ..RemoteRepo::default()
        }
    }

    #[test]
    fn remote_unstar_preserves_local_meta_as_archived_searchable_data() {
        let (_dir, service) = test_service();
        service.apply_remote_repos(vec![remote("demo")]).unwrap();
        service
            .patch_meta(
                &RepoIdentity::new("alice", "demo"),
                MetaPatch {
                    tags: Some(vec!["keepers".to_string()]),
                    summary: Some(Some("Important local note".to_string())),
                    ..MetaPatch::default()
                },
            )
            .unwrap();

        service.apply_remote_repos(Vec::new()).unwrap();
        let archived = service
            .search_repos(RepoFilters {
                q: Some("keepers".to_string()),
                archived: Some(true),
                ..RepoFilters::default()
            })
            .unwrap();

        assert_eq!(archived.items.len(), 1);
        assert!(!archived.items[0].repo.current);
        assert_eq!(archived.items[0].repo.user.tags, vec!["keepers"]);
    }

    #[test]
    fn list_returns_fused_remote_and_meta_fields() {
        let (_dir, service) = test_service();
        service.apply_remote_repos(vec![remote("demo")]).unwrap();
        service
            .patch_meta(
                &RepoIdentity::new("alice", "demo"),
                MetaPatch {
                    tags: Some(vec!["rust".to_string()]),
                    ..MetaPatch::default()
                },
            )
            .unwrap();

        let response = service
            .list_repos(RepoFilters {
                tag: Some("rust".to_string()),
                ..RepoFilters::default()
            })
            .unwrap();

        assert_eq!(response.items.len(), 1);
        assert_eq!(
            response.items[0].html_url.as_deref(),
            Some("https://github.com/alice/demo")
        );
    }

    #[test]
    fn structured_search_bypasses_sqlite_fts_for_field_filters() {
        let (_dir, service) = test_service();
        let mut tooling = remote("Toolbox");
        tooling.language = Some("Rust".to_string());
        tooling.topics = vec!["cli".to_string()];
        tooling.stargazers_count = 1200;
        let mut web = remote("webapp");
        web.language = Some("Rust".to_string());
        web.topics = vec!["web".to_string()];
        web.stargazers_count = 2200;
        service.apply_remote_repos(vec![tooling, web]).unwrap();

        let response = service
            .search_repos(RepoFilters {
                q: Some("language:Rust AND name:^T stars:>=1000".to_string()),
                ..RepoFilters::default()
            })
            .unwrap();

        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].repo.full_name, "alice/Toolbox");
    }

    #[test]
    fn sorted_search_uses_merged_results_instead_of_sqlite_rank() {
        let (_dir, service) = test_service();
        let mut low = remote("low");
        low.description = Some("agent toolkit".to_string());
        low.stargazers_count = 10;
        let mut high = remote("high");
        high.description = Some("agent toolkit".to_string());
        high.stargazers_count = 100;
        service.apply_remote_repos(vec![low, high]).unwrap();

        let response = service
            .search_repos(RepoFilters {
                q: Some("agent".to_string()),
                sort: Some(crate::models::RepoSort::Stars),
                direction: Some(SortDirection::Desc),
                ..RepoFilters::default()
            })
            .unwrap();

        assert_eq!(response.items.len(), 2);
        assert_eq!(response.items[0].repo.full_name, "alice/high");
        assert_eq!(response.items[1].repo.full_name, "alice/low");
    }

    #[test]
    fn sync_rebuilds_markdown_catalog_files() {
        let (dir, service) = test_service();
        service.apply_remote_repos(vec![remote("demo")]).unwrap();
        service
            .patch_meta(
                &RepoIdentity::new("alice", "demo"),
                MetaPatch {
                    tags: Some(vec!["keepers".to_string()]),
                    ..MetaPatch::default()
                },
            )
            .unwrap();

        let repos_dir = dir.path().join("data/repos");
        let index = std::fs::read_to_string(repos_dir.join("INDEX.md")).unwrap();
        assert!(index.contains("kind: repo_index"));
        assert!(index.contains("[alice/demo](alice/demo/INDEX.md)"));

        let catalog: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(repos_dir.join("catalog.json")).unwrap())
                .unwrap();
        assert_eq!(catalog["counts"]["current"], 1);
        assert_eq!(catalog["items"][0]["tags"][0], "keepers");

        service.apply_remote_repos(Vec::new()).unwrap();

        let catalog: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(repos_dir.join("catalog.json")).unwrap())
                .unwrap();
        assert_eq!(catalog["counts"]["current"], 0);
        assert_eq!(catalog["counts"]["archived"], 1);
        assert_eq!(catalog["items"][0]["current"], false);
        assert_eq!(catalog["items"][0]["archived"], true);

        let by_owner = std::fs::read_to_string(repos_dir.join("INDEX.by-owner.md")).unwrap();
        assert!(by_owner.contains("## A"));
        assert!(by_owner.contains("_(archived)_"));
    }
}
