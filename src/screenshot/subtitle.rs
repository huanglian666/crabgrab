use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::media_info::SubtitleTrack;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtitleSource {
    External(PathBuf),
    Embedded { stream_kind_position: usize },
}

#[derive(Debug, Error)]
pub enum SubtitleError {
    #[error("video path has no usable parent directory: {0}")]
    MissingDirectory(PathBuf),
    #[error("video path has no UTF-8 file name: {0}")]
    InvalidName(PathBuf),
    #[error("cannot scan subtitle directory {path}: {source}")]
    Scan {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub fn select_subtitle(
    video: &Path,
    languages: &[String],
    tracks: &[SubtitleTrack],
) -> Result<Option<SubtitleSource>, SubtitleError> {
    if let Some(external) = select_external(video, languages)? {
        return Ok(Some(SubtitleSource::External(external)));
    }
    Ok(
        select_embedded(languages, tracks).map(|track| SubtitleSource::Embedded {
            stream_kind_position: track.stream_kind_position,
        }),
    )
}

fn select_external(video: &Path, languages: &[String]) -> Result<Option<PathBuf>, SubtitleError> {
    let directory = video
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| SubtitleError::MissingDirectory(video.to_path_buf()))?;
    let video_stem = video
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| SubtitleError::InvalidName(video.to_path_buf()))?;
    let normalized_languages = languages
        .iter()
        .map(|language| normalize_language(language))
        .collect::<Vec<_>>();
    let entries = fs::read_dir(directory).map_err(|source| SubtitleError::Scan {
        path: directory.to_path_buf(),
        source,
    })?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| SubtitleError::Scan {
            path: directory.to_path_buf(),
            source,
        })?;
        let file_type = entry.file_type().map_err(|source| SubtitleError::Scan {
            path: directory.to_path_buf(),
            source,
        })?;
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(format_rank) = path
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(subtitle_format_rank)
        else {
            continue;
        };
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let language_rank = if stem == video_stem {
            normalized_languages.len()
        } else if let Some(suffix) = stem
            .strip_prefix(video_stem)
            .and_then(|suffix| suffix.strip_prefix('.'))
        {
            let normalized = normalize_language(suffix);
            let Some(rank) = normalized_languages
                .iter()
                .position(|language| *language == normalized)
            else {
                continue;
            };
            rank
        } else {
            continue;
        };
        candidates.push((language_rank, format_rank, path));
    }
    candidates.sort_by(|left, right| (left.0, left.1, &left.2).cmp(&(right.0, right.1, &right.2)));
    Ok(candidates.into_iter().next().map(|(_, _, path)| path))
}

fn select_embedded<'a>(
    languages: &[String],
    tracks: &'a [SubtitleTrack],
) -> Option<&'a SubtitleTrack> {
    let renderable = tracks
        .iter()
        .filter(|track| is_renderable_format(&track.format))
        .collect::<Vec<_>>();
    let defaults = renderable
        .iter()
        .copied()
        .filter(|track| track.is_default)
        .collect::<Vec<_>>();
    if !defaults.is_empty() {
        return preferred_track(languages, &defaults).or_else(|| defaults.first().copied());
    }
    preferred_track(languages, &renderable).or_else(|| renderable.first().copied())
}

pub(super) fn select_embedded_source(
    languages: &[String],
    tracks: &[SubtitleTrack],
) -> Option<SubtitleSource> {
    select_embedded(languages, tracks).map(|track| SubtitleSource::Embedded {
        stream_kind_position: track.stream_kind_position,
    })
}

fn preferred_track<'a>(
    languages: &[String],
    tracks: &[&'a SubtitleTrack],
) -> Option<&'a SubtitleTrack> {
    languages.iter().find_map(|language| {
        let language = normalize_language(language);
        tracks.iter().copied().find(|track| {
            track
                .language
                .as_deref()
                .is_some_and(|candidate| normalize_language(candidate) == language)
        })
    })
}

fn normalize_language(language: &str) -> String {
    language.trim().replace('_', "-").to_ascii_lowercase()
}

fn subtitle_format_rank(extension: &str) -> Option<usize> {
    match extension.to_ascii_lowercase().as_str() {
        "ass" => Some(0),
        "ssa" => Some(1),
        "srt" => Some(2),
        "vtt" => Some(3),
        _ => None,
    }
}

