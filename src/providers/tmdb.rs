use std::time::Duration;

use reqwest::Url;
use reqwest::blocking::{Client, Response};
use reqwest::redirect::Policy;
use serde::Deserialize;

use crate::config::{SecretToken, TmdbConfig};
use crate::domain::{MediaType, ResourceId};

use super::{Artwork, ArtworkProvider, ProviderError};

pub struct TmdbProvider {
    client: Client,
    api_base: Url,
    token: SecretToken,
    language: String,
}

impl TmdbProvider {
    /// 使用用户配置创建连接 TMDB 官方 API 的提供方。
    pub fn new(config: TmdbConfig) -> Result<Self, ProviderError> {
        let api_base = Url::parse("https://api.themoviedb.org/3/")
            .map_err(|error| ProviderError::Request(error.to_string()))?;
        Self::with_api_base(config, api_base)
    }

    pub(crate) fn with_api_base(config: TmdbConfig, api_base: Url) -> Result<Self, ProviderError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(Policy::limited(5))
            .build()
            .map_err(|error| ProviderError::Request(error.to_string()))?;
        Ok(Self {
            client,
            api_base,
            token: config.api_token,
            language: config.language,
        })
    }

    #[cfg(test)]
    fn for_test(client: Client, api_base: Url, token: &str, language: &str) -> Self {
        Self {
            client,
            api_base,
            token: SecretToken::new(token),
            language: language.to_owned(),
        }
    }

    fn get(&self, url: Url) -> Result<Response, ProviderError> {
        // 统一添加 Bearer Token，并在响应进入业务解析前完成状态分类。
        let response = self
            .client
            .get(url)
            .bearer_auth(self.token.expose_for_request())
            .send()
            .map_err(|error| ProviderError::Request(error.to_string()))?;
        // 在解析响应体前分类状态码，避免错误信息携带服务端敏感内容。
        match response.status().as_u16() {
            200..=299 => Ok(response),
            401 => Err(ProviderError::Unauthorized),
            404 => Err(ProviderError::NotFound),
            429 => Err(ProviderError::RateLimited),
            status => Err(ProviderError::HttpStatus(status)),
        }
    }

    fn json<T: for<'de> Deserialize<'de>>(&self, url: Url) -> Result<T, ProviderError> {
        // 所有 JSON 请求共用认证、状态校验和安全的解析错误映射。
        self.get(url)?
            .json::<T>()
            .map_err(|error| ProviderError::InvalidResponse(error.to_string()))
    }
}

#[derive(Deserialize)]
struct ConfigurationResponse {
    images: ImageConfiguration,
}

#[derive(Deserialize)]
struct ImageConfiguration {
    secure_base_url: String,
    poster_sizes: Vec<String>,
    backdrop_sizes: Vec<String>,
}

#[derive(Deserialize)]
struct DetailsResponse {
    backdrop_path: Option<String>,
    poster_path: Option<String>,
}

