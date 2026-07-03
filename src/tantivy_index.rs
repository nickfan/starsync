use crate::{
    models::{ListResponse, RepoFilters, RepoView, SearchResult},
    search,
};
use anyhow::{Context, Result};
use pinyin::ToPinyin;
use std::path::{Path, PathBuf};
use tantivy::{
    collector::TopDocs,
    doc,
    query::{AllQuery, Query, QueryParser},
    schema::{
        Field, IndexRecordOption, Schema, TantivyDocument, TextFieldIndexing, TextOptions, Value,
        STORED,
    },
    tokenizer::{LowerCaser, RemoveLongFilter, TextAnalyzer},
    Index,
};

const TOKENIZER: &str = "jieba";
const SEARCH_LIMIT: usize = 10_000;

#[derive(Clone, Debug)]
pub struct TantivyIndex {
    path: PathBuf,
}

#[derive(Clone)]
struct IndexFields {
    full_name: Field,
    owner: Field,
    name: Field,
    description: Field,
    language: Field,
    topics: Field,
    tags: Field,
    user_lists: Field,
    github_lists: Field,
    status: Field,
    summary: Field,
    notes: Field,
    readme: Field,
    cjk_ngrams: Field,
    pinyin: Field,
    pinyin_initials: Field,
    repo_json: Field,
}

impl TantivyIndex {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.path.join("meta.json").exists()
    }

    pub fn rebuild(&self, repos: &[RepoView]) -> Result<()> {
        if self.path.exists() {
            std::fs::remove_dir_all(&self.path)
                .with_context(|| format!("failed to remove {}", self.path.display()))?;
        }
        std::fs::create_dir_all(&self.path)
            .with_context(|| format!("failed to create {}", self.path.display()))?;

        let (schema, fields) = schema();
        let index = Index::create_in_dir(&self.path, schema)
            .with_context(|| format!("failed to create Tantivy index {}", self.path.display()))?;
        register_tokenizer(&index);

        let mut writer = index.writer(50_000_000)?;
        for repo in repos {
            writer.add_document(repo_document(repo, &fields))?;
        }
        writer.commit()?;
        Ok(())
    }

    pub fn search(&self, filters: &RepoFilters) -> Result<ListResponse<SearchResult>> {
        let index = Index::open_in_dir(&self.path)
            .with_context(|| format!("failed to open Tantivy index {}", self.path.display()))?;
        register_tokenizer(&index);
        let fields = fields_from_schema(index.schema())?;

        let reader = index.reader()?;
        let searcher = reader.searcher();
        let require_query_match = filters
            .q
            .as_deref()
            .is_some_and(|query| search::query_uses_structured_syntax(Some(query)));
        let query = build_query(&index, &fields, filters)?;
        let top_docs =
            searcher.search(&query, &TopDocs::with_limit(SEARCH_LIMIT).order_by_score())?;

        let mut results = Vec::new();
        for (tantivy_score, address) in top_docs {
            let document: TantivyDocument = searcher.doc(address)?;
            let Some(raw) = document
                .get_first(fields.repo_json)
                .and_then(|value| value.as_str())
            else {
                continue;
            };
            let repo: RepoView = serde_json::from_str(raw)?;
            if let Some(mut result) =
                search::result_for_index_hit(repo, filters, require_query_match)
            {
                result.score += tantivy_score;
                results.push(result);
            }
        }

        Ok(search::order_and_page_search_results(results, filters))
    }
}

fn build_query(
    index: &Index,
    fields: &IndexFields,
    filters: &RepoFilters,
) -> Result<Box<dyn Query>> {
    let Some(query) = filters
        .q
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
    else {
        return Ok(Box::new(AllQuery));
    };

    if search::query_uses_structured_syntax(Some(query)) {
        return Ok(Box::new(AllQuery));
    }

    let parser = QueryParser::for_index(
        index,
        vec![
            fields.full_name,
            fields.owner,
            fields.name,
            fields.description,
            fields.language,
            fields.topics,
            fields.tags,
            fields.user_lists,
            fields.github_lists,
            fields.status,
            fields.summary,
            fields.notes,
            fields.readme,
            fields.cjk_ngrams,
            fields.pinyin,
            fields.pinyin_initials,
        ],
    );
    let query = expand_query_terms(query);
    parser.parse_query(&query).map_err(Into::into)
}

fn schema() -> (Schema, IndexFields) {
    let mut builder = Schema::builder();
    let full_name = builder.add_text_field("full_name", text_options());
    let owner = builder.add_text_field("owner", text_options());
    let name = builder.add_text_field("name", text_options());
    let description = builder.add_text_field("description", text_options());
    let language = builder.add_text_field("language", text_options());
    let topics = builder.add_text_field("topics", text_options());
    let tags = builder.add_text_field("tags", text_options());
    let user_lists = builder.add_text_field("user_lists", text_options());
    let github_lists = builder.add_text_field("github_lists", text_options());
    let status = builder.add_text_field("status", text_options());
    let summary = builder.add_text_field("summary", text_options());
    let notes = builder.add_text_field("notes", text_options());
    let readme = builder.add_text_field("readme", text_options());
    let cjk_ngrams = builder.add_text_field(
        "cjk_ngrams",
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("default")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        ),
    );
    let pinyin = builder.add_text_field(
        "pinyin",
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("default")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        ),
    );
    let pinyin_initials = builder.add_text_field(
        "pinyin_initials",
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("raw")
                .set_index_option(IndexRecordOption::Basic),
        ),
    );
    let repo_json = builder.add_text_field("repo_json", STORED);
    let schema = builder.build();
    let fields = IndexFields {
        full_name,
        owner,
        name,
        description,
        language,
        topics,
        tags,
        user_lists,
        github_lists,
        status,
        summary,
        notes,
        readme,
        cjk_ngrams,
        pinyin,
        pinyin_initials,
        repo_json,
    };
    (schema, fields)
}

