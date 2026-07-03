use crate::models::{ListResponse, RepoFilters, RepoSort, RepoView, SearchResult, SortDirection};
use std::{cmp::Ordering, collections::BTreeSet};

pub fn list_repos(mut repos: Vec<RepoView>, filters: &RepoFilters) -> ListResponse<RepoView> {
    let query = ParsedQuery::new(filters.q.as_deref());
    repos.retain(|repo| matches_filter_fields(repo, filters) && query.matches(repo));
    sort_repos(&mut repos, filters);
    page_items(repos, filters)
}

pub fn search_repos(repos: Vec<RepoView>, filters: &RepoFilters) -> ListResponse<SearchResult> {
    let query = ParsedQuery::new(filters.q.as_deref());
    let mut results: Vec<SearchResult> = repos
        .into_iter()
        .filter(|repo| matches_filter_fields(repo, filters))
        .filter_map(|repo| score_repo(repo, &query))
        .collect();
    sort_search_results(&mut results, filters);
    page_items(results, filters)
}

pub fn result_for_repo(repo: RepoView, filters: &RepoFilters) -> Option<SearchResult> {
    if !matches_filter_fields(&repo, filters) {
        return None;
    }
    let query = ParsedQuery::new(filters.q.as_deref());
    score_repo(repo, &query)
}

pub fn result_for_index_hit(
    repo: RepoView,
    filters: &RepoFilters,
    require_query_match: bool,
) -> Option<SearchResult> {
    if !matches_filter_fields(&repo, filters) {
        return None;
    }
    let query = ParsedQuery::new(filters.q.as_deref());
    if require_query_match {
        return score_repo(repo, &query);
    }
    let index_result = SearchResult {
        repo: repo.clone(),
        score: 0.0,
        matched_fields: Vec::new(),
        snippet: repo
            .user
            .summary
            .clone()
            .or_else(|| repo.description.clone())
            .or_else(|| repo.readme_snippet.clone()),
    };
    Some(score_repo(repo, &query).unwrap_or(index_result))
}

pub fn order_and_page_search_results(
    mut results: Vec<SearchResult>,
    filters: &RepoFilters,
) -> ListResponse<SearchResult> {
    sort_search_results(&mut results, filters);
    page_items(results, filters)
}

pub fn matches_filters(repo: &RepoView, filters: &RepoFilters) -> bool {
    let query = ParsedQuery::new(filters.q.as_deref());
    matches_filter_fields(repo, filters) && query.matches(repo)
}

pub fn query_uses_structured_syntax(query: Option<&str>) -> bool {
    tokenize_query(query.unwrap_or_default())
        .into_iter()
        .any(|token| match token {
            QueryToken::Word(word) => {
                word.strip_prefix('-')
                    .is_some_and(|rest| split_qualifier(rest).is_some())
                    || split_qualifier(&word).is_some()
            }
            QueryToken::LParen
            | QueryToken::RParen
            | QueryToken::And
            | QueryToken::Or
            | QueryToken::Not => true,
        })
}

pub fn query_uses_cjk(query: Option<&str>) -> bool {
    query.unwrap_or_default().chars().any(is_cjk)
}

fn matches_filter_fields(repo: &RepoView, filters: &RepoFilters) -> bool {
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
    true
}

fn sort_repos(repos: &mut [RepoView], filters: &RepoFilters) {
    let direction = filters.direction.unwrap_or_default();
    let sort = filters.sort.unwrap_or_default();
    repos.sort_by(|a, b| {
        let ordering = repo_ordering(a, b, sort);
        match direction {
            SortDirection::Asc => ordering,
            SortDirection::Desc => ordering.reverse(),
        }
    });
}

