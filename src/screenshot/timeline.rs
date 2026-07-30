use std::collections::BTreeSet;

use rand::{Rng, RngExt};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimestampSpec {
    Absolute(u64),
    Percent(f64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeline {
    pub points_ms: Vec<u64>,
    pub duplicate_count: usize,
    pub expanded_beyond_count: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TimelineError {
    #[error("invalid screenshot timestamp: {0}")]
    InvalidTimestamp(String),
    #[error("screenshot timestamp {timestamp} is outside video duration")]
    OutOfRange { timestamp: String },
    #[error("video is too short for the requested screenshot count")]
    InsufficientRange,
}

pub fn parse_timestamp(value: &str) -> Result<TimestampSpec, TimelineError> {
    let value = value.trim();
    if let Some(percent) = value.strip_suffix('%') {
        let percent = percent
            .parse::<f64>()
            .ok()
            .filter(|percent| percent.is_finite() && *percent > 0.0 && *percent < 100.0)
            .ok_or_else(|| TimelineError::InvalidTimestamp(value.to_owned()))?;
        return Ok(TimestampSpec::Percent(percent));
    }

    let mut parts = value.split(':');
    let hours = parse_component(parts.next(), value)?;
    let minutes = parse_component(parts.next(), value)?;
    let seconds_part = parts
        .next()
        .ok_or_else(|| TimelineError::InvalidTimestamp(value.to_owned()))?;
    if parts.next().is_some() || minutes >= 60 {
        return Err(TimelineError::InvalidTimestamp(value.to_owned()));
    }
    let (seconds, milliseconds) = match seconds_part.split_once('.') {
        Some((seconds, fraction_text)) if !fraction_text.is_empty() && fraction_text.len() <= 3 => {
            let seconds = seconds
                .parse::<u64>()
                .map_err(|_| TimelineError::InvalidTimestamp(value.to_owned()))?;
            let fraction = fraction_text
                .parse::<u64>()
                .map_err(|_| TimelineError::InvalidTimestamp(value.to_owned()))?;
            let milliseconds = fraction * 10_u64.pow(3 - fraction_text.len() as u32);
            (seconds, milliseconds)
        }
        Some(_) => return Err(TimelineError::InvalidTimestamp(value.to_owned())),
        None => (
            seconds_part
                .parse::<u64>()
                .map_err(|_| TimelineError::InvalidTimestamp(value.to_owned()))?,
            0,
        ),
    };
    if seconds >= 60 {
        return Err(TimelineError::InvalidTimestamp(value.to_owned()));
    }
    let total = hours
        .checked_mul(3_600_000)
        .and_then(|total| total.checked_add(minutes * 60_000))
        .and_then(|total| total.checked_add(seconds * 1000))
        .and_then(|total| total.checked_add(milliseconds))
        .filter(|total| *total > 0)
        .ok_or_else(|| TimelineError::InvalidTimestamp(value.to_owned()))?;
    Ok(TimestampSpec::Absolute(total))
}

fn parse_component(component: Option<&str>, original: &str) -> Result<u64, TimelineError> {
    component
        .filter(|component| !component.is_empty())
        .and_then(|component| component.parse::<u64>().ok())
        .ok_or_else(|| TimelineError::InvalidTimestamp(original.to_owned()))
}

pub fn build_timeline<R: Rng + ?Sized>(
    duration_ms: u64,
    count: usize,
    configured: &[String],
    rng: &mut R,
) -> Result<Timeline, TimelineError> {
    if duration_ms < 2 {
        return Err(TimelineError::InsufficientRange);
    }
    let mut points = BTreeSet::new();
    let mut duplicate_count = 0;
    for value in configured {
        let point = match parse_timestamp(value)? {
            TimestampSpec::Absolute(milliseconds) => milliseconds,
            TimestampSpec::Percent(percent) => {
                ((duration_ms as f64) * percent / 100.0).round() as u64
            }
        };
        if point == 0 || point >= duration_ms {
            return Err(TimelineError::OutOfRange {
                timestamp: value.clone(),
            });
        }
        if !points.insert(point) {
            duplicate_count += 1;
        }
    }

    let expanded_beyond_count = points.len() > count;
    let random_needed = count.saturating_sub(points.len());
    if random_needed > 0 {
        fill_random_points(duration_ms, random_needed, &mut points, rng)?;
    }

    Ok(Timeline {
        points_ms: points.into_iter().collect(),
        duplicate_count,
        expanded_beyond_count,
    })
}

fn fill_random_points<R: Rng + ?Sized>(
    duration_ms: u64,
    needed: usize,
    points: &mut BTreeSet<u64>,
    rng: &mut R,
) -> Result<(), TimelineError> {
    let safe_start = (duration_ms / 20).max(1);
    let safe_end = (duration_ms.saturating_mul(19) / 20).min(duration_ms - 1);
    if safe_end <= safe_start || safe_end - safe_start < needed as u64 {
        return Err(TimelineError::InsufficientRange);
    }
    let span = safe_end - safe_start;
    let min_gap = (duration_ms / 100).clamp(1, 10_000);

    for index in 0..needed {
        let start = safe_start + span * index as u64 / needed as u64;
        let end = safe_start + span * (index + 1) as u64 / needed as u64;
        let upper = end.max(start + 1).min(safe_end + 1);
        let mut selected = None;
        for _ in 0..64 {
            let candidate = rng.random_range(start..upper);
            if points
                .iter()
                .all(|existing| existing.abs_diff(candidate) >= min_gap)
            {
                selected = Some(candidate);
                break;
            }
        }
        if selected.is_none() {
            selected = (start..upper).find(|candidate| !points.contains(candidate));
        }
        points.insert(selected.ok_or(TimelineError::InsufficientRange)?);
    }
    Ok(())
}

pub fn format_timestamp(milliseconds: u64) -> String {
    let total_seconds = milliseconds / 1000;
    let hours = total_seconds / 3600;
    let minutes = total_seconds % 3600 / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::{TimestampSpec, build_timeline, format_timestamp, parse_timestamp};

    #[test]
    fn parses_absolute_millisecond_and_percent_timestamps() {
        assert_eq!(
            parse_timestamp("01:02:03").unwrap(),
            TimestampSpec::Absolute(3_723_000)
        );
        assert_eq!(
            parse_timestamp("25:02:03.456").unwrap(),
            TimestampSpec::Absolute(90_123_456)
        );
        assert_eq!(
            parse_timestamp("65.5%").unwrap(),
            TimestampSpec::Percent(65.5)
        );
    }

    #[test]
    fn rejects_invalid_timestamp_components_and_percent_boundaries() {
        for value in [
            "01:60:00",
            "01:00:60",
            "-01:00:00",
            "00:00:00",
            "0%",
            "100%",
            "not-a-time",
        ] {
            assert!(parse_timestamp(value).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn preserves_explicit_points_and_randomly_fills_target_count() {
        let mut rng = StdRng::seed_from_u64(7);

        let timeline =
            build_timeline(100_000, 5, &["00:00:10".into(), "65%".into()], &mut rng).unwrap();

        assert_eq!(timeline.points_ms.len(), 5);
        assert!(timeline.points_ms.contains(&10_000));
        assert!(timeline.points_ms.contains(&65_000));
        assert!(timeline.points_ms.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(!timeline.expanded_beyond_count);
    }

    #[test]
    fn keeps_all_unique_explicit_points_when_they_exceed_count() {
        let mut rng = StdRng::seed_from_u64(9);

        let timeline = build_timeline(
            100_000,
            2,
            &["10%".into(), "10%".into(), "20%".into(), "30%".into()],
            &mut rng,
        )
        .unwrap();

        assert_eq!(timeline.points_ms, [10_000, 20_000, 30_000]);
        assert_eq!(timeline.duplicate_count, 1);
        assert!(timeline.expanded_beyond_count);
    }

    #[test]
    fn divides_default_random_points_across_safe_middle_region() {
        let mut rng = StdRng::seed_from_u64(11);

        let timeline = build_timeline(100_000, 3, &[], &mut rng).unwrap();

        assert_eq!(timeline.points_ms.len(), 3);
        assert!((5_000..35_000).contains(&timeline.points_ms[0]));
        assert!((35_000..65_000).contains(&timeline.points_ms[1]));
        assert!((65_000..95_000).contains(&timeline.points_ms[2]));
    }

    #[test]
    fn rejects_explicit_points_at_or_beyond_duration() {
        let mut rng = StdRng::seed_from_u64(13);

        assert!(build_timeline(10_000, 3, &["00:00:10".into()], &mut rng).is_err());
    }

    #[test]
    fn formats_public_timestamps_without_milliseconds() {
        assert_eq!(format_timestamp(3_723_999), "01:02:03");
        assert_eq!(format_timestamp(90_123_456), "25:02:03");
    }
}
