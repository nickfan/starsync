use crate::config::{Config, StorageBackendKind};
use anyhow::{anyhow, Context, Result};
use std::path::Path;
use tokio::process::Command;

pub async fn pull(config: &Config) -> Result<String> {
    match config.storage_backend {
        StorageBackendKind::Local => Ok("local storage: nothing to pull".to_string()),
        StorageBackendKind::Git => {
            ensure_git_repo(config).await?;
            run_git(&config.data_dir, &["pull", "--ff-only"]).await
        }
    }
}

pub async fn push(config: &Config) -> Result<String> {
    match config.storage_backend {
        StorageBackendKind::Local => Ok("local storage: nothing to push".to_string()),
        StorageBackendKind::Git => {
            ensure_git_repo(config).await?;
            run_git(&config.data_dir, &["add", "repos"]).await?;
            let status = run_git(&config.data_dir, &["status", "--porcelain"]).await?;
            if status.trim().is_empty() {
                return Ok("git storage: no metadata changes to push".to_string());
            }
            run_git(
                &config.data_dir,
                &["commit", "-m", "chore: sync starsync metadata"],
            )
            .await?;
            run_git(&config.data_dir, &["push"]).await
        }
    }
}

async fn ensure_git_repo(config: &Config) -> Result<()> {
    tokio::fs::create_dir_all(&config.data_dir).await?;
    if !config.data_dir.join(".git").exists() {
        run_git(&config.data_dir, &["init"]).await?;
    }
    if let Some(remote) = &config.git_remote {
        let existing = run_git(&config.data_dir, &["remote"]).await?;
        if !existing.lines().any(|line| line == "origin") {
            run_git(&config.data_dir, &["remote", "add", "origin", remote]).await?;
        }
    }
    Ok(())
}

async fn run_git(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