fn sort_search_results(results: &mut [SearchResult], filters: &RepoFilters) {
    let Some(sort) = filters.sort else {
        results.sort_by(search_score_ordering);
        return;
    };
    let direction = filters.direction.unwrap_or_default();
    results.sort_by(|a, b| {
        let ordering = repo_ordering(&a.repo, &b.repo, sort);
        let ordering = match direction {
            SortDirection::Asc => ordering,
            SortDirection::Desc => ordering.reverse(),
        };
        ordering
            .then_with(|| search_score_ordering(a, b))
            .then_with(|| a.repo.full_name.cmp(&b.repo.full_name))
    });
}

fn repo_ordering(a: &RepoView, b: &RepoView, sort: RepoSort) -> Ordering {
    match sort {
        RepoSort::Created => a.starred_at.cmp(&b.starred_at),
        RepoSort::Updated => a.updated_at.cmp(&b.updated_at),
        RepoSort::Name => a.full_name.cmp(&b.full_name),
        RepoSort::Stars => a.stargazers_count.cmp(&b.stargazers_count),
        RepoSort::Forks => a.forks_count.cmp(&b.forks_count),
    }
}

fn search_score_ordering(a: &SearchResult, b: &SearchResult) -> Ordering {
    b.score
        .partial_cmp(&a.score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| a.repo.full_name.cmp(&b.repo.full_name))
}

#[derive(Clone, Debug)]
struct ParsedQuery {
    expr: Option<QueryExpr>,
}

impl ParsedQuery {
    fn new(query: Option<&str>) -> Self {
        let query = query.unwrap_or_default().trim();
        Self {
            expr: parse_query(query),
        }
    }

    fn matches(&self, repo: &RepoView) -> bool {
        self.expr
            .as_ref()
            .map(|expr| expr.matches(repo))
            .unwrap_or(true)
    }

