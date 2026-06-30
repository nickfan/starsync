use crate::{
    models::{ListResponse, RepoFilters, RepoView, SearchResult},
    search,
};
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct SqliteIndex {
    path: PathBuf,
}

impl SqliteIndex {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn rebuild(&self, repos: &[RepoView]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut conn = Connection::open(&self.path)
            .with_context(|| format!("failed to open {}", self.path.display()))?;
        create_schema(&conn)?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM repo_fts", [])?;
        for repo in repos {
            tx.execute(
                "INSERT INTO repo_fts(full_name, owner, name, description, language, topics, tags, summary, notes, readme, repo_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    repo.full_name,
                    repo.owner,
                    repo.name,
                    repo.description.clone().unwrap_or_default(),
                    repo.language.clone().unwrap_or_default(),
                    repo.topics.join(" "),
                    repo.user.tags.join(" "),
                    repo.user.summary.clone().unwrap_or_default(),
                    repo.user.notes.clone().unwrap_or_default(),
                    repo.readme_snippet.clone().unwrap_or_default(),
                    serde_json::to_string(repo)?,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn search(&self, filters: &RepoFilters) -> Result<Option<ListResponse<SearchResult>>> {
        let Some(query) = filters
            .q
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(None);
        };
        if !self.path.exists() {
            return Ok(None);
        }
        let conn = Connection::open(&self.path)
            .with_context(|| format!("failed to open {}", self.path.display()))?;
        create_schema(&conn)?;
        let fts_query = fts_query(query);
        let mut stmt = conn.prepare(
            "SELECT repo_json, bm25(repo_fts) AS rank
             FROM repo_fts
             WHERE repo_fts MATCH ?1
             ORDER BY rank
             LIMIT 500",
        )?;
        let rows = stmt.query_map([fts_query], |row| {
            let raw: String = row.get(0)?;
            let rank: f64 = row.get(1)?;
            Ok((raw, rank))
        })?;

        let mut results = Vec::new();
        for row in rows {
            let (raw, rank) = row?;
            let repo: RepoView = serde_json::from_str(&raw)?;
            if !search::matches_filters(&repo, filters) {
                continue;
            }
            results.push(SearchResult {
                matched_fields: matched_fields(&repo, query),
                snippet: repo
                    .user
                    .summary
                    .clone()
                    .or_else(|| repo.description.clone())
                    .or_else(|| repo.readme_snippet.clone()),
                score: (-rank) as f32,
                repo,
            });
        }
        Ok(Some(page_results(results, filters)))
    }
}

fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS repo_fts USING fts5(
            full_name,
            owner,
            name,
            description,
            language,
            topics,
            tags,
            summary,
            notes,
            readme,
            repo_json UNINDEXED
        );",
    )?;
    Ok(())
}

fn fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn matched_fields(repo: &RepoView, query: &str) -> Vec<String> {
    let query = query.to_ascii_lowercase();
    let mut fields = Vec::new();
    let mut add = |field: &str, value: &str| {
        if value.to_ascii_lowercase().contains(&query) {
            fields.push(field.to_string());
        }
    };
    add("name", &repo.full_name);
    if let Some(value) = &repo.description {
        add("description", value);
    }
    if let Some(value) = &repo.user.summary {
        add("summary", value);
    }
    if let Some(value) = &repo.user.notes {
        add("notes", value);
    }
    for tag in &repo.user.tags {
        add("tag", tag);
    }
    fields
}

fn page_results<T>(items: Vec<T>, filters: &RepoFilters) -> ListResponse<T> {
    let total = items.len();
    let start = filters
        .cursor
        .as_deref()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = filters
        .limit
        .or(filters.per_page)
        .unwrap_or(50)
        .clamp(1, 200);
    let next = start + limit;
    ListResponse {
        items: items.into_iter().skip(start).take(limit).collect(),
        total,
        next_cursor: (next < total).then(|| next.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserMeta;

    #[test]
    fn rebuilds_and_searches_fts_index() {
        let dir = tempfile::tempdir().unwrap();
        let index = SqliteIndex::new(dir.path().join("starsync.db"));
        let repo = RepoView {
            owner: "alice".to_string(),
            name: "vector-db".to_string(),
            full_name: "alice/vector-db".to_string(),
            current: true,
            user: UserMeta {
                tags: vec!["retrieval".to_string()],
                summary: Some("Local semantic retrieval notes".to_string()),
                ..UserMeta::default()
            },
            ..RepoView::default()
        };

        index.rebuild(&[repo]).unwrap();
        let response = index
            .search(&RepoFilters {
                q: Some("retrieval".to_string()),
                ..RepoFilters::default()
            })
            .unwrap()
            .unwrap();

        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].repo.full_name, "alice/vector-db");
    }
}
