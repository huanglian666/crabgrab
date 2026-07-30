use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use super::SubtitleSource;

pub struct FrameRequest<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    pub timestamp_ms: u64,
    pub subtitle: Option<&'a SubtitleSource>,
}

pub trait FrameExtractor {
    fn extract(&self, request: &FrameRequest<'_>) -> Result<(), ExtractError>;
}

pub struct ProcessFrameExtractor {
    executable: PathBuf,
    expected_sha256: String,
}

impl ProcessFrameExtractor {
    pub fn new(executable: PathBuf, expected_sha256: impl Into<String>) -> Self {
        Self {
            executable,
            expected_sha256: expected_sha256.into(),
        }
    }

    pub fn bundled(crabgrab_executable: &Path) -> Self {
        let release_name = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        let release_path = crabgrab_executable
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("tools")
            .join(release_name);

        #[cfg(debug_assertions)]
        let executable = if release_path.is_file() {
            release_path
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(".crabgrab-tools/ffmpeg")
                .join(target_triple())
                .join(release_name)
        };
        #[cfg(not(debug_assertions))]
        let executable = release_path;

        Self::new(executable, expected_sha256())
    }
}

#[cfg(all(debug_assertions, target_os = "macos"))]
fn target_triple() -> &'static str {
    "aarch64-apple-darwin"
}

#[cfg(all(debug_assertions, target_os = "windows"))]
fn target_triple() -> &'static str {
    "x86_64-pc-windows-msvc"
}

#[cfg(target_os = "macos")]
fn expected_sha256() -> &'static str {
    "eaf91238e104dd0e262bc6510e25061855cc99a6955a721b0ac99660d58c473d"
}

#[cfg(target_os = "windows")]
fn expected_sha256() -> &'static str {
    "e155d775a7ebd9fe2400b3e880a4b3c5b03ecd34ebdb69bbe4b2787af8c4bf16"
}

#[derive(Debug, Error)]
pub enum ExtractError {
    #[error("screenshot output already exists: {0}")]
    OutputExists(PathBuf),
    #[error("bundled FFmpeg is missing: {0}")]
    MissingTool(PathBuf),
    #[error("cannot read bundled FFmpeg {path}: {source}")]
    ReadTool {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("bundled FFmpeg SHA-256 verification failed: {0}")]
    ToolHash(PathBuf),
    #[error("cannot start bundled FFmpeg: {0}")]
    Start(std::io::Error),
    #[error("bundled FFmpeg failed: {message}")]
    Failed {
        message: String,
        subtitle_attempt: bool,
    },
    #[error("bundled FFmpeg reported success without creating {0}")]
    MissingOutput(PathBuf),
}

impl ExtractError {
    pub fn is_subtitle_failure(&self) -> bool {
        matches!(
            self,
            Self::Failed {
                subtitle_attempt: true,
                ..
            }
        )
    }
}

impl FrameExtractor for ProcessFrameExtractor {
    fn extract(&self, request: &FrameRequest<'_>) -> Result<(), ExtractError> {
        if request.output.exists() {
            return Err(ExtractError::OutputExists(request.output.to_path_buf()));
        }
        verify_ffmpeg(&self.executable, &self.expected_sha256)?;
        let filter = frame_filter(request.input, request.subtitle);
        let output = Command::new(&self.executable)
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-n"])
            .arg("-ss")
            .arg(format_ffmpeg_timestamp(request.timestamp_ms))
            .arg("-i")
            .arg(request.input)
            .args(["-map", "0:v:0", "-frames:v", "1", "-vf"])
            .arg(filter)
            .args([
                "-an", "-sn", "-c:v", "png", "-pix_fmt", "rgb24", "-f", "image2",
            ])
            .arg(request.output)
            .output()
            .map_err(ExtractError::Start)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ExtractError::Failed {
                message: stderr
                    .chars()
                    .take(4096)
                    .collect::<String>()
                    .trim()
                    .to_owned(),
                subtitle_attempt: request.subtitle.is_some(),
            });
        }
        if !request.output.is_file() {
            return Err(ExtractError::MissingOutput(request.output.to_path_buf()));
        }
        Ok(())
    }
}

fn verify_ffmpeg(path: &Path, expected_sha256: &str) -> Result<(), ExtractError> {
    if !path.is_file() {
        return Err(ExtractError::MissingTool(path.to_path_buf()));
    }
    let actual =
        crate::media_info::tool::sha256(path).map_err(|source| ExtractError::ReadTool {
            path: path.to_path_buf(),
            source,
        })?;
    if actual != expected_sha256.to_ascii_lowercase() {
        return Err(ExtractError::ToolHash(path.to_path_buf()));
    }
    Ok(())
}

fn frame_filter(input: &Path, subtitle: Option<&SubtitleSource>) -> String {
    let scale = "scale=trunc(iw*sar/2)*2:ih,setsar=1";
    let subtitle_style = format!("force_style='FontName={}'", fallback_subtitle_font());
    match subtitle {
        Some(SubtitleSource::External(path)) => {
            format!(
                "subtitles=filename='{}':{subtitle_style},{scale}",
                escape_filter_path(path)
            )
        }
        Some(SubtitleSource::Embedded {
            stream_kind_position,
        }) => format!(
            "subtitles=filename='{}':si={}:{subtitle_style},{scale}",
            escape_filter_path(input),
            stream_kind_position.saturating_sub(1)
        ),
        None => scale.to_owned(),
    }
}

