use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use directories::BaseDirs;
use serde::Deserialize;
use thiserror::Error;

const CONFIG_TEMPLATE: &str = "[tmdb]\napi_token = \"\"\nlanguage = \"zh-CN\"\n\n[screenshot]\ncount = 3\ntimestamps = []\nsubtitles = true\nsubtitle_languages = [\"zh-CN\", \"zh\", \"chs\", \"chi\"]\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    pub portable: PathBuf,
    pub system: PathBuf,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SecretToken(String);

impl SecretToken {
    // Token 只允许在构造认证请求时显式取出，避免被普通格式化路径意外泄露。
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(crate) fn expose_for_request(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretToken([REDACTED])")
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub tmdb: TmdbConfig,
    pub screenshot: ScreenshotConfig,
}

#[derive(Debug, Clone)]
pub struct TmdbConfig {
    pub api_token: SecretToken,
    pub language: String,
}

impl TmdbConfig {
    pub(crate) fn require_token(self, path: &Path) -> Result<Self, ConfigError> {
        if self.api_token.expose_for_request().is_empty() {
            return Err(ConfigError::EmptyToken {
                path: path.to_path_buf(),
            });
        }
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub struct ScreenshotConfig {
    pub count: usize,
    pub timestamps: Vec<String>,
    pub subtitles: bool,
    pub subtitle_languages: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub path: PathBuf,
    pub value: AppConfig,
}

#[derive(Debug, Deserialize)]
struct RawAppConfig {
    tmdb: RawTmdbConfig,
    #[serde(default)]
    screenshot: RawScreenshotConfig,
}

#[derive(Debug, Deserialize)]
struct RawTmdbConfig {
    api_token: String,
    #[serde(default = "default_language")]
    language: String,
}

#[derive(Debug, Deserialize)]
struct RawScreenshotConfig {
    #[serde(default = "default_screenshot_count")]
    count: usize,
    #[serde(default)]
    timestamps: Vec<String>,
    #[serde(default = "default_subtitles")]
    subtitles: bool,
    #[serde(default = "default_subtitle_languages")]
    subtitle_languages: Vec<String>,
}

impl Default for RawScreenshotConfig {
    fn default() -> Self {
        Self {
            count: default_screenshot_count(),
            timestamps: Vec::new(),
            subtitles: default_subtitles(),
            subtitle_languages: default_subtitle_languages(),
        }
    }
}

fn default_language() -> String {
    "zh-CN".to_owned()
}

fn default_screenshot_count() -> usize {
    3
}

fn default_subtitles() -> bool {
    true
}

fn default_subtitle_languages() -> Vec<String> {
    ["zh-CN", "zh", "chs", "chi"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot determine the executable directory from {0}")]
    ExecutableDirectory(PathBuf),
    #[error("cannot determine the operating system configuration directory")]
    SystemDirectory,
    #[error(
        "no configuration file found; checked {portable} and {system}; run `crabgrab config init`"
    )]
    NotFound { portable: PathBuf, system: PathBuf },
    #[error("cannot read configuration {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot parse configuration {path}; check the TOML syntax")]
    Parse { path: PathBuf },
    #[error("configuration {path} requires a non-empty [tmdb].api_token")]
    EmptyToken { path: PathBuf },
    #[error("configuration {path} requires screenshot.count between 1 and 100")]
    ScreenshotCount { path: PathBuf },
    #[error("configuration {path} requires at least one [screenshot].subtitle_languages entry")]
    SubtitleLanguages { path: PathBuf },
    #[error("configuration already exists at {0}; refusing to create another file")]
    AlreadyExists(PathBuf),
    #[error(
        "cannot initialize configuration at {portable} or {system}: portable: {portable_error}; system: {system_error}"
    )]
    InitFailed {
        portable: PathBuf,
        system: PathBuf,
        portable_error: String,
        system_error: String,
    },
}

/// 计算便携配置和系统配置的候选路径，返回顺序同时定义读取优先级。
pub fn resolve_config_paths(executable: &Path) -> Result<ConfigPaths, ConfigError> {
    // 便携配置与可执行文件放在一起；系统配置作为不可写安装目录的后备位置。
    let portable_parent = executable
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| ConfigError::ExecutableDirectory(executable.to_owned()))?;
    let base_dirs = BaseDirs::new().ok_or(ConfigError::SystemDirectory)?;

    Ok(ConfigPaths {
        portable: portable_parent.join("config.toml"),
        system: base_dirs.config_dir().join("crabgrab").join("config.toml"),
    })
}

