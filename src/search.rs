use crate::models::{ListResponse, RepoFilters, RepoSort, RepoView, SearchResult, SortDirection};
use std::cmp::Ordering;

pub fn list_repos(mut repos: Vec<RepoView>, filters: &RepoFilters) -> ListResponse<RepoView> {
    repos.retain(|repo| matches_filters(repo, filters));
    sort_repos(&mut repos, filters);
    page_items(repos, filters)
}

pub fn search_repos(repos: Vec<RepoView>, filters: &RepoFilters) -> ListResponse<SearchResult> {
    let query = filters.q.clone().unwrap_or_default();
    let mut results: Vec<SearchResult> = repos
        .into_iter()
        .filter(|repo| matches_filters(repo, filters))
        .filter_map(|repo| score_repo(repo, &query))
        .collect();
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.repo.full_name.cmp(&b.repo.full_name))
    });
    page_items(results, filters)
}

pub fn matches_filters(repo: &RepoView, filters: &RepoFilters) -> bool {
    if filters.archived.unwrap_or(false) != repo.archived && filters.archived.is_some() {
        return false;
    }
    if filters.archived.is_none() && (!repo.current || repo.archived) {
        return false;
    }
    if let Some(owner) = &filters.owner {
        if !repo.owner.eq_ignore_ascii_case(owner) {
            return false;
        }
    }
    if let Some(language) = &filters.language {
        if repo
            .language
            .as_deref()
            .map(|value| !value.eq_ignore_ascii_case(language))
            .unwrap_or(true)
        {
            return false;
        }
    }
    if let Some(topic) = &filters.topic {
        if !repo
            .topics
            .iter()
            .any(|value| value.eq_ignore_ascii_case(topic))
        {
            return false;
        }
    }
    if let Some(tag) = &filters.tag {
        if !repo
            .user
            .tags
            .iter()
            .any(|value| value.eq_ignore_ascii_case(tag))
        {
            return false;
        }
    }
    if let Some(status) = &filters.status {
        if repo
            .user
            .status
            .as_deref()
            .map(|value| !value.eq_ignore_ascii_case(status))
            .unwrap_or(true)
        {
            return false;
        }
    }
    if let Some(q) = filters
        .q
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return score_repo(repo.clone(), q).is_some();
    }
    true
}

fn sort_repos(repos: &mut [RepoView], filters: &RepoFilters) {
    let direction = filters.direction.unwrap_or_default();
    let sort = filters.sort.unwrap_or_default();
    repos.sort_by(|a, b| {
        let ordering = match sort {
            RepoSort::Created => a.starred_at.cmp(&b.starred_at),
            RepoSort::Updated => a.updated_at.cmp(&b.updated_at),
            RepoSort::Name => a.full_name.cmp(&b.full_name),
            RepoSort::Stars => a.stargazers_count.cmp(&b.stargazers_count),
        };
        match direction {
            SortDirection::Asc => ordering,
            SortDirection::Desc => ordering.reverse(),
        }
    });
}

