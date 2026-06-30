use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub bind: String,
    pub github_token: Option<String>,
    pub storage_backend: StorageBackendKind,
    pub git_remote: Option<String>,
    pub sqlite_enabled: bool,
}

impl Config {
    pub fn load(overrides: ConfigOverrides) -> Result<Self> {
        let dotenv = read_dotenv_files(overrides.env_file.as_deref())?;
        let process_env: BTreeMap<String, String> = env::vars().collect();
        let mut interpolation_env = dotenv.clone();
        interpolation_env.extend(process_env.clone());

        let config_path = overrides
            .config_path
            .clone()
            .or_else(|| process_env.get("STARSYNC_CONFIG").map(PathBuf::from))
            .or_else(|| dotenv.get("STARSYNC_CONFIG").map(PathBuf::from))
            .or_else(default_config_file_if_present);

        let file_config = if let Some(path) = config_path {
            read_file_config(&path, &interpolation_env)?
        } else {
            FileConfig::default()
        };

        resolve_config(
            Config::defaults(),
            file_config,
            &dotenv,
            &process_env,
            overrides,
        )
    }

    pub fn defaults() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            data_dir: home.join(".starsync").join("data"),
            state_dir: home.join(".starsync").join("state"),
            bind: "127.0.0.1:8989".to_string(),
            github_token: None,
            storage_backend: StorageBackendKind::Local,
            git_remote: None,
            sqlite_enabled: true,
        }
    }

    pub fn repos_dir(&self) -> PathBuf {
        self.data_dir.join("repos")
    }

    pub fn mirror_file(&self) -> PathBuf {
        self.state_dir.join("mirror.json")
    }

    pub fn sqlite_file(&self) -> PathBuf {
        self.state_dir.join("starsync.db")
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigOverrides {
    pub config_path: Option<PathBuf>,
    pub env_file: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    pub state_dir: Option<PathBuf>,
    pub bind: Option<String>,
    pub github_token: Option<String>,
    pub storage_backend: Option<StorageBackendKind>,
    pub git_remote: Option<String>,
    pub sqlite_enabled: Option<bool>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum StorageBackendKind {
    #[default]
    Local,
    Git,
}

impl StorageBackendKind {
    pub fn parse(input: &str) -> Result<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "git" => Ok(Self::Git),
            other => Err(anyhow!("unsupported storage backend: {other}")),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct FileConfig {
    data_dir: Option<String>,
    state_dir: Option<String>,
    bind: Option<String>,
    sqlite_enabled: Option<bool>,
    github: Option<FileGithubConfig>,
    storage: Option<FileStorageConfig>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct FileGithubConfig {
    token: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct FileStorageConfig {
    backend: Option<String>,
    git_remote: Option<String>,
}

fn resolve_config(
    mut config: Config,
    file: FileConfig,
    dotenv: &BTreeMap<String, String>,
    process_env: &BTreeMap<String, String>,
    overrides: ConfigOverrides,
) -> Result<Config> {
    apply_file_config(&mut config, file)?;
    apply_env_map(&mut config, dotenv)?;
    apply_env_map(&mut config, process_env)?;
    apply_overrides(&mut config, overrides);
    Ok(config)
}

fn apply_file_config(config: &mut Config, file: FileConfig) -> Result<()> {
    if let Some(data_dir) = file.data_dir {
        config.data_dir = expand_path(&data_dir);
    }
    if let Some(state_dir) = file.state_dir {
        config.state_dir = expand_path(&state_dir);
    }
    if let Some(bind) = file.bind {
        config.bind = bind;
    }
    if let Some(sqlite_enabled) = file.sqlite_enabled {
        config.sqlite_enabled = sqlite_enabled;
    }
    if let Some(github) = file.github {
        if let Some(token) = github.token.filter(|value| !value.is_empty()) {
            config.github_token = Some(token);
        }
    }
    if let Some(storage) = file.storage {
        if let Some(backend) = storage.backend {
            config.storage_backend = StorageBackendKind::parse(&backend)?;
        }
        if let Some(git_remote) = storage.git_remote.filter(|value| !value.is_empty()) {
            config.git_remote = Some(git_remote);
        }
    }
    Ok(())
}

fn apply_env_map(config: &mut Config, env_map: &BTreeMap<String, String>) -> Result<()> {
    if let Some(value) = env_map.get("STARSYNC_DATA_DIR") {
        config.data_dir = expand_path(value);
    }
    if let Some(value) = env_map.get("STARSYNC_STATE_DIR") {
        config.state_dir = expand_path(value);
    }
    if let Some(value) = env_map.get("STARSYNC_BIND") {
        config.bind = value.clone();
    }
    if let Some(value) = env_map.get("STARSYNC_GITHUB_TOKEN") {
        if !value.is_empty() {
            config.github_token = Some(value.clone());
        }
    }
    if let Some(value) = env_map.get("STARSYNC_STORAGE_BACKEND") {
        config.storage_backend = StorageBackendKind::parse(value)?;
    }
    if let Some(value) = env_map.get("STARSYNC_GIT_REMOTE") {
        if !value.is_empty() {
            config.git_remote = Some(value.clone());
        }
    }
    if let Some(value) = env_map.get("STARSYNC_SQLITE_ENABLED") {
        config.sqlite_enabled = parse_bool(value)?;
    }
    Ok(())
}

fn apply_overrides(config: &mut Config, overrides: ConfigOverrides) {
    if let Some(data_dir) = overrides.data_dir {
        config.data_dir = data_dir;
    }
    if let Some(state_dir) = overrides.state_dir {
        config.state_dir = state_dir;
    }
    if let Some(bind) = overrides.bind {
        config.bind = bind;
    }
    if let Some(token) = overrides.github_token {
        config.github_token = Some(token);
    }
    if let Some(storage_backend) = overrides.storage_backend {
        config.storage_backend = storage_backend;
    }
    if let Some(git_remote) = overrides.git_remote {
        config.git_remote = Some(git_remote);
    }
    if let Some(sqlite_enabled) = overrides.sqlite_enabled {
        config.sqlite_enabled = sqlite_enabled;
    }
}

fn read_file_config(
    path: &Path,
    interpolation_env: &BTreeMap<String, String>,
) -> Result<FileConfig> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    let expanded = interpolate_env(&raw, interpolation_env)?;
    toml::from_str(&expanded)
        .with_context(|| format!("failed to parse config file {}", path.display()))
}

fn read_dotenv_files(explicit: Option<&Path>) -> Result<BTreeMap<String, String>> {
    let paths = if let Some(path) = explicit {
        vec![path.to_path_buf()]
    } else {
        let mut paths = Vec::new();
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".config").join("starsync").join(".env"));
        }
        paths.push(PathBuf::from(".env"));
        paths
    };

    let mut map = BTreeMap::new();
    for path in paths {
        if !path.exists() {
            continue;
        }
        let iter = dotenvy::from_path_iter(&path)
            .with_context(|| format!("failed to read env file {}", path.display()))?;
        for item in iter {
            let (key, value) =
                item.with_context(|| format!("invalid env file {}", path.display()))?;
            map.insert(key, value);
        }
    }
    Ok(map)
}

fn default_config_file_if_present() -> Option<PathBuf> {
    let path = dirs::config_dir()?.join("starsync").join("config.toml");
    path.exists().then_some(path)
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => Err(anyhow!("invalid boolean value: {other}")),
    }
}

