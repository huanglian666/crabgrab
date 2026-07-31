use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{ArgAction, ArgGroup, Parser, Subcommand};
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
use crate::tree_report::{TreeReportError, generate_tree_report};

#[derive(Debug, Parser)]
#[command(
    name = "crabgrab",
    version,
    disable_version_flag = true,
    arg_required_else_help = true,
    after_help = "Combined actions:\n  crabgrab -p <RESOURCE_ID> <OUTPUT>\n  crabgrab -smt <VIDEO> <OUTPUT>\n  crabgrab -psmt <RESOURCE_ID> <VIDEO> <OUTPUT>",
    group(ArgGroup::new("top_level_action").args(["id", "legacy_tree"]).multiple(false))
)]
pub struct Cli {
    #[arg(short = 'v', long = "version", action = ArgAction::Version)]
    version: Option<bool>,

    #[arg(short = 'i', long, requires = "output", value_name = "RESOURCE_ID")]
    id: Option<String>,

    #[arg(short = 'p', action = ArgAction::SetTrue, help = "Download poster artwork")]
    poster: bool,

    #[arg(short = 's', action = ArgAction::SetTrue, help = "Generate screenshots")]
    screenshots: bool,

    #[arg(short = 'm', action = ArgAction::SetTrue, help = "Generate MediaInfo report")]
    media_info: bool,

    #[arg(short = 't', action = ArgAction::SetTrue, help = "Generate tree report")]
    tree: bool,

    #[arg(
        long = "tree",
        requires = "output",
        value_name = "VIDEO",
        help = "Deprecated tree syntax; prefer -t <VIDEO> <OUTPUT>"
    )]
    legacy_tree: Option<PathBuf>,

    #[arg(
        short = 'o',
        long,
        requires = "top_level_action",
        value_name = "DIRECTORY"
    )]
    output: Option<PathBuf>,

    #[arg(
        value_name = "ARGUMENT",
        help = "Conditional RESOURCE_ID, VIDEO, and OUTPUT values"
    )]
    arguments: Vec<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Copy, Debug)]
struct ActionSet {
    poster: bool,
    screenshots: bool,
    media_info: bool,
    tree: bool,
}

impl ActionSet {
    fn from_cli(cli: &Cli) -> Self {
        Self {
            poster: cli.poster,
            screenshots: cli.screenshots,
            media_info: cli.media_info,
            tree: cli.tree,
        }
    }

    fn any(self) -> bool {
        self.poster || self.screenshots || self.media_info || self.tree
    }

    fn needs_video(self) -> bool {
        self.screenshots || self.media_info || self.tree
    }
}

#[derive(Debug)]
struct CombinedRequest {
    actions: ActionSet,
    resource_id: Option<ResourceId>,
    video: Option<PathBuf>,
    output: PathBuf,
}