    fn is_empty(&self) -> bool {
        self.expr.is_none()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum QueryExpr {
    Term(String),
    Qualifier { field: String, value: String },
    Not(Box<QueryExpr>),
    And(Box<QueryExpr>, Box<QueryExpr>),
    Or(Box<QueryExpr>, Box<QueryExpr>),
}

impl QueryExpr {
    fn matches(&self, repo: &RepoView) -> bool {
        match self {
            QueryExpr::Term(term) => text_term_matches(repo, term),
            QueryExpr::Qualifier { field, value } => qualifier_matches(repo, field, value),
            QueryExpr::Not(expr) => !expr.matches(repo),
            QueryExpr::And(left, right) => left.matches(repo) && right.matches(repo),
            QueryExpr::Or(left, right) => left.matches(repo) || right.matches(repo),
        }
    }

    fn score_into(
        &self,
        repo: &RepoView,
        score: &mut f32,
        fields: &mut Vec<String>,
        snippet: &mut Option<String>,
    ) {
        match self {
            QueryExpr::Term(term) => {
                add_text_term_score(repo, term, score, fields, snippet);
            }
            QueryExpr::Qualifier { field, value } => {
                if qualifier_matches(repo, field, value) {
                    *score += 1.0;
                    push_field(fields, field);
                }
            }
            QueryExpr::Not(_) => {}
            QueryExpr::And(left, right) => {
                left.score_into(repo, score, fields, snippet);
                right.score_into(repo, score, fields, snippet);
            }
            QueryExpr::Or(left, right) => {
                if left.matches(repo) {
                    left.score_into(repo, score, fields, snippet);
                }
                if right.matches(repo) {
                    right.score_into(repo, score, fields, snippet);
                }
            }
        }
    }
}

fn score_repo(repo: RepoView, query: &ParsedQuery) -> Option<SearchResult> {
    if query.is_empty() {
        return Some(SearchResult {
            repo,
            score: 0.0,
            matched_fields: Vec::new(),
            snippet: None,
        });
    }
    if !query.matches(&repo) {
        return None;
    }

    let mut score = 0.0;
    let mut fields = Vec::new();
    let mut snippet = None;
    if let Some(expr) = &query.expr {
        expr.score_into(&repo, &mut score, &mut fields, &mut snippet);
    }
    if score == 0.0 {
        score = 1.0;
    }

    Some(SearchResult {
        repo,
        score,
        matched_fields: fields,
        snippet,
    })
}

fn add_text_term_score(
    repo: &RepoView,
    query: &str,
    score: &mut f32,
    fields: &mut Vec<String>,
    snippet: &mut Option<String>,
) -> bool {
    let mut matched = false;
    matched |= add_match(score, fields, "name", &repo.full_name, query, 4.0);
    if let Some(description) = &repo.description {
        if add_match(score, fields, "description", description, query, 2.0) {
            snippet.get_or_insert_with(|| make_snippet(description, query));
            matched = true;
        }
    }
    if let Some(language) = &repo.language {
        matched |= add_match(score, fields, "language", language, query, 1.0);
    }
    for topic in &repo.topics {
        matched |= add_match(score, fields, "topic", topic, query, 1.5);
    }
    for tag in &repo.user.tags {
        matched |= add_match(score, fields, "tag", tag, query, 3.0);
    }
    if let Some(status) = &repo.user.status {
        matched |= add_match(score, fields, "status", status, query, 1.0);
    }
    if let Some(summary) = &repo.user.summary {
        if add_match(score, fields, "summary", summary, query, 3.0) {
            snippet.get_or_insert_with(|| make_snippet(summary, query));
            matched = true;
        }
    }
    if let Some(notes) = &repo.user.notes {
        if add_match(score, fields, "notes", notes, query, 2.5) {
            snippet.get_or_insert_with(|| make_snippet(notes, query));
            matched = true;
        }
    }
    if let Some(readme) = &repo.readme_snippet {
        if add_match(score, fields, "readme", readme, query, 1.2) {
            snippet.get_or_insert_with(|| make_snippet(readme, query));
            matched = true;
        }
    }
    matched
}

fn text_term_matches(repo: &RepoView, term: &str) -> bool {
    let mut score = 0.0;
    let mut fields = Vec::new();
    let mut snippet = None;
    add_text_term_score(repo, term, &mut score, &mut fields, &mut snippet)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum QueryToken {
    Word(String),
    LParen,
    RParen,
    And,
    Or,
    Not,
}

struct QueryParser {
    tokens: Vec<QueryToken>,
    position: usize,
}

impl QueryParser {
    fn new(tokens: Vec<QueryToken>) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    fn parse(mut self) -> Option<QueryExpr> {
        if self.tokens.is_empty() {
            return None;
        }
        let expr = self.parse_or().ok()?;
        (self.position == self.tokens.len()).then_some(expr)
    }

    fn parse_or(&mut self) -> Result<QueryExpr, ()> {
        let mut expr = self.parse_and()?;
        while self.consume_operator(&QueryToken::Or) {
            let right = self.parse_and()?;
            expr = QueryExpr::Or(Box::new(expr), Box::new(right));
        }
        Ok(expr)
    }

    fn parse_and(&mut self) -> Result<QueryExpr, ()> {
        let mut expr = self.parse_not()?;
        loop {
            let explicit_and = self.consume_operator(&QueryToken::And);
            if explicit_and || self.next_starts_primary() {
                let right = self.parse_not()?;
                expr = QueryExpr::And(Box::new(expr), Box::new(right));
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_not(&mut self) -> Result<QueryExpr, ()> {
        if self.consume_operator(&QueryToken::Not) {
            Ok(QueryExpr::Not(Box::new(self.parse_not()?)))
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<QueryExpr, ()> {
        match self.tokens.get(self.position).cloned() {
            Some(QueryToken::Word(word)) => {
                self.position += 1;
                Ok(word_to_expr(&word))
            }
            Some(QueryToken::LParen) => {
                self.position += 1;
                let expr = self.parse_or()?;
                if matches!(self.tokens.get(self.position), Some(QueryToken::RParen)) {
                    self.position += 1;
                    Ok(expr)
                } else {
                    Err(())
                }
            }
            _ => Err(()),
        }
    }

    fn consume_operator(&mut self, token: &QueryToken) -> bool {
        if self.tokens.get(self.position) == Some(token) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn next_starts_primary(&self) -> bool {
        matches!(
            self.tokens.get(self.position),
            Some(QueryToken::Word(_) | QueryToken::LParen | QueryToken::Not)
        )
    }
}

fn parse_query(query: &str) -> Option<QueryExpr> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    QueryParser::new(tokenize_query(query))
        .parse()
        .or_else(|| Some(QueryExpr::Term(query.to_string())))
}

fn tokenize_query(query: &str) -> Vec<QueryToken> {
    let mut tokens = Vec::new();
    let mut chars = query.char_indices().peekable();
    while let Some((index, ch)) = chars.peek().copied() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }
        if ch == '(' {
            chars.next();
            tokens.push(QueryToken::LParen);
            continue;
        }
        if ch == ')' {
            chars.next();
            tokens.push(QueryToken::RParen);
            continue;
        }

        let mut end = index;
        let mut word = String::new();
        while let Some((byte_index, current)) = chars.peek().copied() {
            if current.is_whitespace() || current == '(' || current == ')' {
                break;
            }
            chars.next();
            end = byte_index + current.len_utf8();
            if current == '"' {
                word.push(current);
                for (quoted_index, quoted) in chars.by_ref() {
                    end = quoted_index + quoted.len_utf8();
                    word.push(quoted);
                    if quoted == '"' {
                        break;
                    }
                }
            } else {
                word.push(current);
            }
        }
        if end > index {
            tokens.push(word_token(word));
        }
    }
    tokens
}

fn word_token(word: String) -> QueryToken {
    match word.to_ascii_uppercase().as_str() {
        "AND" => QueryToken::And,
        "OR" => QueryToken::Or,
        "NOT" => QueryToken::Not,
        _ => QueryToken::Word(word),
    }
}

fn word_to_expr(word: &str) -> QueryExpr {
    if let Some(rest) = word.strip_prefix('-').filter(|rest| !rest.is_empty()) {
        return QueryExpr::Not(Box::new(word_to_expr(rest)));
    }
    if let Some((field, value)) = split_qualifier(word) {
        return QueryExpr::Qualifier {
            field: field.to_ascii_lowercase(),
            value: strip_quotes(value).to_string(),
        };
    }
    QueryExpr::Term(strip_quotes(word).to_string())
}

fn split_qualifier(word: &str) -> Option<(&str, &str)> {
    let colon = word.find(':');
    let equals = word.find('=');
    let index = match (colon, equals) {
        (Some(colon), Some(equals)) => colon.min(equals),
        (Some(index), None) | (None, Some(index)) => index,
        (None, None) => return None,
    };
    let (field, rest) = word.split_at(index);
    let value = &rest[1..];
    (!field.is_empty() && !value.is_empty()).then_some((field, value))
}

fn qualifier_matches(repo: &RepoView, field: &str, value: &str) -> bool {
    match field {
        "owner" | "user" | "org" => match_text(&repo.owner, value, TextDefault::Exact),
        "name" => match_text(&repo.name, value, TextDefault::Contains),
        "repo" | "full_name" | "repository" => {
            match_text(&repo.full_name, value, TextDefault::Contains)
        }
        "language" | "lang" => repo
            .language
            .as_deref()
            .map(|language| match_text(language, value, TextDefault::Exact))
            .unwrap_or(false),
        "topic" | "topics" => repo
            .topics
            .iter()
            .any(|topic| match_text(topic, value, TextDefault::Exact)),
        "tag" | "tags" => repo
            .user
            .tags
            .iter()
            .any(|tag| match_text(tag, value, TextDefault::Exact)),
        "status" => repo
            .user
            .status
            .as_deref()
            .map(|status| match_text(status, value, TextDefault::Exact))
            .unwrap_or(false),
        "description" => repo
            .description
            .as_deref()
            .map(|description| match_text(description, value, TextDefault::Contains))
            .unwrap_or(false),
        "summary" => repo
            .user
            .summary
            .as_deref()
            .map(|summary| match_text(summary, value, TextDefault::Contains))
            .unwrap_or(false),
        "notes" => repo
            .user
            .notes
            .as_deref()
            .map(|notes| match_text(notes, value, TextDefault::Contains))
            .unwrap_or(false),
        "readme" => repo
            .readme_snippet
            .as_deref()
            .map(|readme| match_text(readme, value, TextDefault::Contains))
            .unwrap_or(false),
        "default_branch" | "branch" => repo
            .default_branch
            .as_deref()
            .map(|branch| match_text(branch, value, TextDefault::Exact))
            .unwrap_or(false),
        "archived" => parse_bool(value)
            .map(|expected| repo.archived == expected)
            .unwrap_or(false),
        "current" => parse_bool(value)
            .map(|expected| repo.current == expected)
            .unwrap_or(false),
        "is" => match_is(repo, value),
        "stars" | "stargazers" | "stargazers_count" => repo
            .stargazers_count
            .map(|stars| match_number(stars, value))
            .unwrap_or(false),
        "forks" | "forks_count" => repo
            .forks_count
            .map(|forks| match_number(forks, value))
            .unwrap_or(false),
        _ => false,
    }
}

#[derive(Clone, Copy)]
enum TextDefault {
    Exact,
    Contains,
}

fn match_text(value: &str, pattern: &str, default: TextDefault) -> bool {
    let pattern = strip_quotes(pattern).trim();
    if pattern.is_empty() {
        return false;
    }
    if let Some(prefix) = pattern.strip_prefix('^') {
        return starts_with_ignore_ascii_case(value, prefix);
    }
    if let Some(exact) = pattern.strip_prefix('=') {
        return value.eq_ignore_ascii_case(exact);
    }
    if let Some(contains) = pattern.strip_prefix('~') {
        return contains_text_match(value, contains);
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        if !prefix.contains('*') {
            return starts_with_ignore_ascii_case(value, prefix);
        }
    }
    match default {
        TextDefault::Exact => value.eq_ignore_ascii_case(pattern),
        TextDefault::Contains => contains_text_match(value, pattern),
    }
}

fn match_is(repo: &RepoView, value: &str) -> bool {
    match strip_quotes(value).to_ascii_lowercase().as_str() {
        "archived" => repo.archived,
        "current" | "starred" => repo.current,
        "active" => repo.current && !repo.archived,
        _ => false,
    }
}

fn match_number(actual: u64, pattern: &str) -> bool {
    let pattern = strip_quotes(pattern).trim();
    if let Some((start, end)) = pattern.split_once("..") {
        let start = start.trim().parse::<u64>().ok();
        let end = end.trim().parse::<u64>().ok();
        return start.map(|value| actual >= value).unwrap_or(true)
            && end.map(|value| actual <= value).unwrap_or(true);
    }
    for (prefix, predicate) in [
        (">=", NumberPredicate::Gte),
        ("<=", NumberPredicate::Lte),
        (">", NumberPredicate::Gt),
        ("<", NumberPredicate::Lt),
        ("=", NumberPredicate::Eq),
    ] {
        if let Some(value) = pattern.strip_prefix(prefix) {
            return value
                .trim()
                .parse::<u64>()
                .map(|expected| predicate.matches(actual, expected))
                .unwrap_or(false);
        }
    }
    pattern
        .parse::<u64>()
        .map(|expected| actual == expected)
        .unwrap_or(false)
}

#[derive(Clone, Copy)]
enum NumberPredicate {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
}

impl NumberPredicate {
    fn matches(self, actual: u64, expected: u64) -> bool {
        match self {
            NumberPredicate::Gt => actual > expected,
            NumberPredicate::Gte => actual >= expected,
            NumberPredicate::Lt => actual < expected,
            NumberPredicate::Lte => actual <= expected,
            NumberPredicate::Eq => actual == expected,
        }
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match strip_quotes(value).to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" | "on" => Some(true),
        "false" | "no" | "0" | "off" => Some(false),
        _ => None,
    }
}

fn contains_text_match(value: &str, needle: &str) -> bool {
    let value_normalized = value.to_lowercase();
    let needle_normalized = needle.to_lowercase();
    value_normalized.contains(&needle_normalized)
        || cjk_overlap_score(&value_normalized, &needle_normalized).is_some()
}

fn starts_with_ignore_ascii_case(value: &str, prefix: &str) -> bool {
    value
        .to_ascii_lowercase()
        .starts_with(&prefix.to_ascii_lowercase())
}

fn strip_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

fn push_field(fields: &mut Vec<String>, field: &str) {
    if !fields.iter().any(|existing| existing == field) {
        fields.push(field.to_string());
    }
}

fn add_match(
    score: &mut f32,
    fields: &mut Vec<String>,
    field: &str,
    value: &str,
    query: &str,
    weight: f32,
) -> bool {
    let value_normalized = value.to_lowercase();
    let query_normalized = query.to_lowercase();
    let match_strength = if value_normalized.contains(&query_normalized) {
        Some(1.0)
    } else {
        cjk_overlap_score(&value_normalized, &query_normalized)
    };
    if let Some(match_strength) = match_strength {
        *score += weight * match_strength;
        push_field(fields, field);
        true
    } else {
        false
    }
}

fn cjk_overlap_score(value: &str, query: &str) -> Option<f32> {
    if !query.chars().any(is_cjk) {
        return None;
    }
    let query_tokens = cjk_search_tokens(query);
    if query_tokens.is_empty() {
        return None;
    }
    let value_tokens = cjk_search_tokens(value);
    let matched = query_tokens
        .iter()
        .filter(|token| value_tokens.contains(*token))
        .count();
    let ratio = matched as f32 / query_tokens.len() as f32;
    let threshold = if query_tokens.len() <= 2 { 1.0 } else { 0.6 };
    (ratio >= threshold).then_some(ratio)
}

fn cjk_search_tokens(value: &str) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    let mut cjk_run = Vec::new();
    let mut word = String::new();

    let flush_word = |tokens: &mut BTreeSet<String>, word: &mut String| {
        if !word.is_empty() {
            tokens.insert(word.clone());
            word.clear();
        }
    };
    let flush_cjk = |tokens: &mut BTreeSet<String>, run: &mut Vec<char>| {
        if run.len() == 1 {
            tokens.insert(run[0].to_string());
        } else {
            for pair in run.windows(2) {
                tokens.insert(pair.iter().collect());
            }
        }
        run.clear();
    };

    for ch in value.chars() {
        if is_cjk(ch) {
            flush_word(&mut tokens, &mut word);
            cjk_run.push(ch);
        } else if ch.is_alphanumeric() {
            flush_cjk(&mut tokens, &mut cjk_run);
            word.push(ch);
        } else {
            flush_word(&mut tokens, &mut word);
            flush_cjk(&mut tokens, &mut cjk_run);
        }
    }
    flush_word(&mut tokens, &mut word);
    flush_cjk(&mut tokens, &mut cjk_run);
    tokens
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch,
        '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{20000}'..='\u{2A6DF}'
            | '\u{2A700}'..='\u{2B73F}'
            | '\u{2B740}'..='\u{2B81F}'
            | '\u{2B820}'..='\u{2CEAF}'
            | '\u{2CEB0}'..='\u{2EBEF}'
    )
}

fn make_snippet(value: &str, query: &str) -> String {
    let query = query.to_ascii_lowercase();
    let lower = value.to_ascii_lowercase();
    let Some(index) = lower.find(&query) else {
        return value
            .chars()
            .take(180)
            .collect::<String>()
            .replace('\n', " ");
    };
    let match_start = floor_char_boundary(value, index);
    let match_end = ceil_char_boundary(value, index + query.len());
    let start = char_context_start(value, match_start, 60);
    let end = char_context_end(value, match_end, 120);
    value[start..end].replace('\n', " ")
}

fn floor_char_boundary(value: &str, mut index: usize) -> usize {
    index = index.min(value.len());
    while !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(value: &str, mut index: usize) -> usize {
    index = index.min(value.len());
    while !value.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn char_context_start(value: &str, index: usize, max_chars: usize) -> usize {
    value[..index]
        .char_indices()
        .rev()
        .nth(max_chars)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(0)
}

fn char_context_end(value: &str, index: usize, max_chars: usize) -> usize {
    value[index..]
        .char_indices()
        .nth(max_chars)
        .map(|(byte_index, _)| index + byte_index)
        .unwrap_or(value.len())
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

    fn repo_for(owner: &str, name: &str, tags: &[&str]) -> RepoView {
        let mut repo = repo(name, tags);
        repo.owner = owner.to_string();
        repo.full_name = format!("{owner}/{name}");
        repo
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

    #[test]
    fn search_snippet_handles_multibyte_text() {
        let mut repo = repo("demo", &[]);
        repo.description = Some("可视化大屏，地理轮廓精确呈现3D地图，支持 Rust 插件".to_string());
        let filters = RepoFilters {
            q: Some("rust".to_string()),
            ..RepoFilters::default()
        };

        let results = search_repos(vec![repo], &filters);

        assert_eq!(results.items.len(), 1);
        assert!(results.items[0]
            .snippet
            .as_deref()
            .unwrap()
            .contains("Rust"));
    }

    #[test]
    fn search_matches_chinese_cjk_ngram_overlap() {
        let mut repo = repo("demo", &[]);
        repo.user.summary = Some("支持中文本地检索和搜索控制台".to_string());
        let filters = RepoFilters {
            q: Some("中文搜索".to_string()),
            ..RepoFilters::default()
        };

        let results = search_repos(vec![repo], &filters);

        assert_eq!(results.items.len(), 1);
        assert!(results.items[0]
            .matched_fields
            .iter()
            .any(|field| field == "summary"));
        assert!(query_uses_cjk(Some("中文搜索")));
    }

    #[test]
    fn qualifier_contains_uses_chinese_cjk_ngram_overlap() {
        let mut repo = repo("demo", &[]);
        repo.user.notes = Some("向量 DB 数据库和中文知识库检索".to_string());
        let filters = RepoFilters {
            q: Some("notes:向量数据库".to_string()),
            ..RepoFilters::default()
        };

        let results = list_repos(vec![repo], &filters);

        assert_eq!(results.total, 1);
    }

    #[test]
    fn search_results_support_explicit_sorting() {
        let mut low = repo_for("alice", "low", &[]);
        low.description = Some("agent toolkit".to_string());
        low.stargazers_count = Some(10);
        let mut high = repo_for("alice", "high", &[]);
        high.description = Some("agent toolkit".to_string());
        high.stargazers_count = Some(100);
        let filters = RepoFilters {
            q: Some("agent".to_string()),
            sort: Some(RepoSort::Stars),
            direction: Some(SortDirection::Desc),
            ..RepoFilters::default()
        };

        let results = search_repos(vec![low, high], &filters);

        assert_eq!(results.items[0].repo.name, "high");
        assert_eq!(results.items[1].repo.name, "low");
    }

    #[test]
    fn list_results_sort_by_forks_without_query() {
        let mut low = repo_for("alice", "low", &[]);
        low.forks_count = Some(2);
        let mut high = repo_for("alice", "high", &[]);
        high.forks_count = Some(50);
        let filters = RepoFilters {
            sort: Some(RepoSort::Forks),
            direction: Some(SortDirection::Desc),
            ..RepoFilters::default()
        };

        let results = list_repos(vec![low, high], &filters);

        assert_eq!(results.items[0].name, "high");
        assert_eq!(results.items[1].name, "low");
    }

    #[test]
    fn search_results_sort_by_name_ascending() {
        let mut beta = repo_for("alice", "beta", &[]);
        beta.description = Some("agent toolkit".to_string());
        let mut alpha = repo_for("alice", "alpha", &[]);
        alpha.description = Some("agent toolkit".to_string());
        let filters = RepoFilters {
            q: Some("agent".to_string()),
            sort: Some(RepoSort::Name),
            direction: Some(SortDirection::Asc),
            ..RepoFilters::default()
        };

        let results = search_repos(vec![beta, alpha], &filters);

        assert_eq!(results.items[0].repo.name, "alpha");
        assert_eq!(results.items[1].repo.name, "beta");
    }

    #[test]
    fn query_expression_filters_by_owner_and_name_prefix() {
        let filters = RepoFilters {
            q: Some("owner=nickfan AND name:^T".to_string()),
            ..RepoFilters::default()
        };

        let results = list_repos(
            vec![
                repo_for("nickfan", "Toolbox", &[]),
                repo_for("nickfan", "starsync", &[]),
                repo_for("alice", "Toolbox", &[]),
            ],
            &filters,
        );

        assert_eq!(results.total, 1);
        assert_eq!(results.items[0].full_name, "nickfan/Toolbox");
    }

    #[test]
    fn query_expression_supports_or_not_and_github_style_qualifiers() {
        let mut rust_cli = repo_for("alice", "tooling", &["local"]);
        rust_cli.language = Some("Rust".to_string());
        rust_cli.topics = vec!["cli".to_string()];
        rust_cli.stargazers_count = Some(1500);
        rust_cli.forks_count = Some(200);

        let mut rust_web = repo_for("alice", "webapp", &[]);
        rust_web.language = Some("Rust".to_string());
        rust_web.topics = vec!["web".to_string()];
        rust_web.stargazers_count = Some(2000);

        let filters = RepoFilters {
            q: Some(
                "(language:Rust AND topic:cli AND stars:>=1000 AND forks:>=100) OR owner:bob"
                    .to_string(),
            ),
            ..RepoFilters::default()
        };

        let results = list_repos(
            vec![rust_cli, rust_web, repo_for("bob", "notes", &[])],
            &filters,
        );

        assert_eq!(results.total, 2);
        assert!(results
            .items
            .iter()
            .any(|repo| repo.full_name == "alice/tooling"));
        assert!(results
            .items
            .iter()
            .any(|repo| repo.full_name == "bob/notes"));
    }

    #[test]
    fn query_expression_supports_negative_qualifiers() {
        let mut rust_cli = repo_for("alice", "tooling", &[]);
        rust_cli.language = Some("Rust".to_string());
        rust_cli.topics = vec!["cli".to_string()];
        let mut rust_web = repo_for("alice", "webapp", &[]);
        rust_web.language = Some("Rust".to_string());
        rust_web.topics = vec!["web".to_string()];
        let filters = RepoFilters {
            q: Some("language:Rust -topic:web".to_string()),
            ..RepoFilters::default()
        };

        let results = list_repos(vec![rust_cli, rust_web], &filters);

        assert_eq!(results.total, 1);
        assert_eq!(results.items[0].full_name, "alice/tooling");
    }

    #[test]
    fn search_returns_qualifier_only_expression_matches() {
        let filters = RepoFilters {
            q: Some("owner:alice AND name:Tool*".to_string()),
            ..RepoFilters::default()
        };

        let results = search_repos(vec![repo_for("alice", "Toolbox", &[])], &filters);

        assert_eq!(results.items.len(), 1);
        assert!(results.items[0].score > 0.0);
        assert!(results.items[0]
            .matched_fields
            .iter()
            .any(|field| field == "owner"));
    }
}
