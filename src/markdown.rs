use crate::models::{MirrorState, RepoIdentity, RepoMeta, RepoView};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

use serde::{Deserialize, Serialize};

const CATALOG_YAML_FILE: &str = "catalog.yaml";
const CATALOG_JSON_FILE: &str = "catalog.json";
const BY_REPO_INDEX_FILE: &str = "INDEX.by-repo.md";
const BY_OWNER_INDEX_FILE: &str = "INDEX.by-owner.md";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RepoMetaDocument {
    pub meta: RepoMeta,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct MarkdownStore {
    repos_dir: PathBuf,
    state_dir: PathBuf,
}

impl MarkdownStore {
    pub fn new(repos_dir: PathBuf, state_dir: PathBuf) -> Self {
        Self {
            repos_dir,
            state_dir,
        }
    }

    pub fn init(&self) -> Result<()> {
        fs::create_dir_all(&self.repos_dir)
            .with_context(|| format!("failed to create {}", self.repos_dir.display()))?;
        fs::create_dir_all(&self.state_dir)
            .with_context(|| format!("failed to create {}", self.state_dir.display()))?;
        let index = self.index_path();
        if !index.exists() {
            self.write_catalog(&[])?;
        }
        Ok(())
    }

    pub fn repos_dir(&self) -> &Path {
        &self.repos_dir
    }

    pub fn index_path(&self) -> PathBuf {
        self.repos_dir.join("INDEX.md")
    }

    pub fn catalog_yaml_path(&self) -> PathBuf {
        self.repos_dir.join(CATALOG_YAML_FILE)
    }

    pub fn catalog_json_path(&self) -> PathBuf {
        self.repos_dir.join(CATALOG_JSON_FILE)
    }

    pub fn by_repo_index_path(&self) -> PathBuf {
        self.repos_dir.join(BY_REPO_INDEX_FILE)
    }

    pub fn by_owner_index_path(&self) -> PathBuf {
        self.repos_dir.join(BY_OWNER_INDEX_FILE)
    }

    pub fn mirror_path(&self) -> PathBuf {
        self.state_dir.join("mirror.json")
    }

    pub fn repo_meta_path(&self, identity: &RepoIdentity) -> PathBuf {
        self.repos_dir
            .join(&identity.owner)
            .join(&identity.name)
            .join("INDEX.md")
    }

    pub fn read_mirror(&self) -> Result<MirrorState> {
        let path = self.mirror_path();
        if !path.exists() {
            return Ok(MirrorState::default());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn write_mirror(&self, state: &MirrorState) -> Result<()> {
        fs::create_dir_all(&self.state_dir)
            .with_context(|| format!("failed to create {}", self.state_dir.display()))?;
        let path = self.mirror_path();
        let raw = serde_json::to_string_pretty(state)?;
        fs::write(&path, format!("{raw}\n"))
            .with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn write_catalog(&self, repos: &[RepoView]) -> Result<()> {
        fs::create_dir_all(&self.repos_dir)
            .with_context(|| format!("failed to create {}", self.repos_dir.display()))?;

        let mut items: Vec<RepoCatalogItem> = repos.iter().map(RepoCatalogItem::from).collect();
        items.sort_by(|a, b| a.full_name.cmp(&b.full_name));
        let counts = CatalogCounts::from_items(&items);
        let generated_at = Utc::now();
        let catalog = RepoCatalog {
            starsync: DocumentSchema::new("starsync.repo_catalog.v1"),
            kind: "repo_catalog".to_string(),
            generated_at,
            counts: counts.clone(),
            items,
        };

        let yaml = serde_yaml::to_string(&catalog)?;
        fs::write(self.catalog_yaml_path(), format!("{yaml}"))
            .with_context(|| format!("failed to write {}", self.catalog_yaml_path().display()))?;

        let json = serde_json::to_string_pretty(&catalog)?;
        fs::write(self.catalog_json_path(), format!("{json}\n"))
            .with_context(|| format!("failed to write {}", self.catalog_json_path().display()))?;

        self.write_root_index(&catalog)?;
        self.write_initial_indexes(&catalog)?;
        Ok(())
    }

    pub fn read_meta(&self, identity: &RepoIdentity) -> Result<Option<RepoMetaDocument>> {
        let path = self.repo_meta_path(identity);
        if !path.exists() {
            return Ok(None);
        }
        read_meta_document(&path).map(Some)
    }

    pub fn write_meta(&self, document: &RepoMetaDocument) -> Result<()> {
        let path = self.repo_meta_path(&document.meta.repo);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        write_meta_document(&path, document)
    }

    pub fn ensure_meta(&self, identity: &RepoIdentity) -> Result<RepoMetaDocument> {
        if let Some(document) = self.read_meta(identity)? {
            return Ok(document);
        }
        let document = RepoMetaDocument {
            meta: RepoMeta::new(identity.clone()),
            body: format!("# {}\n\n", identity.full_name()),
        };
        self.write_meta(&document)?;
        Ok(document)
    }

    pub fn list_meta(&self) -> Result<Vec<RepoMetaDocument>> {
        if !self.repos_dir.exists() {
            return Ok(Vec::new());
        }
        let mut docs = Vec::new();
        for entry in WalkDir::new(&self.repos_dir)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() || entry.file_name() != "INDEX.md" {
                continue;
            }
            if entry.path() == self.index_path() {
                continue;
            }
            docs.push(read_meta_document(entry.path())?);
        }
        docs.sort_by_key(|document| document.meta.repo.full_name());
        Ok(docs)
    }

    pub fn mark_meta_archived(&self, identity: &RepoIdentity) -> Result<RepoMetaDocument> {
        let mut document = self.ensure_meta(identity)?;
        document.meta.archived = true;
        document.meta.user.status = Some("archived".to_string());
        if document.body.trim().is_empty() {
            document.body = format!(
                "# {}\n\nArchived locally by StarSync at {}.\n",
                identity.full_name(),
                Utc::now().to_rfc3339()
            );
        }
        self.write_meta(&document)?;
        Ok(document)
    }

    fn write_root_index(&self, catalog: &RepoCatalog) -> Result<()> {
        let front_matter = RepoIndexFrontMatter {
            starsync: DocumentSchema::new("starsync.repo_index.v1"),
            kind: "repo_index".to_string(),
            generated_at: catalog.generated_at,
            counts: catalog.counts.clone(),
            catalog: CatalogFiles {
                yaml: CATALOG_YAML_FILE.to_string(),
                json: CATALOG_JSON_FILE.to_string(),
            },
            indexes: IndexFiles {
                by_repo_initial: BY_REPO_INDEX_FILE.to_string(),
                by_owner_initial: BY_OWNER_INDEX_FILE.to_string(),
            },
        };
        let mut body = String::new();
        body.push_str("# StarSync Repository Index\n\n");
        body.push_str(&format!(
            "Generated at: `{}`\n\n",
            catalog.generated_at.to_rfc3339()
        ));
        body.push_str(&format!(
            "- Total repos: {}\n- Current repos: {}\n- Archived/tombstone repos: {}\n\n",
            catalog.counts.total, catalog.counts.current, catalog.counts.archived
        ));
        body.push_str("## Data Files\n\n");
        body.push_str(&format!(
            "- Machine YAML catalog: [{}]({})\n",
            CATALOG_YAML_FILE, CATALOG_YAML_FILE
        ));
        body.push_str(&format!(
            "- Machine JSON catalog: [{}]({})\n",
            CATALOG_JSON_FILE, CATALOG_JSON_FILE
        ));
        body.push_str(&format!(
            "- Repo-name initial index: [{}]({})\n",
            BY_REPO_INDEX_FILE, BY_REPO_INDEX_FILE
        ));
        body.push_str(&format!(
            "- Owner initial index: [{}]({})\n\n",
            BY_OWNER_INDEX_FILE, BY_OWNER_INDEX_FILE
        ));
        body.push_str("## Repositories\n\n");
        body.push_str(&render_catalog_table(&catalog.items));
        write_markdown_with_front_matter(&self.index_path(), &front_matter, &body)
    }

    fn write_initial_indexes(&self, catalog: &RepoCatalog) -> Result<()> {
        let by_repo = InitialIndexFrontMatter {
            starsync: DocumentSchema::new("starsync.repo_initial_index.v1"),
            kind: "repo_initial_index".to_string(),
            generated_at: catalog.generated_at,
            axis: "repo".to_string(),
            counts: catalog.counts.clone(),
            source_catalog: CATALOG_JSON_FILE.to_string(),
        };
        let by_repo_body = render_initial_index(
            "StarSync Repo Initial Index",
            "repo name",
            &catalog.items,
            |item| &item.name,
            sort_by_repo_name,
        );
        write_markdown_with_front_matter(&self.by_repo_index_path(), &by_repo, &by_repo_body)?;

        let by_owner = InitialIndexFrontMatter {
            starsync: DocumentSchema::new("starsync.owner_initial_index.v1"),
            kind: "owner_initial_index".to_string(),
            generated_at: catalog.generated_at,
            axis: "owner".to_string(),
            counts: catalog.counts.clone(),
            source_catalog: CATALOG_JSON_FILE.to_string(),
        };
        let by_owner_body = render_initial_index(
            "StarSync Owner Initial Index",
            "owner",
            &catalog.items,
            |item| &item.owner,
            sort_by_owner_name,
        );
        write_markdown_with_front_matter(&self.by_owner_index_path(), &by_owner, &by_owner_body)
    }
}

#[derive(Clone, Debug, Serialize)]
struct DocumentSchema {
    schema: String,
}

impl DocumentSchema {
    fn new(schema: &str) -> Self {
        Self {
            schema: schema.to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct RepoCatalog {
    starsync: DocumentSchema,
    kind: String,
    generated_at: DateTime<Utc>,
    counts: CatalogCounts,
    items: Vec<RepoCatalogItem>,
}

#[derive(Clone, Debug, Serialize)]
struct RepoCatalogItem {
    owner: String,
    name: String,
    full_name: String,
    path: String,
    html_url: Option<String>,
    description: Option<String>,
    language: Option<String>,
    topics: Vec<String>,
    stargazers_count: Option<u64>,
    default_branch: Option<String>,
    pushed_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    starred_at: Option<DateTime<Utc>>,
    current: bool,
    archived: bool,
    tags: Vec<String>,
    status: Option<String>,
    summary: Option<String>,
}

impl From<&RepoView> for RepoCatalogItem {
    fn from(repo: &RepoView) -> Self {
        Self {
            owner: repo.owner.clone(),
            name: repo.name.clone(),
            full_name: repo.full_name.clone(),
            path: format!("{}/{}/INDEX.md", repo.owner, repo.name),
            html_url: repo.html_url.clone(),
            description: repo.description.clone(),
            language: repo.language.clone(),
            topics: repo.topics.clone(),
            stargazers_count: repo.stargazers_count,
            default_branch: repo.default_branch.clone(),
            pushed_at: repo.pushed_at,
            updated_at: repo.updated_at,
            starred_at: repo.starred_at,
            current: repo.current,
            archived: repo.archived,
            tags: repo.user.tags.clone(),
            status: repo.user.status.clone(),
            summary: repo.user.summary.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct CatalogCounts {
    total: usize,
    current: usize,
    archived: usize,
}

impl CatalogCounts {
    fn from_items(items: &[RepoCatalogItem]) -> Self {
        Self {
            total: items.len(),
            current: items
                .iter()
                .filter(|item| item.current && !item.archived)
                .count(),
            archived: items
                .iter()
                .filter(|item| item.archived || !item.current)
                .count(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct RepoIndexFrontMatter {
    starsync: DocumentSchema,
    kind: String,
    generated_at: DateTime<Utc>,
    counts: CatalogCounts,
    catalog: CatalogFiles,
    indexes: IndexFiles,
}

#[derive(Clone, Debug, Serialize)]
struct CatalogFiles {
    yaml: String,
    json: String,
}

#[derive(Clone, Debug, Serialize)]
struct IndexFiles {
    by_repo_initial: String,
    by_owner_initial: String,
}

#[derive(Clone, Debug, Serialize)]
struct InitialIndexFrontMatter {
    starsync: DocumentSchema,
    kind: String,
    generated_at: DateTime<Utc>,
    axis: String,
    counts: CatalogCounts,
    source_catalog: String,
}

pub fn read_meta_document(path: &Path) -> Result<RepoMetaDocument> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let (front_matter, body) = split_front_matter(&raw)
        .with_context(|| format!("missing YAML front matter in {}", path.display()))?;
    let meta: RepoMeta = serde_yaml::from_str(front_matter)
        .with_context(|| format!("invalid YAML front matter in {}", path.display()))?;
    Ok(RepoMetaDocument {
        meta,
        body: body.to_string(),
    })
}

pub fn write_meta_document(path: &Path, document: &RepoMetaDocument) -> Result<()> {
    let front_matter = serde_yaml::to_string(&document.meta)?;
    let body = document.body.trim_start_matches('\n');
    fs::write(path, format!("---\n{front_matter}---\n{body}"))
        .with_context(|| format!("failed to write {}", path.display()))
}

fn write_markdown_with_front_matter<T: Serialize>(
    path: &Path,
    front_matter: &T,
    body: &str,
) -> Result<()> {
    let front_matter = serde_yaml::to_string(front_matter)?;
    let body = body.trim_start_matches('\n');
    fs::write(path, format!("---\n{front_matter}---\n{body}"))
        .with_context(|| format!("failed to write {}", path.display()))
}

fn render_catalog_table(items: &[RepoCatalogItem]) -> String {
    let mut table = String::new();
    table.push_str("| Repo | Owner | Language | Stars | Tags | Status | Current | Summary |\n");
    table.push_str("| --- | --- | --- | ---: | --- | --- | --- | --- |\n");
    for item in items {
        table.push_str(&format!(
            "| [{}]({}) | {} | {} | {} | {} | {} | {} | {} |\n",
            markdown_cell(&item.full_name),
            item.path,
            markdown_cell(&item.owner),
            markdown_cell(item.language.as_deref().unwrap_or("")),
            item.stargazers_count
                .map(|value| value.to_string())
                .unwrap_or_default(),
            markdown_cell(&item.tags.join(", ")),
            markdown_cell(item.status.as_deref().unwrap_or("")),
            if item.current && !item.archived {
                "yes"
            } else {
                "no"
            },
            markdown_cell(item.summary.as_deref().unwrap_or(""))
        ));
    }
    if items.is_empty() {
        table.push_str("| _No repositories indexed yet._ |  |  |  |  |  |  |  |\n");
    }
    table
}

fn render_initial_index<F, S>(
    title: &str,
    axis_label: &str,
    items: &[RepoCatalogItem],
    key: F,
    sort_entries: S,
) -> String
where
    F: Fn(&RepoCatalogItem) -> &str,
    S: Fn(&mut Vec<&RepoCatalogItem>),
{
    let mut groups: BTreeMap<String, Vec<&RepoCatalogItem>> = BTreeMap::new();
    for item in items {
        groups
            .entry(initial_group(key(item)))
            .or_default()
            .push(item);
    }

    let mut body = String::new();
    body.push_str(&format!("# {title}\n\n"));
    body.push_str(&format!("Grouped by {axis_label} initial.\n\n"));
    if groups.is_empty() {
        body.push_str("_No repositories indexed yet._\n");
        return body;
    }

    for (initial, mut entries) in groups {
        sort_entries(&mut entries);
        body.push_str(&format!("## {initial}\n\n"));
        for item in entries {
            let details = compact_details(item);
            body.push_str(&format!(
                "- [{}]({}){}{}\n",
                markdown_inline(&item.full_name),
                item.path,
                if item.current && !item.archived {
                    ""
                } else {
                    " _(archived)_"
                },
                details
            ));
        }
        body.push('\n');
    }
    body
}

fn sort_by_repo_name(entries: &mut Vec<&RepoCatalogItem>) {
    entries.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.owner.to_lowercase().cmp(&b.owner.to_lowercase()))
    });
}

fn sort_by_owner_name(entries: &mut Vec<&RepoCatalogItem>) {
    entries.sort_by(|a, b| {
        a.owner
            .to_lowercase()
            .cmp(&b.owner.to_lowercase())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

fn initial_group(value: &str) -> String {
    value
        .chars()
        .find(|ch| ch.is_alphanumeric())
        .map(|ch| ch.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "#".to_string())
}

fn compact_details(item: &RepoCatalogItem) -> String {
    let mut details = Vec::new();
    if let Some(language) = item.language.as_deref().filter(|value| !value.is_empty()) {
        details.push(language.to_string());
    }
    if !item.tags.is_empty() {
        details.push(format!("tags: {}", item.tags.join(", ")));
    }
    if let Some(summary) = item.summary.as_deref().filter(|value| !value.is_empty()) {
        details.push(summary.to_string());
    }
    if details.is_empty() {
        String::new()
    } else {
        format!(" - {}", markdown_inline(&details.join(" | ")))
    }
}

fn markdown_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn markdown_inline(value: &str) -> String {
    value.replace('\n', " ")
}

pub fn split_front_matter(raw: &str) -> Result<(&str, &str)> {
    let normalized = raw
        .strip_prefix("---\n")
        .ok_or_else(|| anyhow!("no opening marker"))?;
    let marker = "\n---\n";
    let end = normalized
        .find(marker)
        .ok_or_else(|| anyhow!("no closing marker"))?;
    let yaml = &normalized[..end];
    let body = &normalized[end + marker.len()..];
    Ok((yaml, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{RepoIdentity, RepoView, UserMeta};

    #[test]
    fn round_trips_repo_meta_front_matter_and_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("INDEX.md");
        let document = RepoMetaDocument {
            meta: RepoMeta {
                user: UserMeta {
                    tags: vec!["rust".to_string()],
                    summary: Some("A useful crate".to_string()),
                    ..UserMeta::default()
                },
                ..RepoMeta::new(RepoIdentity::new("owner", "repo"))
            },
            body: "# owner/repo\n\nNotes stay here.\n".to_string(),
        };

        write_meta_document(&path, &document).unwrap();
        let loaded = read_meta_document(&path).unwrap();

        assert_eq!(loaded.meta.user.tags, vec!["rust"]);
        assert!(loaded.body.contains("Notes stay here."));
    }

    #[test]
    fn store_lists_only_per_repo_index_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path().join("repos"), dir.path().join("state"));
        store.init().unwrap();
        store
            .ensure_meta(&RepoIdentity::new("alice", "demo"))
            .unwrap();

        let docs = store.list_meta().unwrap();

        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].meta.repo.full_name(), "alice/demo");
    }

    #[test]
    fn writes_root_catalog_and_initial_indexes() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path().join("repos"), dir.path().join("state"));
        let repo = RepoView {
            owner: "alice".to_string(),
            name: "Toolbox".to_string(),
            full_name: "alice/Toolbox".to_string(),
            html_url: Some("https://github.com/alice/Toolbox".to_string()),
            language: Some("Rust".to_string()),
            stargazers_count: Some(42),
            current: true,
            user: UserMeta {
                tags: vec!["cli".to_string()],
                summary: Some("Terminal utilities".to_string()),
                ..UserMeta::default()
            },
            ..RepoView::default()
        };

        store.write_catalog(&[repo]).unwrap();

        let index = fs::read_to_string(store.index_path()).unwrap();
        assert!(index.contains("kind: repo_index"));
        assert!(index.contains("[catalog.yaml](catalog.yaml)"));
        assert!(index.contains("[alice/Toolbox](alice/Toolbox/INDEX.md)"));

        let catalog: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(store.catalog_json_path()).unwrap()).unwrap();
        assert_eq!(catalog["counts"]["total"], 1);
        assert_eq!(catalog["items"][0]["full_name"], "alice/Toolbox");
        assert_eq!(catalog["items"][0]["tags"][0], "cli");

        let by_repo = fs::read_to_string(store.by_repo_index_path()).unwrap();
        assert!(by_repo.contains("## T"));
        assert!(by_repo.contains("[alice/Toolbox](alice/Toolbox/INDEX.md)"));

        let by_owner = fs::read_to_string(store.by_owner_index_path()).unwrap();
        assert!(by_owner.contains("## A"));
        assert!(by_owner.contains("[alice/Toolbox](alice/Toolbox/INDEX.md)"));
    }
}