/// 按优先级读取并验证配置；便携配置存在但无效时不会静默回退。
pub fn load_config(paths: &ConfigPaths) -> Result<LoadedConfig, ConfigError> {
    // try_exists 会保留权限等元数据错误，不能像 Path::exists 那样静默回退。
    let portable_exists = paths
        .portable
        .try_exists()
        .map_err(|source| ConfigError::Read {
            path: paths.portable.clone(),
            source,
        })?;
    let system_exists = paths
        .system
        .try_exists()
        .map_err(|source| ConfigError::Read {
            path: paths.system.clone(),
            source,
        })?;
    let path = if portable_exists {
        paths.portable.clone()
    } else if system_exists {
        paths.system.clone()
    } else {
        return Err(ConfigError::NotFound {
            portable: paths.portable.clone(),
            system: paths.system.clone(),
        });
    };

    let contents = fs::read_to_string(&path).map_err(|source| ConfigError::Read {
        path: path.clone(),
        source,
    })?;
    let raw: RawAppConfig =
        // 原始 TOML 错误可能包含 Token 所在行，对外只保留固定说明和文件路径。
        toml::from_str(&contents).map_err(|_| ConfigError::Parse { path: path.clone() })?;
    if !(1..=100).contains(&raw.screenshot.count) {
        return Err(ConfigError::ScreenshotCount { path });
    }
    if raw.screenshot.subtitle_languages.is_empty()
        || raw
            .screenshot
            .subtitle_languages
            .iter()
            .any(|language| language.trim().is_empty())
    {
        return Err(ConfigError::SubtitleLanguages { path });
    }

    let token = raw.tmdb.api_token.trim();
    Ok(LoadedConfig {
        path,
        value: AppConfig {
            tmdb: TmdbConfig {
                api_token: SecretToken::new(token),
                language: raw.tmdb.language,
            },
            screenshot: ScreenshotConfig {
                count: raw.screenshot.count,
                timestamps: raw.screenshot.timestamps,
                subtitles: raw.screenshot.subtitles,
                subtitle_languages: raw
                    .screenshot
                    .subtitle_languages
                    .into_iter()
                    .map(|language| language.trim().to_owned())
                    .collect(),
            },
        },
    })
}

/// 创建空白配置模板；使用 create_new 保证并发情况下也绝不覆盖已有文件。
pub fn init_config(paths: &ConfigPaths) -> Result<PathBuf, ConfigError> {
    // 初始化前同时检查两个位置，防止用户在不知情时维护两份配置。
    match paths.portable.try_exists() {
        Ok(true) => return Err(ConfigError::AlreadyExists(paths.portable.clone())),
        Ok(false) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotADirectory => {}
        Err(source) => {
            return Err(ConfigError::Read {
                path: paths.portable.clone(),
                source,
            });
        }
    }
    if paths
        .system
        .try_exists()
        .map_err(|source| ConfigError::Read {
            path: paths.system.clone(),
            source,
        })?
    {
        return Err(ConfigError::AlreadyExists(paths.system.clone()));
    }

    match create_config(&paths.portable) {
        Ok(()) => Ok(paths.portable.clone()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(ConfigError::AlreadyExists(paths.portable.clone()))
        }
        Err(portable_error)
            if !portable_path_allows_system_fallback(&paths.portable, &portable_error) =>
        {
            Err(ConfigError::InitFailed {
                portable: paths.portable.clone(),
                system: paths.system.clone(),
                portable_error: portable_error.to_string(),
                system_error: "system fallback was not attempted for this error".to_owned(),
            })
        }
        Err(portable_error) => {
            // 只有明确的目录不可写场景，才允许回退到系统配置目录。
            if let Some(parent) = paths.system.parent()
                && let Err(source) = fs::create_dir_all(parent)
            {
                return Err(ConfigError::InitFailed {
                    portable: paths.portable.clone(),
                    system: paths.system.clone(),
                    portable_error: portable_error.to_string(),
                    system_error: source.to_string(),
                });
            }
            create_config(&paths.system).map_err(|system_error| {
                if system_error.kind() == std::io::ErrorKind::AlreadyExists {
                    ConfigError::AlreadyExists(paths.system.clone())
                } else {
                    ConfigError::InitFailed {
                        portable: paths.portable.clone(),
                        system: paths.system.clone(),
                        portable_error: portable_error.to_string(),
                        system_error: system_error.to_string(),
                    }
                }
            })?;
            Ok(paths.system.clone())
        }
    }
}

fn portable_path_allows_system_fallback(path: &Path, error: &std::io::Error) -> bool {
    if matches!(
        error.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotADirectory
    ) {
        return true;
    }

    // Windows reports NotFound instead of NotADirectory when a parent component is a file.
    path.parent().is_some_and(|parent| parent.is_file())
}