fn is_renderable_format(format: &str) -> bool {
    matches!(
        format.trim().to_ascii_lowercase().as_str(),
        "ass"
            | "ssa"
            | "utf-8"
            | "utf-8 plain text"
            | "subrip"
            | "webvtt"
            | "pgs"
            | "vobsub"
            | "dvb subtitle"
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use crate::media_info::SubtitleTrack;

    use super::{SubtitleSource, select_subtitle};

    fn track(
        position: usize,
        language: Option<&str>,
        is_default: bool,
        format: &str,
    ) -> SubtitleTrack {
        SubtitleTrack {
            stream_kind_position: position,
            language: language.map(str::to_owned),
            is_default,
            format: format.to_owned(),
        }
    }

    #[test]
    fn selects_preferred_language_then_best_external_format() {
        let root = tempdir().unwrap();
        let video = root.path().join("Movie.2026.mkv");
        fs::write(&video, b"video").unwrap();
        fs::write(root.path().join("Movie.2026.en.ass"), b"english").unwrap();
        fs::write(root.path().join("Movie.2026.zh_CN.srt"), b"chinese srt").unwrap();
        fs::write(root.path().join("Movie.2026.ZH-cn.ASS"), b"chinese ass").unwrap();

        let selected = select_subtitle(&video, &["zh-CN".into(), "en".into()], &[]).unwrap();

        assert_eq!(
            selected,
            Some(SubtitleSource::External(
                root.path().join("Movie.2026.ZH-cn.ASS")
            ))
        );
    }

    #[test]
    fn selects_strict_same_name_external_after_language_candidates() {
        let root = tempdir().unwrap();
        let video = root.path().join("Movie.mkv");
        fs::write(&video, b"video").unwrap();
        fs::write(root.path().join("Movie.srt"), b"same").unwrap();
        fs::write(root.path().join("Movie-Trailer.ass"), b"wrong").unwrap();
        fs::write(root.path().join("Movie.extra.ass"), b"wrong").unwrap();

        let selected = select_subtitle(&video, &["zh-CN".into()], &[]).unwrap();

        assert_eq!(
            selected,
            Some(SubtitleSource::External(root.path().join("Movie.srt")))
        );
    }

    #[test]
    fn falls_back_to_preferred_default_embedded_track() {
        let root = tempdir().unwrap();
        let video = root.path().join("Movie.mkv");
        fs::write(&video, b"video").unwrap();
        let tracks = [
            track(1, Some("en"), true, "UTF-8"),
            track(2, Some("zh_CN"), true, "ASS"),
            track(3, Some("zh-CN"), false, "ASS"),
        ];

        let selected = select_subtitle(&video, &["zh-CN".into(), "en".into()], &tracks).unwrap();

        assert_eq!(
            selected,
            Some(SubtitleSource::Embedded {
                stream_kind_position: 2
            })
        );
    }

    #[test]
    fn falls_back_to_language_then_first_renderable_embedded_track() {
        let root = tempdir().unwrap();
        let video = root.path().join("Movie.mkv");
        fs::write(&video, b"video").unwrap();
        let preferred = [
            track(1, Some("en"), false, "UTF-8"),
            track(2, Some("zh"), false, "WebVTT"),
        ];

        assert_eq!(
            select_subtitle(&video, &["zh".into()], &preferred).unwrap(),
            Some(SubtitleSource::Embedded {
                stream_kind_position: 2
            })
        );
        assert_eq!(
            select_subtitle(&video, &["ja".into()], &preferred).unwrap(),
            Some(SubtitleSource::Embedded {
                stream_kind_position: 1
            })
        );
    }

    #[test]
    fn ignores_unsupported_embedded_tracks_and_returns_none() {
        let root = tempdir().unwrap();
        let video = root.path().join("Movie.mkv");
        fs::write(&video, b"video").unwrap();
        let tracks = [track(1, Some("zh"), true, "Unknown")];

        assert_eq!(
            select_subtitle(&video, &["zh".into()], &tracks).unwrap(),
            None
        );
    }

    #[test]
    fn reports_missing_video_parent_instead_of_scanning_another_directory() {
        let error = select_subtitle(&PathBuf::from("Movie.mkv"), &["zh".into()], &[]).unwrap_err();

        assert!(error.to_string().contains("directory"));
    }
}
