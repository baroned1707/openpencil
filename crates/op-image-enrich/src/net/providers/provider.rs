//! The image-search providers as one closed set.
//!
//! An enum rather than a trait object: the set is small and known, every
//! consumer runs on the same blocking-over-one-runtime bridge, and `match`
//! arms keep the per-provider quirks (aspect support, retry stage labels)
//! next to each other instead of hiding them behind dyn-async boxing.

use op_editor_core::agent_settings::ImageTestStatus;

use crate::ImageAspectRatio;

use super::catalog::{parse_openverse_results, parse_wikimedia_results, RawHit};
use super::license::{retain_usable_license, ALLOW_SHARE_ALIKE_DEFAULT};
use super::{openverse, wikimedia, WebOpenverseCredentials, SEARCH_CANDIDATE_COUNT};

pub(crate) const OPENVERSE_BASE_URL: &str = "https://api.openverse.org";
pub(crate) const WIKIMEDIA_BASE_URL: &str = "https://commons.wikimedia.org";

/// What a provider is, for UI copy and policy decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// A catalogue of openly licensed works (Openverse, Wikimedia Commons).
    OpenCatalogue,
    /// A stock-photo service with its own licence terms.
    Stock,
}

/// Why a provider produced no list at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchError {
    /// Transport failure, HTTP error, or an unreadable body.
    RequestFailed,
    /// Pacing refused to send (daily quota spent, or repeated HTTP 429).
    Skipped,
}

/// Everything a provider needs to run one query besides the query itself.
pub struct ProviderCtx<'a> {
    pub client: &'a reqwest::Client,
    pub credentials: Option<&'a WebOpenverseCredentials>,
    /// Candidate window requested from the catalogue.
    pub page_size: usize,
    /// See [`ALLOW_SHARE_ALIKE_DEFAULT`].
    pub allow_share_alike: bool,
}

impl<'a> ProviderCtx<'a> {
    pub fn new(
        client: &'a reqwest::Client,
        credentials: Option<&'a WebOpenverseCredentials>,
    ) -> Self {
        Self {
            client,
            credentials,
            page_size: SEARCH_CANDIDATE_COUNT,
            allow_share_alike: ALLOW_SHARE_ALIKE_DEFAULT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageProvider {
    Openverse { base_url: String },
    Wikimedia { base_url: String },
}

/// Run the Settings test through the FIRST provider of `providers`.
pub async fn test_first_provider(
    client: &reqwest::Client,
    providers: &[ImageProvider],
    credentials: Option<&WebOpenverseCredentials>,
) -> ImageTestStatus {
    let Some(provider) = providers.first() else {
        return ImageTestStatus::Invalid;
    };
    provider
        .test_connection(&ProviderCtx::new(client, credentials))
        .await
}

/// The production ladder order: Openverse first, Wikimedia Commons second.
pub fn default_providers() -> Vec<ImageProvider> {
    vec![ImageProvider::openverse(), ImageProvider::wikimedia()]
}

impl ImageProvider {
    pub fn openverse() -> Self {
        Self::openverse_at(OPENVERSE_BASE_URL)
    }

    pub fn wikimedia() -> Self {
        Self::wikimedia_at(WIKIMEDIA_BASE_URL)
    }

    /// Point at another host (tests use a loopback fake).
    pub fn openverse_at(base_url: &str) -> Self {
        Self::Openverse {
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    pub fn wikimedia_at(base_url: &str) -> Self {
        Self::Wikimedia {
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Stage name used in logs, `ResolvedImage::provider` and the popover's
    /// `source` field.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Openverse { .. } => "openverse",
            Self::Wikimedia { .. } => "wikimedia",
        }
    }

    pub fn kind(&self) -> ProviderKind {
        match self {
            Self::Openverse { .. } | Self::Wikimedia { .. } => ProviderKind::OpenCatalogue,
        }
    }

    /// Whether the provider is unusable without a key. Openverse takes an
    /// OAuth client for higher limits but answers anonymous requests, and
    /// Commons needs nothing, so neither requires one.
    pub fn needs_key(&self) -> bool {
        match self {
            Self::Openverse { .. } | Self::Wikimedia { .. } => false,
        }
    }

    /// Whether the catalogue can filter by aspect ratio.
    pub(crate) fn supports_aspect(&self) -> bool {
        matches!(self, Self::Openverse { .. })
    }

    /// Stage label of a list produced by the two-keyword retry. Openverse
    /// labels it (`openverse-retry`, with the retry query); Wikimedia has
    /// always reported the primary stage, so its label stays put.
    pub(crate) fn retry_stage(&self) -> Option<&'static str> {
        match self {
            Self::Openverse { .. } => Some("openverse-retry"),
            Self::Wikimedia { .. } => None,
        }
    }

    /// Settings > Images "Test": one tiny real query (`q=test`, one result)
    /// answered as Valid or Invalid.
    ///
    /// Openverse with credentials must obtain a token FIRST: the normal
    /// request path silently falls back to anonymous when the token request
    /// fails, which would show Valid for a wrong secret.
    pub async fn test_connection(&self, ctx: &ProviderCtx<'_>) -> ImageTestStatus {
        let probe = ProviderCtx {
            page_size: 1,
            ..ProviderCtx::new(ctx.client, ctx.credentials)
        };
        let reachable = match self {
            Self::Openverse { base_url } => {
                if let Some(credentials) = probe.credentials {
                    if openverse::fetch_token(probe.client, base_url, credentials)
                        .await
                        .is_none()
                    {
                        return ImageTestStatus::Invalid;
                    }
                }
                openverse::fetch_list_json(
                    probe.client,
                    base_url,
                    "test",
                    None,
                    probe.page_size,
                    probe.credentials,
                )
                .await
                .is_some_and(|json| json.get("results").is_some_and(serde_json::Value::is_array))
            }
            Self::Wikimedia { base_url } => matches!(
                wikimedia::fetch_pages_json(probe.client, base_url, "test", probe.page_size).await,
                Ok(Some(_))
            ),
        };
        if reachable {
            ImageTestStatus::Valid
        } else {
            ImageTestStatus::Invalid
        }
    }

    /// One catalogue query, licence-filtered BEFORE any relevance logic sees
    /// the hits. Openverse failures are `Err`; Wikimedia swallows transport
    /// errors into an empty list (its historical behaviour) and only errors
    /// when pacing skips it.
    pub async fn search(
        &self,
        ctx: &ProviderCtx<'_>,
        query: &str,
        aspect: Option<ImageAspectRatio>,
    ) -> Result<Vec<RawHit>, SearchError> {
        let hits = match self {
            Self::Openverse { base_url } => {
                let json = openverse::fetch_list_json(
                    ctx.client,
                    base_url,
                    query,
                    aspect,
                    ctx.page_size,
                    ctx.credentials,
                )
                .await
                .ok_or(SearchError::RequestFailed)?;
                parse_openverse_results(&json)
            }
            Self::Wikimedia { base_url } => {
                // Commons has no aspect filter; the argument is ignored.
                let json =
                    wikimedia::fetch_pages_json(ctx.client, base_url, query, ctx.page_size).await?;
                json.map(|json| parse_wikimedia_results(&json))
                    .unwrap_or_default()
            }
        };
        Ok(retain_usable_license(hits, ctx.allow_share_alike))
    }
}