impl CombinedRequest {
    fn parse(actions: ActionSet, arguments: &[String]) -> Result<Self, AppError> {
        let expected = match (actions.poster, actions.needs_video()) {
            (true, true) => 3,
            (true, false) | (false, true) => 2,
            (false, false) => return Err(AppError::CombinedArguments),
        };
        if arguments.len() != expected {
            return Err(AppError::CombinedArguments);
        }

        let (resource_id, video, output) = match (actions.poster, actions.needs_video()) {
            (true, true) => (
                Some(arguments[0].parse::<ResourceId>()?),
                Some(PathBuf::from(&arguments[1])),
                PathBuf::from(&arguments[2]),
            ),
            (true, false) => (
                Some(arguments[0].parse::<ResourceId>()?),
                None,
                PathBuf::from(&arguments[1]),
            ),
            (false, true) => (
                None,
                Some(PathBuf::from(&arguments[0])),
                PathBuf::from(&arguments[1]),
            ),
            (false, false) => unreachable!("no actions rejected above"),
        };
        Ok(Self {
            actions,
            resource_id,
            video,
            output,
        })
    }
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
    #[error(transparent)]
    TreeReport(#[from] TreeReportError),
    #[error("screenshot services were not configured")]
    ScreenshotServices,
    #[error("cannot determine current executable: {0}")]
    Executable(std::io::Error),
    #[error("failed to create HTTP client: {0}")]
    HttpClient(String),
    #[error("top-level --id/--tree/--output options cannot be combined with a subcommand")]
    ConflictingActions,
    #[error(
        "invalid combined arguments; use `crabgrab -p <RESOURCE_ID> <OUTPUT>`, `crabgrab -smt <VIDEO> <OUTPUT>`, or `crabgrab -psmt <RESOURCE_ID> <VIDEO> <OUTPUT>`"
    )]
    CombinedArguments,
    #[error("cannot inspect combined media input {path}: {source}")]
    CombinedInput {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot prepare combined output directory {path}: {source}")]
    CombinedOutput {
        path: PathBuf,
        source: std::io::Error,
    },
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
    let actions = ActionSet::from_cli(&cli);
    if actions.any() {
        if cli.command.is_some()
            || cli.id.is_some()
            || cli.legacy_tree.is_some()
            || cli.output.is_some()
        {
            return Err(AppError::ConflictingActions);
        }
        let request = CombinedRequest::parse(actions, &cli.arguments)?;
        if let Some(video) = &request.video {
            let metadata = fs::metadata(video).map_err(|source| AppError::CombinedInput {
                path: video.clone(),
                source,
            })?;
            if !metadata.is_file() {
                return Err(AppError::CombinedInput {
                    path: video.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "path is not a regular file",
                    ),
                });
            }
        }
        fs::create_dir_all(&request.output).map_err(|source| AppError::CombinedOutput {
            path: request.output.clone(),
            source,
        })?;
        let mut completed = Vec::new();
        if request.actions.poster {
            let resource_id = request
                .resource_id
                .as_ref()
                .expect("poster action requires resource ID");
            download_artwork(
                &executable,
                tmdb_api_base.clone(),
                resource_id,
                &request.output,
            )?;
            completed.push("p");
        }
        if request.actions.media_info {
            let input = request
                .video
                .as_deref()
                .expect("media info action requires video");
            let installed = generate_report(analyzer, input, &request.output)?;
            println!("mediainfo: {}", installed.display());
            completed.push("m");
        }
        if request.actions.screenshots {
            let (prober, extractor) = screenshot_services.ok_or(AppError::ScreenshotServices)?;
            let paths = resolve_config_paths(&executable)?;
            let loaded = load_config(&paths)?;
            let input = request
                .video
                .as_deref()
                .expect("screenshot action requires video");
            let result = generate_screenshots(
                prober,
                extractor,
                input,
                &request.output,
                &loaded.value.screenshot,
            )?;
            for warning in &result.warnings {
                eprintln!("warning: {warning}");
            }
            println!("screenshots: {}", result.directory.display());
            println!("generated: {}", result.generated);
            println!("subtitle: {}", result.subtitle.as_deref().unwrap_or("none"));
            println!("timestamps: {}", result.timestamps.join(", "));
            completed.push("s");
        }
        if request.actions.tree {
            let input = request
                .video
                .as_deref()
                .expect("tree action requires video");
            let installed = generate_tree_report(input, &request.output)?;
            println!("tree: {}", installed.display());
            completed.push("t");
        }
        println!("completed: {}", completed.join(","));
        return Ok(());
    }
    if !cli.arguments.is_empty() {
        return Err(AppError::CombinedArguments);
    }
    // 测试可注入本地 API 地址；生产入口传入 None，始终使用 TMDB 官方地址。
    if cli.command.is_some()
        && (cli.id.is_some() || cli.legacy_tree.is_some() || cli.output.is_some())
    {
        // 子命令与下载参数是两类独立动作，禁止在一次调用中混用。
        return Err(AppError::ConflictingActions);
    }
    if let Some(video) = cli.legacy_tree {
        let output = cli.output.expect("clap requires output for tree report");
        let installed = generate_tree_report(&video, &output)?;
        println!("tree: {}", installed.display());
        return Ok(());
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
            download_artwork(&executable, tmdb_api_base, &resource_id, &output)
        }
    }
}