impl ArtworkProvider for TmdbProvider {
    /// 查询详情和图片配置，并生成两张 `original` 图片的完整地址。
    fn artwork(&self, id: &ResourceId) -> Result<Artwork, ProviderError> {
        // 图片基础地址和可用尺寸来自 TMDB 配置接口，避免硬编码 CDN 细节。
        let configuration_url = self
            .api_base
            .join("configuration")
            .map_err(|error| ProviderError::Request(error.to_string()))?;
        let configuration: ConfigurationResponse = self.json(configuration_url)?;
        if !configuration
            .images
            .backdrop_sizes
            .iter()
            .any(|size| size == "original")
        {
            // 不静默降级到较小尺寸，确保输出满足 original 约定。
            return Err(ProviderError::MissingOriginal("background"));
        }
        if !configuration
            .images
            .poster_sizes
            .iter()
            .any(|size| size == "original")
        {
            return Err(ProviderError::MissingOriginal("cover"));
        }

        let detail_path = match id.media_type {
            // movie 与 TV 属于不同命名空间，资源 ID 已显式携带媒体类型。
            MediaType::Movie => format!("movie/{}", id.numeric_id),
            MediaType::Tv => format!("tv/{}", id.numeric_id),
        };
        let mut details_url = self
            .api_base
            .join(&detail_path)
            .map_err(|error| ProviderError::Request(error.to_string()))?;
        details_url
            .query_pairs_mut()
            .append_pair("language", &self.language);
        let details: DetailsResponse = self.json(details_url)?;
        // backdrop 与 poster 构成一个完整结果，任一缺失都中止下载。
        let backdrop = details
            .backdrop_path
            .ok_or(ProviderError::MissingImage("background"))?;
        let poster = details
            .poster_path
            .ok_or(ProviderError::MissingImage("cover"))?;
        let image_base = Url::parse(&configuration.images.secure_base_url)
            .map_err(|error| ProviderError::InvalidImageBase(error.to_string()))?;

        Ok(Artwork {
            // 去除开头斜杠，防止 URL join 覆盖 `/t/p/` 图片路径前缀。
            background_url: image_base
                .join(&format!("original/{}", backdrop.trim_start_matches('/')))
                .map_err(|error| ProviderError::InvalidImageBase(error.to_string()))?,
            cover_url: image_base
                .join(&format!("original/{}", poster.trim_start_matches('/')))
                .map_err(|error| ProviderError::InvalidImageBase(error.to_string()))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use httpmock::Method::GET;
    use httpmock::MockServer;
    use reqwest::Url;

    use crate::domain::ResourceId;

    use super::ArtworkProvider;

    use super::TmdbProvider;

    fn provider(server: &MockServer) -> TmdbProvider {
        TmdbProvider::for_test(
            reqwest::blocking::Client::new(),
            Url::parse(&format!("{}/3/", server.base_url())).unwrap(),
            "test-secret",
            "zh-CN",
        )
    }

    fn mock_configuration(server: &MockServer) {
        server.mock(|when, then| {
            when.method(GET)
                .path("/3/configuration")
                .header("authorization", "Bearer test-secret");
            then.status(200).json_body_obj(&serde_json::json!({
                "images": {
                    "secure_base_url": format!("{}/t/p/", server.base_url()),
                    "poster_sizes": ["w500", "original"],
                    "backdrop_sizes": ["w1280", "original"]
                }
            }));
        });
    }

    #[test]
    fn fetches_movie_artwork_with_language_and_original_size() {
        let server = MockServer::start();
        mock_configuration(&server);
        server.mock(|when, then| {
            when.method(GET)
                .path("/3/movie/550")
                .query_param("language", "zh-CN")
                .header("authorization", "Bearer test-secret");
            then.status(200).json_body_obj(&serde_json::json!({
                "backdrop_path": "/backdrop.jpg",
                "poster_path": "/poster.jpg"
            }));
        });

        let artwork = provider(&server)
            .artwork(&"tmdb-movie-550".parse::<ResourceId>().unwrap())
            .unwrap();

        assert_eq!(
            artwork.background_url.as_str(),
            format!("{}/t/p/original/backdrop.jpg", server.base_url())
        );
        assert_eq!(
            artwork.cover_url.as_str(),
            format!("{}/t/p/original/poster.jpg", server.base_url())
        );
    }

    #[test]
    fn fetches_tv_artwork_from_tv_endpoint() {
        let server = MockServer::start();
        mock_configuration(&server);
        let details = server.mock(|when, then| {
            when.method(GET).path("/3/tv/1399");
            then.status(200).json_body_obj(&serde_json::json!({
                "backdrop_path": "/tv-backdrop.jpg",
                "poster_path": "/tv-poster.jpg"
            }));
        });

        provider(&server)
            .artwork(&"tmdb-tv-1399".parse::<ResourceId>().unwrap())
            .unwrap();

        details.assert();
    }

    #[test]
    fn classifies_errors_and_never_exposes_token() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/3/configuration");
            then.status(401);
        });

        let error = provider(&server)
            .artwork(&"tmdb-movie-550".parse::<ResourceId>().unwrap())
            .unwrap_err();
        let visible = format!("{error:?} {error}");

        assert!(visible.contains("unauthorized"));
        assert!(!visible.contains("test-secret"));
    }

    #[test]
    fn classifies_not_found_rate_limit_and_service_errors() {
        for (status, expected) in [(404, "not found"), (429, "rate limit"), (500, "HTTP 500")] {
            let server = MockServer::start();
            server.mock(|when, then| {
                when.method(GET).path("/3/configuration");
                then.status(status);
            });

            let error = provider(&server)
                .artwork(&"tmdb-movie-550".parse::<ResourceId>().unwrap())
                .unwrap_err()
                .to_string();

            assert!(error.contains(expected), "{status}: {error}");
        }
    }

    #[test]
    fn rejects_missing_images_and_missing_original_size() {
        let server = MockServer::start();
        mock_configuration(&server);
        server.mock(|when, then| {
            when.method(GET).path("/3/movie/550");
            then.status(200).json_body_obj(&serde_json::json!({
                "backdrop_path": "/background.jpg",
                "poster_path": null
            }));
        });
        let missing_cover = provider(&server)
            .artwork(&"tmdb-movie-550".parse::<ResourceId>().unwrap())
            .unwrap_err()
            .to_string();
        assert!(missing_cover.contains("cover"));

        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/3/configuration");
            then.status(200).json_body_obj(&serde_json::json!({
                "images": {
                    "secure_base_url": format!("{}/t/p/", server.base_url()),
                    "poster_sizes": ["w500"],
                    "backdrop_sizes": ["w1280", "original"]
                }
            }));
        });
        let missing_original = provider(&server)
            .artwork(&"tmdb-movie-550".parse::<ResourceId>().unwrap())
            .unwrap_err()
            .to_string();
        assert!(missing_original.contains("original cover"));
    }
}