fn score_repo(repo: RepoView, query: &str) -> Option<SearchResult> {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return Some(SearchResult {
            repo,
            score: 0.0,
            matched_fields: Vec::new(),
            snippet: None,
        });
    }

    let mut score = 0.0;
    let mut fields = Vec::new();
    let mut snippet = None;
    add_match(
        &mut score,
        &mut fields,
        "name",
        &repo.full_name,
        &query,
        4.0,
    );
    if let Some(description) = &repo.description {
        if add_match(
            &mut score,
            &mut fields,
            "description",
            description,
            &query,
            2.0,
        ) {
            snippet.get_or_insert_with(|| make_snippet(description, &query));
        }
    }
    if let Some(language) = &repo.language {
        add_match(&mut score, &mut fields, "language", language, &query, 1.0);
    }
    for topic in &repo.topics {
        add_match(&mut score, &mut fields, "topic", topic, &query, 1.5);
    }
    for tag in &repo.user.tags {
        add_match(&mut score, &mut fields, "tag", tag, &query, 3.0);
    }
    if let Some(status) = &repo.user.status {
        add_match(&mut score, &mut fields, "status", status, &query, 1.0);
    }
    if let Some(summary) = &repo.user.summary {
        if add_match(&mut score, &mut fields, "summary", summary, &query, 3.0) {
            snippet.get_or_insert_with(|| make_snippet(summary, &query));
        }
    }
    if let Some(notes) = &repo.user.notes {
        if add_match(&mut score, &mut fields, "notes", notes, &query, 2.5) {
            snippet.get_or_insert_with(|| make_snippet(notes, &query));
        }
    }
    if let Some(readme) = &repo.readme_snippet {
        if add_match(&mut score, &mut fields, "readme", readme, &query, 1.2) {
            snippet.get_or_insert_with(|| make_snippet(readme, &query));
        }
    }

    (score > 0.0).then_some(SearchResult {
        repo,
        score,
        matched_fields: fields,
        snippet,
    })
}

fn add_match(
    score: &mut f32,
    fields: &mut Vec<String>,
    field: &str,
    value: &str,
    query: &str,
    weight: f32,
) -> bool {
    if value.to_ascii_lowercase().contains(query) {
        *score += weight;
        if !fields.iter().any(|existing| existing == field) {
            fields.push(field.to_string());
        }
        true
    } else {
        false
    }
}

fn make_snippet(value: &str, query: &str) -> String {
    let lower = value.to_ascii_lowercase();
    let Some(index) = lower.find(query) else {
        return value.chars().take(180).collect();
    };
    let start = index.saturating_sub(60);
    let end = (index + query.len() + 120).min(value.len());
    value[start..end].replace('\n', " ")
}

fn page_items<T>(items: Vec<T>, filters: &RepoFilters) -> ListResponse<T> {
    let total = items.len();
    let start = filters
        .cursor
        .as_deref()
        .and_then(|value| value.parse::<usize>().ok())
        .or_else(|| {
            let page = filters.page.unwrap_or(1).max(1);
            let per_page = filters.per_page.or(filters.limit).unwrap_or(50);
            Some((page - 1) * per_page)
        })
        .unwrap_or(0);
    let limit = filters
        .limit
        .or(filters.per_page)
        .unwrap_or(50)
        .clamp(1, 200);
    let next = start + limit;
    let page = items.into_iter().skip(start).take(limit).collect();
    ListResponse {
        items: page,
        total,
        next_cursor: (next < total).then(|| next.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserMeta;

    fn repo(name: &str, tags: &[&str]) -> RepoView {
        RepoView {
            owner: "alice".to_string(),
            name: name.to_string(),
            full_name: format!("alice/{name}"),
            current: true,
            user: UserMeta {
                tags: tags.iter().map(|value| value.to_string()).collect(),
                summary: Some("local markdown knowledge".to_string()),
                ..UserMeta::default()
            },
            ..RepoView::default()
        }
    }

    #[test]
    fn searches_local_meta_fields() {
        let filters = RepoFilters {
            q: Some("knowledge".to_string()),
            ..RepoFilters::default()
        };

        let results = search_repos(vec![repo("demo", &[])], &filters);

        assert_eq!(results.items.len(), 1);
        assert!(results.items[0]
            .matched_fields
            .iter()
            .any(|field| field == "summary"));
    }

    #[test]
    fn filters_by_user_tag_and_pages_results() {
        let filters = RepoFilters {
            tag: Some("ai".to_string()),
            limit: Some(1),
            ..RepoFilters::default()
        };

        let results = list_repos(vec![repo("one", &["ai"]), repo("two", &["ai"])], &filters);

        assert_eq!(results.items.len(), 1);
        assert_eq!(results.total, 2);
        assert_eq!(results.next_cursor, Some("1".to_string()));
    }
}
