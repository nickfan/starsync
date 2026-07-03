use crate::models::{RemoteRepo, RepoSort, SortDirection};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use futures_util::{stream, StreamExt, TryStreamExt};
use reqwest::{header, Client, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
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

    pub async fn fetch_star_lists(&self) -> Result<Vec<GitHubStarList>> {
        let mut lists = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let page: ViewerListsData = self
                .graphql(VIEWER_LISTS_QUERY, serde_json::json!({ "after": after }))
                .await?;
            let connection = page.viewer.lists;
            for list in connection.nodes {
                let mut list = GitHubStarList::from_node(list);
                while list.has_next_page {
                    let page: ListItemsData = self
                        .graphql(
                            LIST_ITEMS_QUERY,
                            serde_json::json!({
                                "id": list.id,
                                "after": list.next_cursor,
                            }),
                        )
                        .await?;
                    if let Some(node) = page.node {
                        list.extend_items(node.items);
                    } else {
                        break;
                    }
                }
                lists.push(list);
            }
            if !connection.page_info.has_next_page {
                break;
            }
            after = connection.page_info.end_cursor;
        }
        Ok(lists)
    }

    async fn graphql<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<T> {
        let response = self
            .client
            .post("https://api.github.com/graphql")
            .bearer_auth(&self.token)
            .json(&GraphQlRequest { query, variables })
            .send()
            .await
            .context("failed to request GitHub GraphQL API")?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!("GitHub GraphQL request failed: {status} {text}"));
        }
        let payload: GraphQlResponse<T> = response.json().await?;
        if let Some(errors) = payload.errors.filter(|errors| !errors.is_empty()) {
            let message = errors
                .into_iter()
                .map(|error| error.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(anyhow!("GitHub GraphQL errors: {message}"));
        }
        let data = payload
            .data
            .ok_or_else(|| anyhow!("GitHub GraphQL response did not contain data"))?;
        Ok(data)
    }
}

fn github_sort(sort: RepoSort) -> &'static str {
    match sort {
        RepoSort::Created => "created",
        RepoSort::Updated | RepoSort::Name | RepoSort::Stars | RepoSort::Forks => "updated",
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
            forks_count: value.repo.forks_count,
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
    #[serde(default)]
    forks_count: u64,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubStarList {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub is_private: bool,
    pub repositories: Vec<GitHubListedRepo>,
    has_next_page: bool,
    next_cursor: Option<String>,
}

impl GitHubStarList {
    fn from_node(node: UserListNode) -> Self {
        let mut list = Self {
            id: node.id,
            name: node.name,
            slug: node.slug,
            is_private: node.is_private,
            repositories: Vec::new(),
            has_next_page: false,
            next_cursor: None,
        };
        list.extend_items(node.items);
        list
    }

    fn extend_items(&mut self, items: UserListItemsConnection) {
        self.repositories.extend(
            items
                .nodes
                .into_iter()
                .filter_map(GitHubListedRepo::from_node),
        );
        self.has_next_page = items.page_info.has_next_page;
        self.next_cursor = items.page_info.end_cursor;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubListedRepo {
    pub github_id: Option<i64>,
    pub full_name: String,
    pub html_url: Option<String>,
    pub viewer_has_starred: bool,
}

impl GitHubListedRepo {
    fn from_node(node: UserListItemNode) -> Option<Self> {
        (node.typename == "Repository").then_some(Self {
            github_id: node.database_id,
            full_name: node.name_with_owner?,
            html_url: node.url,
            viewer_has_starred: node.viewer_has_starred.unwrap_or(false),
        })
    }
}

#[derive(Serialize)]
struct GraphQlRequest<'a> {
    query: &'a str,
    variables: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    message: String,
}

impl std::fmt::Display for GraphQlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[derive(Debug, Deserialize)]
struct ViewerListsData {
    viewer: ViewerLists,
}

#[derive(Debug, Deserialize)]
struct ViewerLists {
    lists: UserListsConnection,
}

#[derive(Debug, Deserialize)]
struct UserListsConnection {
    #[serde(default)]
    nodes: Vec<UserListNode>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Debug, Deserialize)]
struct UserListNode {
    id: String,
    name: String,
    slug: String,
    #[serde(rename = "isPrivate")]
    is_private: bool,
    items: UserListItemsConnection,
}

#[derive(Debug, Deserialize)]
struct ListItemsData {
    node: Option<ListItemsNode>,
}

#[derive(Debug, Deserialize)]
struct ListItemsNode {
    items: UserListItemsConnection,
}

#[derive(Debug, Deserialize)]
struct UserListItemsConnection {
    #[serde(default)]
    nodes: Vec<UserListItemNode>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Debug, Deserialize)]
struct UserListItemNode {
    #[serde(rename = "__typename")]
    typename: String,
    #[serde(rename = "databaseId")]
    database_id: Option<i64>,
    #[serde(rename = "nameWithOwner")]
    name_with_owner: Option<String>,
    url: Option<String>,
    #[serde(rename = "viewerHasStarred")]
    viewer_has_starred: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

const VIEWER_LISTS_QUERY: &str = r#"
query StarSyncViewerLists($after: String) {
  viewer {
    lists(first: 100, after: $after) {
      nodes {
        id
        name
        slug
        isPrivate
        items(first: 100) {
          nodes {
            __typename
            ... on Repository {
              databaseId
              nameWithOwner
              url
              viewerHasStarred
            }
          }
          pageInfo {
            hasNextPage
            endCursor
          }
        }
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
  }
}
"#;

const LIST_ITEMS_QUERY: &str = r#"
query StarSyncListItems($id: ID!, $after: String) {
  node(id: $id) {
    ... on UserList {
      items(first: 100, after: $after) {
        nodes {
          __typename
          ... on Repository {
            databaseId
            nameWithOwner
            url
            viewerHasStarred
          }
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
  }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_last_page_from_github_link_header() {
        let link = r#"<https://api.github.com/user/starred?per_page=100&page=2>; rel="next", <https://api.github.com/user/starred?per_page=100&page=46>; rel="last""#;

        assert_eq!(parse_last_page(link), Some(46));
    }

    #[test]
    fn builds_star_list_from_repository_union_nodes() {
        let list = GitHubStarList::from_node(UserListNode {
            id: "UL_1".to_string(),
            name: "Toolkit".to_string(),
            slug: "toolkit".to_string(),
            is_private: false,
            items: UserListItemsConnection {
                nodes: vec![
                    UserListItemNode {
                        typename: "Repository".to_string(),
                        database_id: Some(42),
                        name_with_owner: Some("alice/toolbox".to_string()),
                        url: Some("https://github.com/alice/toolbox".to_string()),
                        viewer_has_starred: Some(true),
                    },
                    UserListItemNode {
                        typename: "Issue".to_string(),
                        database_id: None,
                        name_with_owner: None,
                        url: None,
                        viewer_has_starred: None,
                    },
                ],
                page_info: PageInfo {
                    has_next_page: false,
                    end_cursor: None,
                },
            },
        });

        assert_eq!(list.slug, "toolkit");
        assert_eq!(list.repositories.len(), 1);
        assert_eq!(list.repositories[0].github_id, Some(42));
        assert_eq!(list.repositories[0].full_name, "alice/toolbox");
    }
}
