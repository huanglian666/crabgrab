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
use crate::media_info::{
    MediaAnalyzer, MediaInfoError, MediaProber, ProcessMediaAnalyzer, generate_report,
};
use crate::providers::{ProviderError, ProviderRegistry, TmdbProvider};
use crate::screenshot::{
    FrameExtractor, ProcessFrameExtractor, ScreenshotError, generate_screenshots,
};

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
    Sc {
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
    #[error(transparent)]
    Screenshot(#[from] ScreenshotError),
    #[error("screenshot services were not configured")]
    ScreenshotServices,
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
    let extractor = ProcessFrameExtractor::bundled(&executable);
    run_with_services(
        cli,
        executable,
        None,
        &analyzer,
        Some((&analyzer, &extractor)),
    )
}

#[doc(hidden)]
/// 执行 CLI；可选 API 基址仅供本地模拟测试注入。
pub fn run_with_api_base(
    cli: Cli,
    executable: PathBuf,
    tmdb_api_base: Option<Url>,
) -> Result<(), AppError> {
    let analyzer = ProcessMediaAnalyzer::bundled(&executable);
    let extractor = ProcessFrameExtractor::bundled(&executable);
    run_with_services(
        cli,
        executable,
        tmdb_api_base,
        &analyzer,
        Some((&analyzer, &extractor)),
    )
}

#[doc(hidden)]
pub fn run_with_media_analyzer(
    cli: Cli,
    executable: PathBuf,
    analyzer: &impl MediaAnalyzer,
) -> Result<(), AppError> {
    run_with_services(cli, executable, None, analyzer, None)
}

#[doc(hidden)]
pub fn run_with_screenshot_services(
    cli: Cli,
    executable: PathBuf,
    media: &(impl MediaAnalyzer + MediaProber),
    extractor: &impl FrameExtractor,
) -> Result<(), AppError> {
    run_with_services(cli, executable, None, media, Some((media, extractor)))
}

fn run_with_services(
    cli: Cli,
    executable: PathBuf,
    tmdb_api_base: Option<Url>,
    analyzer: &impl MediaAnalyzer,
    screenshot_services: Option<(&dyn MediaProber, &dyn FrameExtractor)>,
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
        Some(Command::Sc { input, output }) => {
            let (prober, extractor) = screenshot_services.ok_or(AppError::ScreenshotServices)?;
            let paths = resolve_config_paths(&executable)?;
            let loaded = load_config(&paths)?;
            let result =
                generate_screenshots(prober, extractor, &input, &output, &loaded.value.screenshot)?;
            for warning in &result.warnings {
                eprintln!("warning: {warning}");
            }
            println!("screenshots: {}", result.directory.display());
            println!("generated: {}", result.generated);
            println!("subtitle: {}", result.subtitle.as_deref().unwrap_or("none"));
            println!("timestamps: {}", result.timestamps.join(", "));
            Ok(())
        }
        None => {
            let id = cli.id.expect("clap requires id for download");
            let output = cli.output.expect("clap requires output for download");
            let resource_id = id.parse::<ResourceId>()?;
            // ID 必须先验证，非法输入不能触发配置读取或网络请求。
            let paths = resolve_config_paths(&executable)?;
            let loaded = load_config(&paths)?;
            let tmdb_config = loaded.value.tmdb.require_token(&loaded.path)?;
            let tmdb = match tmdb_api_base {
                Some(api_base) => TmdbProvider::with_api_base(tmdb_config, api_base)?,
                None => TmdbProvider::new(tmdb_config)?,
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

    use crate::media_info::{AnalyzeError, MediaAnalyzer, MediaProbe, MediaProber};
    use crate::screenshot::{ExtractError, FrameExtractor, FrameRequest};

    use super::{Cli, Command, run, run_with_media_analyzer, run_with_screenshot_services};

    struct FakeAnalyzer;

    impl MediaAnalyzer for FakeAnalyzer {
        fn analyze(&self, _input: &Path) -> Result<String, AnalyzeError> {
            Ok("General\nFormat : MPEG-4\n\nVideo\nFormat : AVC\n".into())
        }
    }

    impl MediaProber for FakeAnalyzer {
        fn probe(&self, _input: &Path) -> Result<MediaProbe, AnalyzeError> {
            Ok(MediaProbe {
                duration_ms: 60_000,
                has_video: true,
                subtitles: Vec::new(),
            })
        }
    }

    struct FakeExtractor;

    impl FrameExtractor for FakeExtractor {
        fn extract(&self, request: &FrameRequest<'_>) -> Result<(), ExtractError> {
            fs::write(request.output, b"\x89PNG\r\n\x1a\n").unwrap();
            Ok(())
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
    fn parses_sc_short_and_long_options() {
        for args in [
            vec!["crabgrab", "sc", "-i", "movie.mkv", "-o", "out"],
            vec!["crabgrab", "sc", "--input", "movie.mkv", "--output", "out"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(matches!(cli.command, Some(Command::Sc { .. })));
        }
    }

    #[test]
    fn sc_dispatch_uses_screenshot_config_without_tmdb_token() {
        let root = tempdir().unwrap();
        let executable = root.path().join("bin/crabgrab");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(
            executable.parent().unwrap().join("config.toml"),
            "[tmdb]\napi_token=''\n[screenshot]\ncount=2\ntimestamps=['00:00:10','00:00:20']\nsubtitles=false\n",
        )
        .unwrap();
        let input = root.path().join("Movie.mkv");
        let output = root.path().join("result");
        fs::write(&input, b"video").unwrap();
        let cli = Cli::try_parse_from([
            "crabgrab",
            "sc",
            "-i",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .unwrap();

        run_with_screenshot_services(cli, executable, &FakeAnalyzer, &FakeExtractor).unwrap();

        assert!(output.join("screenshots/01.png").is_file());
        assert!(output.join("screenshots/02.png").is_file());
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

        assert!(original.contains("[tmdb]\napi_token = \"\"\nlanguage = \"zh-CN\""));
        assert!(original.contains("[screenshot]\ncount = 3"));
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
