use std::path::Path;

use serde::Deserialize;

use super::AnalyzeError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaProbe {
    pub duration_ms: u64,
    pub has_video: bool,
    pub subtitles: Vec<SubtitleTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleTrack {
    pub stream_kind_position: usize,
    pub language: Option<String>,
    pub is_default: bool,
    pub format: String,
}

pub trait MediaProber {
    fn probe(&self, input: &Path) -> Result<MediaProbe, AnalyzeError>;
}

#[derive(Deserialize)]
struct Root {
    media: Media,
}

#[derive(Deserialize)]
struct Media {
    #[serde(default)]
    track: Vec<Track>,
}

#[derive(Deserialize)]
struct Track {
    #[serde(rename = "@type")]
    kind: String,
    #[serde(rename = "Duration")]
    duration: Option<String>,
    #[serde(rename = "StreamKindPos")]
    stream_kind_position: Option<String>,
    #[serde(rename = "Language")]
    language: Option<String>,
    #[serde(rename = "Default")]
    default: Option<String>,
    #[serde(rename = "Format")]
    format: Option<String>,
}

pub fn parse_probe_json(json: &str) -> Result<MediaProbe, AnalyzeError> {
    let root: Root = serde_json::from_str(json)
        .map_err(|_| AnalyzeError::Failed("MediaInfo returned invalid JSON".into()))?;
    let has_video = root.media.track.iter().any(|track| track.kind == "Video");
    if !has_video {
        return Err(AnalyzeError::Failed(
            "MediaInfo found no video track".into(),
        ));
    }

    let duration_seconds = root
        .media
        .track
        .iter()
        .find(|track| track.kind == "General")
        .and_then(|track| track.duration.as_deref())
        .and_then(|duration| duration.parse::<f64>().ok())
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .ok_or_else(|| AnalyzeError::Failed("MediaInfo returned no valid duration".into()))?;
    let duration_ms = (duration_seconds * 1000.0).round() as u64;
    if duration_ms == 0 {
        return Err(AnalyzeError::Failed(
            "MediaInfo returned no valid duration".into(),
        ));
    }

    let subtitles = root
        .media
        .track
        .into_iter()
        .filter(|track| track.kind == "Text")
        .enumerate()
        .map(|(index, track)| SubtitleTrack {
            stream_kind_position: track
                .stream_kind_position
                .as_deref()
                .and_then(|position| position.parse().ok())
                .unwrap_or(index + 1),
            language: track
                .language
                .filter(|language| !language.trim().is_empty()),
            is_default: track
                .default
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case("yes")),
            format: track.format.unwrap_or_default(),
        })
        .collect();

    Ok(MediaProbe {
        duration_ms,
        has_video,
        subtitles,
    })
}