fn download_artwork(
    executable: &Path,
    tmdb_api_base: Option<Url>,
    resource_id: &ResourceId,
    output: &Path,
) -> Result<(), AppError> {
    // ID 必须先验证，非法输入不能触发配置读取或网络请求。
    let paths = resolve_config_paths(executable)?;
    let loaded = load_config(&paths)?;
    let tmdb_config = loaded.value.tmdb.require_token(&loaded.path)?;
    let tmdb = match tmdb_api_base {
        Some(api_base) => TmdbProvider::with_api_base(tmdb_config, api_base)?,
        None => TmdbProvider::new(tmdb_config)?,
    };
    let providers = ProviderRegistry::new(tmdb);
    let artwork = providers
        .provider(resource_id.provider)
        .artwork(resource_id)?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .redirect(Policy::limited(5))
        .build()
        .map_err(|error| AppError::HttpClient(error.to_string()))?;
    let installed = install_artwork(&ReqwestBinaryFetcher::new(client), &artwork, output)?;
    println!("background: {}", installed.background.display());
    println!("cover: {}", installed.cover.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use clap::Parser;
    use tempfile::tempdir;

    use crate::media_info::{AnalyzeError, MediaAnalyzer, MediaProbe, MediaProber};
    use crate::screenshot::{ExtractError, FrameExtractor, FrameRequest};

    use super::{
        AppError, Cli, Command, run, run_with_media_analyzer, run_with_screenshot_services,
    };

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

    struct FailingAnalyzer;

    impl MediaAnalyzer for FailingAnalyzer {
        fn analyze(&self, _input: &Path) -> Result<String, AnalyzeError> {
            Err(AnalyzeError::Failed("fixture failure".into()))
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
    fn parses_combined_short_actions() {
        assert!(
            Cli::try_parse_from(["crabgrab", "-psmt", "tmdb-movie-550", "movie.mkv", "out",])
                .is_ok()
        );
    }

    #[test]
    fn parses_standalone_combined_mediainfo_action() {
        assert!(Cli::try_parse_from(["crabgrab", "-m", "movie.mkv", "out"]).is_ok());
    }

    #[test]
    fn combined_actions_reject_wrong_positional_arity() {
        let root = tempdir().unwrap();
        let executable = root.path().join("bin/crabgrab");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();

        for args in [
            vec!["crabgrab", "-m", "movie.mkv"],
            vec!["crabgrab", "-p", "tmdb-movie-550", "movie.mkv", "out"],
            vec!["crabgrab", "movie.mkv", "out"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(matches!(
                run_with_media_analyzer(cli, executable.clone(), &FakeAnalyzer),
                Err(AppError::CombinedArguments)
            ));
        }
    }

    #[test]
    fn combined_video_validation_happens_before_poster_configuration() {
        let root = tempdir().unwrap();
        let executable = root.path().join("bin/crabgrab");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        let missing = root.path().join("missing.mkv");
        let output = root.path().join("out");
        let cli = Cli::try_parse_from([
            "crabgrab",
            "-pm",
            "tmdb-movie-550",
            missing.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .unwrap();

        let error = run_with_media_analyzer(cli, executable, &FakeAnalyzer).unwrap_err();
        assert!(error.to_string().contains("combined media input"));
        assert!(!output.exists());
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
    fn parses_legacy_tree_long_option_and_rejects_id_combination() {
        let cli =
            Cli::try_parse_from(["crabgrab", "--tree", "movie.mkv", "--output", "out"]).unwrap();
        assert_eq!(cli.legacy_tree, Some(PathBuf::from("movie.mkv")));

        assert!(
            Cli::try_parse_from([
                "crabgrab",
                "-i",
                "tmdb-movie-550",
                "--tree",
                "movie.mkv",
                "-o",
                "out",
            ])
            .is_err()
        );
    }

    #[test]
    fn tree_dispatch_does_not_need_tmdb_configuration_or_move_video() {
        let root = tempdir().unwrap();
        let executable = root.path().join("bin/crabgrab");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        let input = root.path().join("火遮眼 (2025).mp4");
        fs::write(&input, b"video").unwrap();
        let output = root.path().join("火遮眼 (2025)");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("01.jpg"), b"jpg").unwrap();
        let cli = Cli::try_parse_from([
            "crabgrab",
            "-t",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .unwrap();

        run(cli, executable.clone()).unwrap();

        let report = output.join("火遮眼 (2025).tree.txt");
        assert!(report.is_file());
        assert!(
            fs::read_to_string(report)
                .unwrap()
                .contains("火遮眼 (2025).mp4  [5 B]")
        );
        assert_eq!(fs::read(&input).unwrap(), b"video");
        assert!(!executable.parent().unwrap().join("config.toml").exists());
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
    fn combined_mediainfo_generates_report_without_tmdb_config() {
        let root = tempdir().unwrap();
        let executable = root.path().join("bin/crabgrab");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        let input = root.path().join("影片.mp4");
        fs::write(&input, b"fixture").unwrap();
        let output = root.path().join("result");
        let cli = Cli::try_parse_from([
            "crabgrab",
            "-m",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .unwrap();

        run_with_media_analyzer(cli, executable.clone(), &FakeAnalyzer).unwrap();

        assert!(output.join("mediainfo.txt").is_file());
        assert!(!executable.parent().unwrap().join("config.toml").exists());
    }

    #[test]
    fn combined_mediainfo_runs_before_tree() {
        let root = tempdir().unwrap();
        let executable = root.path().join("bin/crabgrab");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        let input = root.path().join("Movie.mkv");
        fs::write(&input, b"video").unwrap();
        let output = root.path().join("Movie");
        let cli = Cli::try_parse_from([
            "crabgrab",
            "-mt",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .unwrap();

        run_with_media_analyzer(cli, executable, &FakeAnalyzer).unwrap();

        let report = fs::read_to_string(output.join("Movie.tree.txt")).unwrap();
        assert!(report.contains("mediainfo.txt"));
        assert!(report.contains("Movie.mkv  [5 B]"));
    }

    #[test]
    fn combined_failure_stops_before_tree() {
        let root = tempdir().unwrap();
        let executable = root.path().join("bin/crabgrab");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        let input = root.path().join("Movie.mkv");
        fs::write(&input, b"video").unwrap();
        let output = root.path().join("Movie");
        let cli = Cli::try_parse_from([
            "crabgrab",
            "-mt",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .unwrap();

        assert!(run_with_media_analyzer(cli, executable, &FailingAnalyzer).is_err());
        assert!(!output.join("Movie.tree.txt").exists());
    }

    #[test]
    fn combined_screenshot_generates_images() {
        let root = tempdir().unwrap();
        let executable = root.path().join("bin/crabgrab");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(
            executable.parent().unwrap().join("config.toml"),
            "[tmdb]\napi_token=''\n[screenshot]\ncount=2\ntimestamps=['00:00:10','00:00:20']\nsubtitles=false\n",
        )
        .unwrap();
        let input = root.path().join("Movie.mkv");
        fs::write(&input, b"video").unwrap();
        let output = root.path().join("result");
        let cli = Cli::try_parse_from([
            "crabgrab",
            "-s",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .unwrap();

        run_with_screenshot_services(cli, executable, &FakeAnalyzer, &FakeExtractor).unwrap();

        assert!(output.join("screenshots/01.png").is_file());
        assert!(output.join("screenshots/02.png").is_file());
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