fn create_config(path: &Path) -> Result<(), std::io::Error> {
    // create_new 提供并发安全的“不覆盖”语义；写入失败时只清理本次创建的文件。
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let result = file
        .write_all(CONFIG_TEMPLATE.as_bytes())
        .and_then(|_| file.sync_all());
    drop(file);
    if let Err(error) = result {
        // 写入失败时清理本次创建的半成品，并聚合可能发生的清理错误。
        return match fs::remove_file(path) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(std::io::Error::other(format!(
                "{error}; additionally could not remove partial {}: {cleanup}",
                path.display()
            ))),
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{CONFIG_TEMPLATE, ConfigPaths, init_config, load_config};

    fn paths() -> (tempfile::TempDir, ConfigPaths) {
        let root = tempdir().unwrap();
        let paths = ConfigPaths {
            portable: root.path().join("portable/config.toml"),
            system: root.path().join("system/crabgrab/config.toml"),
        };
        (root, paths)
    }

    fn write(path: &std::path::Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn load_prefers_portable_configuration() {
        let (_root, paths) = paths();
        write(
            &paths.portable,
            "[tmdb]\napi_token='portable'\nlanguage='en-US'\n",
        );
        write(&paths.system, "[tmdb]\napi_token='system'\n");

        let loaded = load_config(&paths).unwrap();

        assert_eq!(loaded.path, paths.portable);
        assert_eq!(loaded.value.tmdb.api_token.expose_for_request(), "portable");
        assert_eq!(loaded.value.tmdb.language, "en-US");
    }

    #[test]
    fn load_falls_back_to_system_and_defaults_language() {
        let (_root, paths) = paths();
        write(&paths.system, "[tmdb]\napi_token='system'\n");

        let loaded = load_config(&paths).unwrap();

        assert_eq!(loaded.path, paths.system);
        assert_eq!(loaded.value.tmdb.language, "zh-CN");
    }

    #[test]
    fn invalid_portable_configuration_does_not_fall_back() {
        let (_root, paths) = paths();
        write(&paths.portable, "not toml");
        write(&paths.system, "[tmdb]\napi_token='system'\n");

        let error = load_config(&paths).unwrap_err().to_string();

        assert!(error.contains(paths.portable.to_string_lossy().as_ref()));
    }

    #[test]
    fn malformed_token_line_never_appears_in_parse_error() {
        let (_root, paths) = paths();
        write(
            &paths.portable,
            "[tmdb]\napi_token=\"super-secret-unclosed\n",
        );

        let error = load_config(&paths).unwrap_err();
        let visible = format!("{error:?} {error}");

        assert!(!visible.contains("super-secret-unclosed"));
        assert!(visible.contains(paths.portable.to_string_lossy().as_ref()));
    }

    #[test]
    fn blank_token_loads_for_non_tmdb_commands_with_screenshot_defaults() {
        let (_root, paths) = paths();
        write(&paths.portable, "[tmdb]\napi_token='  '\n");

        let loaded = load_config(&paths).unwrap();

        assert_eq!(loaded.value.tmdb.api_token.expose_for_request(), "");
        assert_eq!(loaded.value.screenshot.count, 3);
        assert!(loaded.value.screenshot.timestamps.is_empty());
        assert!(loaded.value.screenshot.subtitles);
        assert_eq!(
            loaded.value.screenshot.subtitle_languages,
            ["zh-CN", "zh", "chs", "chi"]
        );
    }

    #[test]
    fn loads_explicit_screenshot_configuration() {
        let (_root, paths) = paths();
        write(
            &paths.portable,
            "[tmdb]\napi_token=''\n[screenshot]\ncount=5\ntimestamps=['00:10:00','65%']\nsubtitles=false\nsubtitle_languages=['en-US','en']\n",
        );

        let loaded = load_config(&paths).unwrap();

        assert_eq!(loaded.value.screenshot.count, 5);
        assert_eq!(loaded.value.screenshot.timestamps, ["00:10:00", "65%"]);
        assert!(!loaded.value.screenshot.subtitles);
        assert_eq!(loaded.value.screenshot.subtitle_languages, ["en-US", "en"]);
    }

    #[test]
    fn rejects_screenshot_count_outside_one_to_one_hundred() {
        for count in [0, 101] {
            let (_root, paths) = paths();
            write(
                &paths.portable,
                &format!("[tmdb]\napi_token=''\n[screenshot]\ncount={count}\n"),
            );

            let error = load_config(&paths).unwrap_err().to_string();

            assert!(error.contains("screenshot.count"));
        }
    }

    #[test]
    fn rejects_empty_subtitle_language_priority() {
        let (_root, paths) = paths();
        write(
            &paths.portable,
            "[tmdb]\napi_token=''\n[screenshot]\nsubtitle_languages=[]\n",
        );

        let error = load_config(&paths).unwrap_err().to_string();

        assert!(error.contains("subtitle_languages"));
    }

    #[test]
    fn init_creates_template_and_refuses_a_second_configuration() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.portable.parent().unwrap()).unwrap();

        let created = init_config(&paths).unwrap();
        let original = fs::read_to_string(&created).unwrap();

        assert_eq!(created, paths.portable);
        assert_eq!(original, CONFIG_TEMPLATE);
        assert!(init_config(&paths).is_err());
        assert_eq!(fs::read_to_string(created).unwrap(), original);
    }

    #[test]
    fn init_falls_back_to_system_when_portable_parent_is_a_file() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.portable.parent().unwrap().parent().unwrap()).unwrap();
        fs::write(paths.portable.parent().unwrap(), "not a directory").unwrap();

        let created = init_config(&paths).unwrap();

        assert_eq!(created, paths.system);
        assert!(created.exists());
    }
}
