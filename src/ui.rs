use crate::config::Config;
use anyhow::{bail, Context, Result};
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
    pub version: &'static str,
}

pub fn prepare_ui(config: &Config) -> Result<UiBundleStatus> {
    let index = config.ui_dir.join("index.html");
    if index.exists() {
        return Ok(UiBundleStatus {
            dir: config.ui_dir.clone(),
            extracted: false,
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
        version: UI_VERSION,
    })
}

fn extract_ui_assets(root: &Path) -> Result<()> {
    fs::create_dir_all(root)
        .with_context(|| format!("failed to create UI directory {}", root.display()))?;
    for asset in UI_ASSETS {
        write_asset(root, asset)?;
    }
    fs::write(root.join(".starsync-ui-version"), UI_VERSION)
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
        assert!(config.ui_dir.join("index.html").exists());
        assert!(config.ui_dir.join("assets").join("app.js").exists());
    }

    #[test]
    fn keeps_existing_ui_directory_when_index_exists() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::defaults();
        config.ui_dir = dir.path().join("ui");
        fs::create_dir_all(&config.ui_dir).unwrap();
        fs::write(config.ui_dir.join("index.html"), "custom").unwrap();

        let status = prepare_ui(&config).unwrap();

        assert!(!status.extracted);
        assert_eq!(
            fs::read_to_string(config.ui_dir.join("index.html")).unwrap(),
            "custom"
        );
    }
}