#[cfg(target_os = "macos")]
fn fallback_subtitle_font() -> &'static str {
    "Hiragino Sans GB"
}

#[cfg(target_os = "windows")]
fn fallback_subtitle_font() -> &'static str {
    "Microsoft YaHei"
}

fn escape_filter_path(path: &Path) -> String {
    let mut escaped = String::new();
    for character in path.to_string_lossy().chars() {
        if matches!(character, '\\' | ':' | '\'' | ',' | ';' | '[' | ']') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn format_ffmpeg_timestamp(milliseconds: u64) -> String {
    let hours = milliseconds / 3_600_000;
    let minutes = milliseconds % 3_600_000 / 60_000;
    let seconds = milliseconds % 60_000 / 1000;
    let millis = milliseconds % 1000;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use tempfile::tempdir;

    use crate::screenshot::SubtitleSource;

    use super::{FrameExtractor, FrameRequest, ProcessFrameExtractor};

    fn fake_ffmpeg() -> (tempfile::TempDir, std::path::PathBuf, String) {
        let root = tempdir().unwrap();
        let executable = root.path().join("fake-ffmpeg");
        fs::write(
            &executable,
            r#"#!/bin/sh
printf '%s\n' "$@" > "$0.args"
for value in "$@"; do output="$value"; done
printf '\211PNG\r\n\032\n' > "$output"
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        let hash = crate::media_info::tool::sha256(&executable).unwrap();
        (root, executable, hash)
    }

    #[test]
    fn extracts_one_png_frame_without_a_shell() {
        let (root, executable, hash) = fake_ffmpeg();
        let input = root.path().join("中文 ; $movie.mkv");
        let output = root.path().join("frame.png");
        fs::write(&input, b"video").unwrap();
        let extractor = ProcessFrameExtractor::new(executable.clone(), hash);

        extractor
            .extract(&FrameRequest {
                input: &input,
                output: &output,
                timestamp_ms: 10_500,
                subtitle: None,
            })
            .unwrap();

        assert_eq!(&fs::read(&output).unwrap()[..4], b"\x89PNG");
        let args = fs::read_to_string(executable.with_extension("args")).unwrap();
        assert!(args.lines().any(|arg| arg == "00:00:10.500"));
        assert!(args.lines().any(|arg| arg == input.to_string_lossy()));
        assert!(args.lines().any(|arg| arg == "png"));
        assert!(args.lines().any(|arg| arg == "rgb24"));
    }

    #[test]
    fn builds_external_subtitle_filter_with_escaped_path() {
        let (root, executable, hash) = fake_ffmpeg();
        let input = root.path().join("Movie.mkv");
        let subtitle = root.path().join("a: b's\\字幕.ass");
        let output = root.path().join("frame.png");
        fs::write(&input, b"video").unwrap();
        fs::write(&subtitle, b"subtitle").unwrap();

        ProcessFrameExtractor::new(executable.clone(), hash)
            .extract(&FrameRequest {
                input: &input,
                output: &output,
                timestamp_ms: 20_000,
                subtitle: Some(&SubtitleSource::External(subtitle.clone())),
            })
            .unwrap();

        let args = fs::read_to_string(executable.with_extension("args")).unwrap();
        let filter = args
            .lines()
            .find(|arg| arg.starts_with("subtitles="))
            .unwrap();
        assert!(filter.contains("a\\: b\\'s\\\\字幕.ass"));
        assert!(filter.contains(":force_style='FontName=Hiragino Sans GB'"));
        assert!(filter.ends_with(",scale=trunc(iw*sar/2)*2:ih,setsar=1"));
    }

    #[test]
    fn converts_one_based_embedded_position_to_ffmpeg_subtitle_index() {
        let (root, executable, hash) = fake_ffmpeg();
        let input = root.path().join("Movie.mkv");
        let output = root.path().join("frame.png");
        fs::write(&input, b"video").unwrap();

        ProcessFrameExtractor::new(executable.clone(), hash)
            .extract(&FrameRequest {
                input: &input,
                output: &output,
                timestamp_ms: 30_000,
                subtitle: Some(&SubtitleSource::Embedded {
                    stream_kind_position: 2,
                }),
            })
            .unwrap();

        let args = fs::read_to_string(executable.with_extension("args")).unwrap();
        assert!(args.contains(":si=1"));
    }

    #[test]
    fn refuses_to_overwrite_an_existing_output() {
        let (root, executable, hash) = fake_ffmpeg();
        let input = root.path().join("Movie.mkv");
        let output = root.path().join("frame.png");
        fs::write(&input, b"video").unwrap();
        fs::write(&output, b"old").unwrap();

        let error = ProcessFrameExtractor::new(executable, hash)
            .extract(&FrameRequest {
                input: &input,
                output: &output,
                timestamp_ms: 1_000,
                subtitle: None,
            })
            .unwrap_err();

        assert!(error.to_string().contains("exists"));
        assert_eq!(fs::read(output).unwrap(), b"old");
    }
}
