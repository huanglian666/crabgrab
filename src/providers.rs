use reqwest::Url;
use thiserror::Error;

use crate::domain::{ProviderKind, ResourceId};

mod tmdb;

pub use tmdb::TmdbProvider;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artwork {
    pub background_url: Url,
    pub cover_url: Url,
}

pub trait ArtworkProvider {
    /// 查询资源对应的横屏背景图和竖屏封面下载地址。
    fn artwork(&self, id: &ResourceId) -> Result<Artwork, ProviderError>;
}

pub struct ProviderRegistry {
    tmdb: TmdbProvider,
}

impl ProviderRegistry {
    /// 创建当前已注册图片提供方的集合。
    pub fn new(tmdb: TmdbProvider) -> Self {
        Self { tmdb }
    }

    /// 根据资源 ID 中的提供方类型返回对应策略。
    pub fn provider(&self, kind: ProviderKind) -> &dyn ArtworkProvider {
        // CLI 只依赖注册表；新增 IMDb 或豆瓣策略时在此扩充分派关系。
        match kind {
            ProviderKind::Tmdb => &self.tmdb,
        }
    }
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("TMDB request was unauthorized; check [tmdb].api_token")]
    Unauthorized,
    #[error("TMDB resource was not found")]
    NotFound,
    #[error("TMDB rate limit reached; try again later")]
    RateLimited,
    #[error("TMDB service returned HTTP {0}")]
    HttpStatus(u16),
    #[error("TMDB request failed: {0}")]
    Request(String),
    #[error("TMDB returned invalid JSON: {0}")]
    InvalidResponse(String),
    #[error("TMDB resource has no {0}")]
    MissingImage(&'static str),
    #[error("TMDB image configuration does not support original {0} images")]
    MissingOriginal(&'static str),
    #[error("TMDB returned an invalid image base URL: {0}")]
    InvalidImageBase(String),
}
