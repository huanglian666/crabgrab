use std::str::FromStr;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Tmdb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Movie,
    Tv,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceId {
    pub provider: ProviderKind,
    pub media_type: MediaType,
    pub numeric_id: u64,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("invalid resource ID '{input}'; expected tmdb-movie-550 or tmdb-tv-1399")]
pub struct ResourceIdError {
    input: String,
}

impl FromStr for ResourceId {
    type Err = ResourceIdError;

    /// 将 `<provider>-<media-type>-<numeric-id>` 转换为结构化资源 ID。
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        // 固定为三段格式，让新增提供方时仍能复用统一的分派入口。
        let invalid = || ResourceIdError {
            input: input.to_owned(),
        };
        let mut parts = input.split('-');
        let provider = match parts.next() {
            Some("tmdb") => ProviderKind::Tmdb,
            _ => return Err(invalid()),
        };
        let media_type = match parts.next() {
            Some("movie") => MediaType::Movie,
            Some("tv") => MediaType::Tv,
            _ => return Err(invalid()),
        };
        let numeric_id = parts
            .next()
            .ok_or_else(&invalid)?
            .parse::<u64>()
            .map_err(|_| invalid())?;

        // 零不是有效 ID；第四段及更多内容表示输入超出当前协议。
        if numeric_id == 0 || parts.next().is_some() {
            return Err(invalid());
        }

        Ok(Self {
            provider,
            media_type,
            numeric_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{MediaType, ResourceId};

    #[test]
    fn parses_supported_tmdb_resource_ids() {
        let movie = "tmdb-movie-550".parse::<ResourceId>().unwrap();
        let tv = "tmdb-tv-1399".parse::<ResourceId>().unwrap();

        assert_eq!(movie.media_type, MediaType::Movie);
        assert_eq!(movie.numeric_id, 550);
        assert_eq!(tv.media_type, MediaType::Tv);
        assert_eq!(tv.numeric_id, 1399);
    }

    #[test]
    fn rejects_unsupported_or_malformed_resource_ids() {
        for invalid in [
            "imdb-movie-550",
            "tmdb-show-1",
            "tmdb-movie-0",
            "tmdb-movie-x",
            "tmdb-550",
            "tmdb-movie-1-extra",
        ] {
            assert!(invalid.parse::<ResourceId>().is_err(), "{invalid}");
        }
    }
}
