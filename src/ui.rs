use crate::config::Config;
use anyhow::{bail, Context, Result};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

const UI_VERSION: &str = env!("CARGO_PKG_VERSION");

struct UiAsset {
    path: &'static str,
    bytes: &'static [u8],
}

static UI_ASSETS: &[UiAsset] = &[
    UiAsset {
        path: "index.html",
        bytes: include_bytes!("../ui/dist/index.html"),
    },
    UiAsset {
        path: "assets/app.js",
        bytes: include_bytes!("../ui/dist/assets/app.js"),
    },
    UiAsset {
        path: "assets/styles.css",
        bytes: include_bytes!("../ui/dist/assets/styles.css"),
    },
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiBundleStatus {
    pub dir: PathBuf,
    pub extracted: bool,
    pub overwritten: bool,
    pub backup_dir: Option<PathBuf>,
    pub version: &'static str,
}

pub fn prepare_ui(config: &Config) -> Result<UiBundleStatus> {
    let index = config.ui_dir.join("index.html");
    if index.exists() {
        if ui_marker_matches(&config.ui_dir)? {
            return Ok(UiBundleStatus {
                dir: config.ui_dir.clone(),
                extracted: false,
                overwritten: false,
                backup_dir: None,
                version: UI_VERSION,
            });
        }

        if !config.ui_overwrite {
            return Ok(UiBundleStatus {
                dir: config.ui_dir.clone(),
                extracted: false,
                overwritten: false,
                backup_dir: None,
                version: UI_VERSION,
            });
        }

        let backup_dir = if config.ui_backup {
            Some(backup_ui_dir(&config.ui_dir)?)
        } else {
            clear_ui_dir(&config.ui_dir)?;
            None
        };
        extract_ui_assets(&config.ui_dir)?;
        return Ok(UiBundleStatus {
            dir: config.ui_dir.clone(),
            extracted: true,
            overwritten: true,
            backup_dir,
            version: UI_VERSION,
        });
    }

    if !config.ui_auto_extract {
        bail!(
            "StarSync UI index.html is missing at {} and auto extraction is disabled",
            index.display()
        );
    }

    extract_ui_assets(&config.ui_dir)?;
    Ok(UiBundleStatus {
        dir: config.ui_dir.clone(),
        extracted: true,
        overwritten: false,
        backup_dir: None,
        version: UI_VERSION,
    })
}

fn extract_ui_assets(root: &Path) -> Result<()> {
    fs::create_dir_all(root)
        .with_context(|| format!("failed to create UI directory {}", root.display()))?;
    for asset in UI_ASSETS {
        write_asset(root, asset)?;
    }
    fs::write(root.join(".starsync-ui-version"), ui_marker())
        .with_context(|| format!("failed to write UI version marker under {}", root.display()))?;
    Ok(())
}

fn write_asset(root: &Path, asset: &UiAsset) -> Result<()> {
    let target = root.join(asset.path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create UI asset directory {}", parent.display()))?;
    }
    fs::write(&target, asset.bytes)
        .with_context(|| format!("failed to write UI asset {}", target.display()))?;
    Ok(())
}

fn ui_marker_matches(root: &Path) -> Result<bool> {
    let marker = root.join(".starsync-ui-version");
    if !marker.exists() {
        return Ok(false);
    }
    let current = fs::read_to_string(&marker)
        .with_context(|| format!("failed to read UI version marker {}", marker.display()))?;
    Ok(current == ui_marker())
}

fn ui_marker() -> String {
    format!("version={UI_VERSION}\nfingerprint={}\n", ui_fingerprint())
}

fn ui_fingerprint() -> String {
    let mut hasher = Sha256::new();
    for asset in UI_ASSETS {
        hasher.update(asset.path.as_bytes());
        hasher.update([0]);
        hasher.update(asset.bytes);
        hasher.update([0]);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn backup_ui_dir(root: &Path) -> Result<PathBuf> {
    let parent = root.parent().unwrap_or_else(|| Path::new("."));
    let name = root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("ui");
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    for attempt in 0..100 {
        let suffix = if attempt == 0 {
            String::new()
        } else {
            format!("-{attempt}")
        };
        let backup = parent.join(format!("{name}.bak-{stamp}{suffix}"));
        if !backup.exists() {
            fs::rename(root, &backup).with_context(|| {
                format!(
                    "failed to backup UI directory {} to {}",
                    root.display(),
                    backup.display()
                )
            })?;
            return Ok(backup);
        }
    }
    bail!(
        "failed to find available UI backup path for {}",
        root.display()
    )
}

fn clear_ui_dir(root: &Path) -> Result<()> {
    if root.is_dir() {
        fs::remove_dir_all(root)
            .with_context(|| format!("failed to remove stale UI directory {}", root.display()))?;
    } else if root.exists() {
        fs::remove_file(root)
            .with_context(|| format!("failed to remove stale UI path {}", root.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_embedded_ui_when_index_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::defaults();
        config.ui_dir = dir.path().join("ui");

        let status = prepare_ui(&config).unwrap();

        assert!(status.extracted);
        assert!(!status.overwritten);
        assert!(status.backup_dir.is_none());
        assert!(config.ui_dir.join("index.html").exists());
        assert!(config.ui_dir.join("assets").join("app.js").exists());
    }

    #[test]
    fn keeps_current_ui_directory_when_marker_matches() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::defaults();
        config.ui_dir = dir.path().join("ui");
        fs::create_dir_all(&config.ui_dir).unwrap();
        fs::write(config.ui_dir.join("index.html"), "current").unwrap();
        fs::write(config.ui_dir.join(".starsync-ui-version"), ui_marker()).unwrap();

        let status = prepare_ui(&config).unwrap();

        assert!(!status.extracted);
        assert!(!status.overwritten);
        assert_eq!(
            fs::read_to_string(config.ui_dir.join("index.html")).unwrap(),
            "current"
        );
    }

    #[test]
    fn overwrites_stale_ui_and_backs_it_up_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::defaults();
        config.ui_dir = dir.path().join("ui");
        fs::create_dir_all(&config.ui_dir).unwrap();
        fs::write(config.ui_dir.join("index.html"), "custom").unwrap();
        fs::write(config.ui_dir.join(".starsync-ui-version"), "old").unwrap();

        let status = prepare_ui(&config).unwrap();

        assert!(status.extracted);
        assert!(status.overwritten);
        let backup = status.backup_dir.unwrap();
        assert!(backup.exists());
        assert_eq!(
            fs::read_to_string(backup.join("index.html")).unwrap(),
            "custom"
        );
        assert_ne!(
            fs::read_to_string(config.ui_dir.join("index.html")).unwrap(),
            "custom"
        );
    }

    #[test]
    fn keeps_stale_ui_when_overwrite_is_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::defaults();
        config.ui_overwrite = false;
        config.ui_dir = dir.path().join("ui");
        fs::create_dir_all(&config.ui_dir).unwrap();
        fs::write(config.ui_dir.join("index.html"), "custom").unwrap();

        let status = prepare_ui(&config).unwrap();

        assert!(!status.extracted);
        assert!(!status.overwritten);
        assert!(status.backup_dir.is_none());
        assert_eq!(
            fs::read_to_string(config.ui_dir.join("index.html")).unwrap(),
            "custom"
        );
    }

    #[test]
    fn overwrites_stale_ui_without_backup_when_backup_is_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::defaults();
        config.ui_backup = false;
        config.ui_dir = dir.path().join("ui");
        fs::create_dir_all(&config.ui_dir).unwrap();
        fs::write(config.ui_dir.join("index.html"), "custom").unwrap();

        let status = prepare_ui(&config).unwrap();

        assert!(status.extracted);
        assert!(status.overwritten);
        assert!(status.backup_dir.is_none());
        assert!(!dir.path().read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".bak-")));
    }
}