fn expand_path(value: &str) -> PathBuf {
    if let Some(stripped) = value.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    PathBuf::from(value)
}

fn interpolate_env(raw: &str, env_map: &BTreeMap<String, String>) -> Result<String> {
    let mut output = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '$' && chars.get(index + 1) == Some(&'{') {
            let mut end = index + 2;
            while end < chars.len() && chars[end] != '}' {
                end += 1;
            }
            if end == chars.len() {
                return Err(anyhow!("unterminated environment interpolation"));
            }
            let key: String = chars[index + 2..end].iter().collect();
            output.push_str(env_map.get(&key).map(String::as_str).unwrap_or(""));
            index = end + 1;
        } else {
            output.push(chars[index]);
            index += 1;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_config_from_env_map() {
        let mut env = BTreeMap::new();
        env.insert("STARSYNC_GITHUB_TOKEN".to_string(), "secret".to_string());
        let raw = r#"[github]
token = "${STARSYNC_GITHUB_TOKEN}"
"#;

        let expanded = interpolate_env(raw, &env).unwrap();

        assert!(expanded.contains("secret"));
    }

    #[test]
    fn env_overrides_config_and_cli_overrides_env() {
        let defaults = Config::defaults();
        let file = FileConfig {
            bind: Some("127.0.0.1:1".to_string()),
            ..FileConfig::default()
        };
        let dotenv = BTreeMap::new();
        let mut env = BTreeMap::new();
        env.insert("STARSYNC_BIND".to_string(), "127.0.0.1:2".to_string());
        let overrides = ConfigOverrides {
            bind: Some("127.0.0.1:3".to_string()),
            ..ConfigOverrides::default()
        };

        let config = resolve_config(defaults, file, &dotenv, &env, overrides).unwrap();

        assert_eq!(config.bind, "127.0.0.1:3");
    }

    #[test]
    fn dotenv_overrides_config_when_process_env_is_absent() {
        let defaults = Config::defaults();
        let file = FileConfig {
            sqlite_enabled: Some(false),
            ..FileConfig::default()
        };
        let mut dotenv = BTreeMap::new();
        dotenv.insert("STARSYNC_SQLITE_ENABLED".to_string(), "true".to_string());

        let config = resolve_config(
            defaults,
            file,
            &dotenv,
            &BTreeMap::new(),
            ConfigOverrides::default(),
        )
        .unwrap();

        assert!(config.sqlite_enabled);
    }
}
