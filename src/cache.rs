use crate::models::RepoView;
use moka::sync::Cache;
use std::{sync::Arc, time::Duration};

const MERGED_REPOS_KEY: &str = "merged_repos";

#[derive(Clone, Debug)]
pub struct RepoCache {
    merged_repos: Cache<&'static str, Arc<Vec<RepoView>>>,
}

impl RepoCache {
    pub fn new() -> Self {
        Self {
            merged_repos: Cache::builder()
                .max_capacity(8)
                .time_to_live(Duration::from_secs(30))
                .build(),
        }
    }

    pub fn merged_repos(&self) -> Option<Vec<RepoView>> {
        self.merged_repos
            .get(MERGED_REPOS_KEY)
            .map(|repos| repos.as_ref().clone())
    }

    pub fn store_merged_repos(&self, repos: Vec<RepoView>) -> Vec<RepoView> {
        self.merged_repos
            .insert(MERGED_REPOS_KEY, Arc::new(repos.clone()));
        repos
    }

    pub fn invalidate_all(&self) {
        self.merged_repos.invalidate_all();
    }
}

impl Default for RepoCache {
    fn default() -> Self {
        Self::new()
    }
}
