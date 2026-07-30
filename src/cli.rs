use std::path::PathBuf;
use std::time::Duration;

use clap::{ArgAction, Parser, Subcommand};
use reqwest::Url;
use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use thiserror::Error;

use crate::artwork::DownloadError;
use crate::artwork::{ReqwestBinaryFetcher, install_artwork};
use crate::config::{ConfigError, init_config, load_config, resolve_config_paths};
use crate::domain::{ResourceId, ResourceIdError};
use crate::media_info::{MediaAnalyzer, MediaInfoError, ProcessMediaAnalyzer, generate_report};
use crate::providers::{ProviderError, ProviderRegistry, TmdbProvider};

#[derive(Debug, Parser)]
#[command(
    name = "crabgrab",
    version,
    disable_version_flag = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[arg(short = 'v', long = "version", action = ArgAction::Version)]
    version: Option<bool>,

    #[arg(short = 'i', long, requires = "output", value_name = "RESOURCE_ID")]
    id: Option<String>,

    #[arg(short = 'o', long, requires = "id", value_name = "DIRECTORY")]
    output: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    #[command(name = "mediainfo")]
    MediaInfo {
        #[arg(short = 'i', long, value_name = "FILE")]
        input: PathBuf,
        #[arg(short = 'o', long, value_name = "DIRECTORY")]
        output: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Init,
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    ResourceId(#[from] ResourceIdError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error(transparent)]
    MediaInfo(#[from] MediaInfoError),
    #[error("cannot determine current executable: {0}")]
    Executable(std::io::Error),
    #[error("failed to create HTTP client: {0}")]
    HttpClient(String),
    #[error("download options --id/--output cannot be combined with a subcommand")]
    ConflictingActions,
}

/// 解析当前进程参数，并使用当前可执行文件位置启动应用。
pub fn run_default() -> Result<(), AppError> {
    let cli = Cli::parse();
    let executable = std::env::current_exe().map_err(AppError::Executable)?;
    run(cli, executable)
}

/// 使用指定可执行文件路径执行命令，便于隔离测试配置查找行为。
pub fn run(cli: Cli, executable: PathBuf) -> Result<(), AppError> {
    let analyzer = ProcessMediaAnalyzer::bundled(&executable);
    run_with_services(cli, executable, None, &analyzer)
}

#[doc(hidden)]
/// 执行 CLI；可选 API 基址仅供本地模拟测试注入。
pub fn run_with_api_base(
    cli: Cli,
    executable: PathBuf,
    tmdb_api_base: Option<Url>,
) -> Result<(), AppError> {
    let analyzer = ProcessMediaAnalyzer::bundled(&executable);
    run_with_services(cli, executable, tmdb_api_base, &analyzer)
}

#[doc(hidden)]
pub fn run_with_media_analyzer(
    cli: Cli,
    executable: PathBuf,
    analyzer: &impl MediaAnalyzer,
) -> Result<(), AppError> {
    run_with_services(cli, executable, None, analyzer)
}

fn run_with_services(
    cli: Cli,
    executable: PathBuf,
    tmdb_api_base: Option<Url>,
    analyzer: &impl MediaAnalyzer,
) -> Result<(), AppError> {
    // 测试可注入本地 API 地址；生产入口传入 None，始终使用 TMDB 官方地址。
    if cli.command.is_some() && (cli.id.is_some() || cli.output.is_some()) {
        // 子命令与下载参数是两类独立动作，禁止在一次调用中混用。
        return Err(AppError::ConflictingActions);
    }
    match cli.command {
        Some(Command::Config {
            command: ConfigCommand::Init,
        }) => {
            // 初始化只解析候选路径，不读取或验证现有 Token。
            let paths = resolve_config_paths(&executable)?;
            let path = init_config(&paths)?;
            println!(
                "created configuration at {}\nfill in [tmdb].api_token before downloading",
                path.display()
            );
            Ok(())
        }
        Some(Command::MediaInfo { input, output }) => {
            let installed = generate_report(analyzer, &input, &output)?;
            println!("mediainfo: {}", installed.display());
            Ok(())
        }
        None => {
            let id = cli.id.expect("clap requires id for download");
            let output = cli.output.expect("clap requires output for download");
            let resource_id = id.parse::<ResourceId>()?;
            // ID 必须先验证，非法输入不能触发配置读取或网络请求。
            let paths = resolve_config_paths(&executable)?;
            let loaded = load_config(&paths)?;
            let tmdb = match tmdb_api_base {
                Some(api_base) => TmdbProvider::with_api_base(loaded.value.tmdb, api_base)?,
                None => TmdbProvider::new(loaded.value.tmdb)?,
            };
            let providers = ProviderRegistry::new(tmdb);
            // 通过注册表选择策略，使通用下载流程不依赖 TMDB 实现细节。
            let artwork = providers
                .provider(resource_id.provider)
                .artwork(&resource_id)?;
            let client = Client::builder()
                // 原图可能较大，因此图片下载使用比元数据请求更宽松的总超时。
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(60))
                .redirect(Policy::limited(5))
                .build()
                .map_err(|error| AppError::HttpClient(error.to_string()))?;
            let installed = install_artwork(&ReqwestBinaryFetcher::new(client), &artwork, &output)?;
            // 两张图片全部提交成功后，才向用户输出最终路径。
            println!("background: {}", installed.background.display());
            println!("cover: {}", installed.cover.display());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use clap::Parser;
    use tempfile::tempdir;

    use crate::media_info::{AnalyzeError, MediaAnalyzer};

    use super::{Cli, Command, run, run_with_media_analyzer};

    struct FakeAnalyzer;

    impl MediaAnalyzer for FakeAnalyzer {
        fn analyze(&self, _input: &Path) -> Result<String, AnalyzeError> {
            Ok("General\nFormat : MPEG-4\n\nVideo\nFormat : AVC\n".into())
        }
    }

    #[test]
    fn parses_mediainfo_short_and_long_options() {
        for args in [
            vec!["crabgrab", "mediainfo", "-i", "movie.mp4", "-o", "out"],
            vec![
                "crabgrab",
                "mediainfo",
                "--input",
                "movie.mp4",
                "--output",
                "out",
            ],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(matches!(cli.command, Some(Command::MediaInfo { .. })));
        }
    }

    #[test]
    fn mediainfo_dispatch_does_not_need_tmdb_configuration() {
        let root = tempdir().unwrap();
        let executable = root.path().join("bin/crabgrab");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        let input = root.path().join("影片.mp4");
        fs::write(&input, b"fixture").unwrap();
        let output = root.path().join("result");
        let cli = Cli::try_parse_from([
            "crabgrab",
            "mediainfo",
            "-i",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .unwrap();

        run_with_media_analyzer(cli, executable.clone(), &FakeAnalyzer).unwrap();

        assert!(output.join("mediainfo.txt").is_file());
        assert!(!executable.parent().unwrap().join("config.toml").exists());
    }

    #[test]
    fn config_init_creates_template_without_overwriting_it() {
        let root = tempdir().unwrap();
        let executable = root.path().join("bin/crabgrab");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();

        run(
            Cli::try_parse_from(["crabgrab", "config", "init"]).unwrap(),
            executable.clone(),
        )
        .unwrap();
        let config = executable.parent().unwrap().join("config.toml");
        let original = fs::read_to_string(&config).unwrap();

        assert_eq!(original, "[tmdb]\napi_token = \"\"\nlanguage = \"zh-CN\"\n");
        assert!(
            run(
                Cli::try_parse_from(["crabgrab", "config", "init"]).unwrap(),
                executable
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(config).unwrap(), original);
    }
}
