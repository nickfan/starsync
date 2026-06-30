use crate::models::{RemoteRepo, RepoSort, SortDirection};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use futures_util::{stream, StreamExt, TryStreamExt};
use reqwest::{header, Client, StatusCode};
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone)]
pub struct GitHubClient {
    client: Client,
    token: String,
}

impl GitHubClient {
    pub fn new(token: impl Into<String>) -> Result<Self> {
        let client = Client::builder()
            .user_agent(format!("starsync/{}", env!("CARGO_PKG_VERSION")))
            .http1_only()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
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
        let (first_page, last_page) = self.fetch_starred_page(1, sort, direction).await?;
        let mut repos = first_page;
        let Some(last_page) = last_page else {
            return Ok(repos);
        };
        if last_page <= 1 {
            return Ok(repos);
        }

        let mut remaining = stream::iter(2..=last_page)
            .map(|page| async move {
                let (items, _) = self.fetch_starred_page(page, sort, direction).await?;
                Ok::<_, anyhow::Error>(items)
            })
            .buffer_unordered(4);

        while let Some(items) = remaining.try_next().await? {
            repos.extend(items);
        }
        repos.sort_by(|a, b| {
            b.starred_at
                .cmp(&a.starred_at)
                .then_with(|| a.full_name.cmp(&b.full_name))
        });
        Ok(repos)
    }

    async fn fetch_starred_page(
        &self,
        page: usize,
        sort: RepoSort,
        direction: SortDirection,
    ) -> Result<(Vec<RemoteRepo>, Option<usize>)> {
        for attempt in 1..=3 {
            match self.fetch_starred_page_once(page, sort, direction).await {
                Ok(result) => return Ok(result),
                Err(error) if attempt < 3 => {
                    tracing::warn!(
                        page,
                        attempt,
                        error = %error,
                        "retrying GitHub starred page request"
                    );
                    tokio::time::sleep(Duration::from_millis(750 * attempt)).await;
                }
                Err(error) => {
                    return Err(error.context(format!(
                        "failed to fetch GitHub starred page {page} after {attempt} attempts"
                    )));
                }
            }
        }
        unreachable!("retry loop always returns");
    }

    async fn fetch_starred_page_once(
        &self,
        page: usize,
        sort: RepoSort,
        direction: SortDirection,
    ) -> Result<(Vec<RemoteRepo>, Option<usize>)> {
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
            .with_context(|| {
                format!("failed to request GitHub starred repositories page {page}")
            })?;
        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok((Vec::new(), None));
        }
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "GitHub starred request failed on page {page}: {status} {text}"
            ));
        }
        let last_page = response
            .headers()
            .get(header::LINK)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_last_page);
        let items: Vec<StarredRepoItem> = response.json().await?;
        Ok((items.into_iter().map(RemoteRepo::from).collect(), last_page))
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

fn parse_last_page(link_header: &str) -> Option<usize> {
    link_header.split(',').find_map(|part| {
        let part = part.trim();
        if !part.contains("rel=\"last\"") {
            return None;
        }
        let start = part.rfind("page=")? + "page=".len();
        let digits: String = part[start..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        digits.parse().ok()
    })
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

#[cfg(test)]
mod tests {
    use super::parse_last_page;

    #[test]
    fn parses_last_page_from_github_link_header() {
        let link = r#"<https://api.github.com/user/starred?per_page=100&page=2>; rel="next", <https://api.github.com/user/starred?per_page=100&page=46>; rel="last""#;

        assert_eq!(parse_last_page(link), Some(46));
    }
}
