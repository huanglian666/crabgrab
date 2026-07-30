mod install;
mod process;
mod subtitle;
mod timeline;

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::config::ScreenshotConfig;
use crate::media_info::{AnalyzeError, MediaProber};

pub use install::{InstallError, ManifestImage, ScreenshotManifest, install_screenshots};
pub use process::{ExtractError, FrameExtractor, FrameRequest, ProcessFrameExtractor};
pub use subtitle::{SubtitleError, SubtitleSource, select_subtitle};
pub use timeline::{
    Timeline, TimelineError, TimestampSpec, build_timeline, format_timestamp, parse_timestamp,
};

#[derive(Debug, Clone)]
pub struct ScreenshotResult {
    pub directory: PathBuf,
    pub generated: usize,
    pub timestamps: Vec<String>,
    pub subtitle: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ScreenshotError {
    #[error("cannot inspect screenshot input {path}: {source}")]
    Input {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("screenshot input is not a regular file: {0}")]
    NotFile(PathBuf),
    #[error(transparent)]
    Analyze(#[from] AnalyzeError),
    #[error(transparent)]
    Timeline(#[from] TimelineError),
    #[error(transparent)]
    Subtitle(#[from] SubtitleError),
    #[error(transparent)]
    Extract(#[from] ExtractError),
    #[error(transparent)]
    Install(#[from] InstallError),
}

pub fn generate_screenshots(
    prober: &(impl MediaProber + ?Sized),
    extractor: &(impl FrameExtractor + ?Sized),
    input: &Path,
    output: &Path,
    config: &ScreenshotConfig,
) -> Result<ScreenshotResult, ScreenshotError> {
    let metadata = fs::metadata(input).map_err(|source| ScreenshotError::Input {
        path: input.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(ScreenshotError::NotFile(input.to_path_buf()));
    }
    let probe = prober.probe(input)?;
    let mut rng = rand::rng();
    let timeline = build_timeline(
        probe.duration_ms,
        config.count,
        &config.timestamps,
        &mut rng,
    )?;
    let mut active_subtitle = if config.subtitles {
        select_subtitle(input, &config.subtitle_languages, &probe.subtitles)?
    } else {
        None
    };
    let embedded_fallback =
        subtitle::select_embedded_source(&config.subtitle_languages, &probe.subtitles);
    let width = timeline.points_ms.len().to_string().len().max(2);
    let mut warnings = Vec::new();
    if timeline.duplicate_count > 0 {
        warnings.push(format!(
            "ignored {} duplicate screenshot timestamp(s)",
            timeline.duplicate_count
        ));
    }
    if timeline.expanded_beyond_count {
        warnings.push(format!(
            "configured timestamps expanded screenshot count to {}",
            timeline.points_ms.len()
        ));
    }

    let video_name = input
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| input.to_string_lossy().into_owned());
    let points = timeline.points_ms.clone();
    let directory = install::install_generated_screenshots(output, |stage| {
        let mut images = Vec::with_capacity(points.len());
        for (index, point) in points.iter().copied().enumerate() {
            let filename = format!("{:0width$}.png", index + 1, width = width);
            let destination = stage.join(&filename);
            loop {
                let request = FrameRequest {
                    input,
                    output: &destination,
                    timestamp_ms: point,
                    subtitle: active_subtitle.as_ref(),
                };
                match extractor.extract(&request) {
                    Ok(()) => break,
                    Err(error) if error.is_subtitle_failure() => {
                        let failed = subtitle_label(active_subtitle.as_ref());
                        warnings.push(format!(
                            "subtitle {failed} could not be rendered; using fallback"
                        ));
                        let _ = fs::remove_file(&destination);
                        active_subtitle = match active_subtitle {
                            Some(SubtitleSource::External(_)) => embedded_fallback.clone(),
                            Some(SubtitleSource::Embedded { .. }) => None,
                            None => return Err(ScreenshotError::Extract(error)),
                        };
                    }
                    Err(error) => return Err(ScreenshotError::Extract(error)),
                }
            }
            images.push(ManifestImage {
                file: filename,
                timestamp: format_timestamp(point),
                subtitle: active_subtitle
                    .as_ref()
                    .map(|source| subtitle_label(Some(source))),
            });
        }
        Ok::<_, ScreenshotError>(ScreenshotManifest {
            ffmpeg_version: "8.1.2".into(),
            video: video_name,
            images,
        })
    })?;
    let timestamps = points
        .iter()
        .copied()
        .map(format_timestamp)
        .collect::<Vec<_>>();
    Ok(ScreenshotResult {
        directory,
        generated: points.len(),
        timestamps,
        subtitle: active_subtitle
            .as_ref()
            .map(|source| subtitle_label(Some(source))),
        warnings,
    })
}

fn subtitle_label(source: Option<&SubtitleSource>) -> String {
    match source {
        Some(SubtitleSource::External(path)) => path.to_string_lossy().into_owned(),
        Some(SubtitleSource::Embedded {
            stream_kind_position,
        }) => format!("embedded:{stream_kind_position}"),
        None => "none".into(),
    }
}

#[cfg(test)]
mod workflow_tests {
    use std::fs;
    use std::path::Path;
    use std::sync::Mutex;

    use tempfile::tempdir;

    use crate::config::ScreenshotConfig;
    use crate::media_info::{AnalyzeError, MediaProbe, MediaProber, SubtitleTrack};

    use super::{ExtractError, FrameExtractor, FrameRequest, SubtitleSource, generate_screenshots};

    struct FakeProber(MediaProbe);

    impl MediaProber for FakeProber {
        fn probe(&self, _input: &Path) -> Result<MediaProbe, AnalyzeError> {
            Ok(self.0.clone())
        }
    }

    struct RecordingExtractor {
        requests: Mutex<Vec<Option<SubtitleSource>>>,
        fail_external: bool,
    }

    impl FrameExtractor for RecordingExtractor {
        fn extract(&self, request: &FrameRequest<'_>) -> Result<(), ExtractError> {
            let subtitle = request.subtitle.cloned();
            self.requests.lock().unwrap().push(subtitle.clone());
            if self.fail_external && matches!(subtitle, Some(SubtitleSource::External(_))) {
                return Err(ExtractError::Failed {
                    message: "bad external subtitle".into(),
                    subtitle_attempt: true,
                });
            }
            fs::write(request.output, b"\x89PNG\r\n\x1a\n").unwrap();
            Ok(())
        }
    }

    fn config() -> ScreenshotConfig {
        ScreenshotConfig {
            count: 2,
            timestamps: vec!["00:00:10".into(), "00:00:20".into()],
            subtitles: true,
            subtitle_languages: vec!["zh-CN".into(), "zh".into()],
        }
    }

    #[test]
    fn generates_time_ordered_pngs_and_manifest() {
        let root = tempdir().unwrap();
        let input = root.path().join("Movie.mkv");
        let output = root.path().join("result");
        fs::write(&input, b"video").unwrap();
        let extractor = RecordingExtractor {
            requests: Mutex::new(Vec::new()),
            fail_external: false,
        };

        let result = generate_screenshots(
            &FakeProber(MediaProbe {
                duration_ms: 60_000,
                has_video: true,
                subtitles: Vec::new(),
            }),
            &extractor,
            &input,
            &output,
            &config(),
        )
        .unwrap();

        assert_eq!(result.generated, 2);
        assert_eq!(result.timestamps, ["00:00:10", "00:00:20"]);
        assert!(output.join("screenshots/01.png").is_file());
        assert!(output.join("screenshots/02.png").is_file());
        let marker = fs::read_to_string(output.join("screenshots/.crabgrab-screenshots")).unwrap();
        assert!(marker.contains("file = \"01.png\""));
        assert!(marker.contains("timestamp = \"00:00:20\""));
    }

    #[test]
    fn external_subtitle_failure_falls_back_to_embedded_for_all_images() {
        let root = tempdir().unwrap();
        let input = root.path().join("Movie.mkv");
        let output = root.path().join("result");
        fs::write(&input, b"video").unwrap();
        fs::write(root.path().join("Movie.zh-CN.ass"), b"broken").unwrap();
        let extractor = RecordingExtractor {
            requests: Mutex::new(Vec::new()),
            fail_external: true,
        };

        let result = generate_screenshots(
            &FakeProber(MediaProbe {
                duration_ms: 60_000,
                has_video: true,
                subtitles: vec![SubtitleTrack {
                    stream_kind_position: 1,
                    language: Some("zh-CN".into()),
                    is_default: true,
                    format: "ASS".into(),
                }],
            }),
            &extractor,
            &input,
            &output,
            &config(),
        )
        .unwrap();

        assert_eq!(result.subtitle.as_deref(), Some("embedded:1"));
        assert_eq!(result.warnings.len(), 1);
        let requests = extractor.requests.lock().unwrap();
        assert!(matches!(requests[0], Some(SubtitleSource::External(_))));
        assert!(requests[1..].iter().all(|source| {
            matches!(
                source,
                Some(SubtitleSource::Embedded {
                    stream_kind_position: 1
                })
            )
        }));
    }
}