fn fields_from_schema(schema: Schema) -> Result<IndexFields> {
    let field = |name: &str| {
        schema
            .get_field(name)
            .with_context(|| format!("Tantivy schema missing {name} field"))
    };
    Ok(IndexFields {
        full_name: field("full_name")?,
        owner: field("owner")?,
        name: field("name")?,
        description: field("description")?,
        language: field("language")?,
        topics: field("topics")?,
        tags: field("tags")?,
        user_lists: field("user_lists")?,
        github_lists: field("github_lists")?,
        status: field("status")?,
        summary: field("summary")?,
        notes: field("notes")?,
        readme: field("readme")?,
        cjk_ngrams: field("cjk_ngrams")?,
        pinyin: field("pinyin")?,
        pinyin_initials: field("pinyin_initials")?,
        repo_json: field("repo_json")?,
    })
}

fn text_options() -> TextOptions {
    TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(TOKENIZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored()
}

fn register_tokenizer(index: &Index) {
    let tokenizer = tantivy_jieba::JiebaTokenizer::new();
    let analyzer = TextAnalyzer::builder(tokenizer)
        .filter(RemoveLongFilter::limit(40))
        .filter(LowerCaser)
        .build();
    index.tokenizers().register(TOKENIZER, analyzer);
}

fn repo_document(repo: &RepoView, fields: &IndexFields) -> TantivyDocument {
    let searchable = searchable_text(repo);
    doc!(
        fields.full_name => repo.full_name.clone(),
        fields.owner => repo.owner.clone(),
        fields.name => repo.name.clone(),
        fields.description => repo.description.clone().unwrap_or_default(),
        fields.language => repo.language.clone().unwrap_or_default(),
        fields.topics => repo.topics.join(" "),
        fields.tags => repo.user.tags.join(" "),
        fields.user_lists => repo.user.lists.join(" "),
        fields.github_lists => repo.github_lists.join(" "),
        fields.status => repo.user.status.clone().unwrap_or_default(),
        fields.summary => repo.user.summary.clone().unwrap_or_default(),
        fields.notes => repo.user.notes.clone().unwrap_or_default(),
        fields.readme => repo.readme_snippet.clone().unwrap_or_default(),
        fields.cjk_ngrams => cjk_ngrams(&searchable),
        fields.pinyin => to_pinyin_words(&searchable),
        fields.pinyin_initials => to_pinyin_initials(&searchable),
        fields.repo_json => serde_json::to_string(repo).unwrap_or_default(),
    )
}

fn searchable_text(repo: &RepoView) -> String {
    [
        repo.full_name.as_str(),
        repo.description.as_deref().unwrap_or_default(),
        repo.language.as_deref().unwrap_or_default(),
        &repo.topics.join(" "),
        &repo.user.tags.join(" "),
        &repo.user.lists.join(" "),
        &repo.github_lists.join(" "),
        repo.user.status.as_deref().unwrap_or_default(),
        repo.user.summary.as_deref().unwrap_or_default(),
        repo.user.notes.as_deref().unwrap_or_default(),
        repo.readme_snippet.as_deref().unwrap_or_default(),
    ]
    .join(" ")
}

fn to_pinyin_words(value: &str) -> String {
    value
        .to_pinyin()
        .map(|item| {
            item.map(|pinyin| pinyin.plain().to_string())
                .unwrap_or_else(|| " ".to_string())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn to_pinyin_initials(value: &str) -> String {
    value
        .to_pinyin()
        .filter_map(|item| item.and_then(|pinyin| pinyin.plain().chars().next()))
        .collect()
}

fn expand_query_terms(query: &str) -> String {
    let ngrams = cjk_ngrams(query);
    if ngrams.is_empty() {
        query.to_string()
    } else {
        ngrams.split_whitespace().collect::<Vec<_>>().join(" OR ")
    }
}

fn cjk_ngrams(value: &str) -> String {
    let mut terms = Vec::new();
    let mut run = Vec::new();
    for ch in value.chars() {
        if is_cjk(ch) {
            run.push(ch);
        } else {
            push_cjk_terms(&mut terms, &mut run);
        }
    }
    push_cjk_terms(&mut terms, &mut run);
    terms.join(" ")
}

fn push_cjk_terms(terms: &mut Vec<String>, run: &mut Vec<char>) {
    if run.len() == 1 {
        terms.push(run[0].to_string());
    } else {
        for pair in run.windows(2) {
            terms.push(pair.iter().collect());
        }
    }
    run.clear();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserMeta;

    #[test]
    fn rebuilds_and_searches_chinese_and_pinyin() {
        let dir = tempfile::tempdir().unwrap();
        let index = TantivyIndex::new(dir.path().join("search"));
        let repo = RepoView {
            owner: "alice".to_string(),
            name: "vector-db".to_string(),
            full_name: "alice/vector-db".to_string(),
            current: true,
            user: UserMeta {
                tags: vec!["retrieval".to_string()],
                summary: Some("中文本地检索控制台".to_string()),
                ..UserMeta::default()
            },
            ..RepoView::default()
        };

        index.rebuild(&[repo]).unwrap();

        let chinese = index
            .search(&RepoFilters {
                q: Some("中文检索".to_string()),
                ..RepoFilters::default()
            })
            .unwrap();
        assert_eq!(chinese.items.len(), 1);

        let pinyin = index
            .search(&RepoFilters {
                q: Some("zhong wen jian suo".to_string()),
                ..RepoFilters::default()
            })
            .unwrap();
        assert_eq!(pinyin.items.len(), 1);
    }
}
