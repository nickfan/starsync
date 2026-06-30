use crate::models::{RemoteRepo, RepoSort, SortDirection};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::{header, Client, StatusCode};
use serde::Deserialize;

#[derive(Clone)]
pub struct GitHubClient {
    client: Client,
    token: String,
}

impl GitHubClient {
    pub fn new(token: impl Into<String>) -> Result<Self> {
        let client = Client::builder()
            .user_agent(format!("starsync/{}", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            client,
            token: token.into(),
        })
    }

    pub async fn fetch_starred(
        &self,
        sort: RepoSort,
        direction: SortDirection,
    ) -> Result<Vec<RemoteRepo>> {
        let mut page = 1;
        let mut repos = Vec::new();
        loop {
            let url = format!(
                "https://api.github.com/user/starred?per_page=100&page={page}&sort={}&direction={}",
                github_sort(sort),
                github_direction(direction)
            );
            let response = self
                .client
                .get(url)
                .bearer_auth(&self.token)
                .header(header::ACCEPT, "application/vnd.github.star+json")
                .send()
                .await
                .context("failed to request GitHub starred repositories")?;
            if response.status() == StatusCode::NOT_MODIFIED {
                break;
            }
            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(anyhow!("GitHub starred request failed: {status} {text}"));
            }
            let items: Vec<StarredRepoItem> = response.json().await?;
            let count = items.len();
            repos.extend(items.into_iter().map(RemoteRepo::from));
            if count < 100 {
                break;
            }
            page += 1;
        }
        Ok(repos)
    }

    pub async fn fetch_readme(&self, repo: &RemoteRepo) -> Result<Option<String>> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/readme",
            repo.owner, repo.name
        );
        let response = self
            .client
            .get(url)
            .bearer_auth(&self.token)
            .header(header::ACCEPT, "application/vnd.github.raw+json")
            .send()
            .await
            .with_context(|| format!("failed to request README for {}", repo.full_name))?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!("GitHub README request failed: {status} {text}"));
        }
        let text = response.text().await?;
        Ok(Some(text))
    }
}

fn github_sort(sort: RepoSort) -> &'static str {
    match sort {
        RepoSort::Created => "created",
        RepoSort::Updated | RepoSort::Name | RepoSort::Stars => "updated",
    }
}

fn github_direction(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Asc => "asc",
        SortDirection::Desc => "desc",
    }
}

#[derive(Debug, Deserialize)]
struct StarredRepoItem {
    starred_at: Option<DateTime<Utc>>,
    repo: GitHubRepo,
}

impl From<StarredRepoItem> for RemoteRepo {
    fn from(value: StarredRepoItem) -> Self {
        let owner = value.repo.owner.login;
        let name = value.repo.name;
        Self {
            github_id: value.repo.id,
            full_name: format!("{owner}/{name}"),
            owner,
            name,
            html_url: value.repo.html_url,
            description: value.repo.description,
            language: value.repo.language,
            topics: value.repo.topics,
            stargazers_count: value.repo.stargazers_count,
            default_branch: value.repo.default_branch,
            pushed_at: value.repo.pushed_at,
            updated_at: value.repo.updated_at,
            starred_at: value.starred_at,
            current: true,
            archived: value.repo.archived,
        }
    }
}

#[derive(Debug, Deserialize)]
struct GitHubRepo {
    id: i64,
    name: String,
    owner: GitHubOwner,
    html_url: String,
    description: Option<String>,
    language: Option<String>,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    stargazers_count: u64,
    default_branch: Option<String>,
    pushed_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    archived: bool,
}

#[derive(Debug, Deserialize)]
struct GitHubOwner {
    login: String,
}
