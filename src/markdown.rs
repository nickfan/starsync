use crate::models::{MirrorState, RepoIdentity, RepoMeta};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use std::{
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

use serde::{Deserialize, Serialize};

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
            fs::write(
                &index,
                "# StarSync Index\n\nThis file anchors the local StarSync repository index.\n",
            )
            .with_context(|| format!("failed to write {}", index.display()))?;
        }
        Ok(())
    }

    pub fn repos_dir(&self) -> &Path {
        &self.repos_dir
    }

    pub fn index_path(&self) -> PathBuf {
        self.repos_dir.join("INDEX.md")
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
    use crate::models::{RepoIdentity, UserMeta};

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
}
