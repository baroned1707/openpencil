//! The image-search providers as one closed set.
//!
//! An enum rather than a trait object: the set is small and known, every
//! consumer runs on the same blocking-over-one-runtime bridge, and `match`
//! arms keep the per-provider quirks (aspect support, retry stage labels)
//! next to each other instead of hiding them behind dyn-async boxing.

use std::time::Instant;

use op_editor_core::agent_settings::{AgentSettings, ImageTestStatus};
use op_editor_core::ApiKey;

use crate::{ImageAspectRatio, ImageKind};

use super::catalog::{parse_openverse_results, parse_wikimedia_results, RawHit};
use super::iconify::{self, parse_iconify_results};
use super::license::{retain_usable_license, ALLOW_SHARE_ALIKE_DEFAULT};
use super::pixabay::{self, parse_pixabay_results};
use super::unsplash::{self, parse_unsplash_results};
use super::{openverse, wikimedia, WebOpenverseCredentials, SEARCH_CANDIDATE_COUNT};

pub(crate) const OPENVERSE_BASE_URL: &str = "https://api.openverse.org";
pub(crate) const WIKIMEDIA_BASE_URL: &str = "https://commons.wikimedia.org";
pub(crate) const PIXABAY_BASE_URL: &str = "https://pixabay.com";
pub(crate) const UNSPLASH_BASE_URL: &str = "https://api.unsplash.com";
pub(crate) const ICONIFY_BASE_URL: &str = "https://api.iconify.design";

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
    /// Photo (default) or vector / icon; narrows Openverse (`extension=svg`)
    /// and Commons (`filetype:drawing`) and picks the Pixabay `image_type`.
    pub kind: ImageKind,
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
            kind: ImageKind::Photo,
        }
    }

    pub fn with_kind(mut self, kind: ImageKind) -> Self {
        self.kind = kind;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageProvider {
    Openverse {
        base_url: String,
    },
    Wikimedia {
        base_url: String,
    },
    /// Needs the user's own key (`ApiKey` keeps it out of `Debug`).
    Pixabay {
        base_url: String,
        key: ApiKey,
    },
    /// Needs the user's own key AND the Settings opt-in.
    Unsplash {
        base_url: String,
        key: ApiKey,
    },
    /// Icons, keyless; licence per collection.
    Iconify {
        base_url: String,
    },
}

/// The user's stock-provider settings, as the ladders need them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StockKeys {
    pub pixabay: ApiKey,
    pub unsplash: ApiKey,
    /// Unsplash runs only with a key AND this switch.
    pub unsplash_enabled: bool,
}

impl StockKeys {
    pub fn from_settings(settings: &AgentSettings) -> Self {
        Self {
            pixabay: settings.pixabay_api_key.clone(),
            unsplash: settings.unsplash_access_key.clone(),
            unsplash_enabled: settings.unsplash_enabled,
        }
    }
}

/// The ladder for one kind of slot.
///
/// Photo: Pixabay (key set) -> Unsplash (key set AND opted in) -> Openverse
/// -> Wikimedia. Vector: Iconify -> Openverse (SVG only) -> Commons (SVG
/// only). A provider whose key is missing is simply absent, so with no keys
/// the photo ladder is exactly [`default_providers`].
pub fn providers_for(kind: ImageKind, keys: &StockKeys) -> Vec<ImageProvider> {
    let mut providers = Vec::new();
    match kind {
        ImageKind::Photo => {
            if !keys.pixabay.is_blank() {
                providers.push(ImageProvider::pixabay(keys.pixabay.clone()));
            }
            if keys.unsplash_enabled && !keys.unsplash.is_blank() {
                providers.push(ImageProvider::unsplash(keys.unsplash.clone()));
            }
        }
        ImageKind::Vector => providers.push(ImageProvider::iconify()),
    }
    providers.push(ImageProvider::openverse());
    providers.push(ImageProvider::wikimedia());
    providers
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
    providers_for(ImageKind::Photo, &StockKeys::default())
}

impl ImageProvider {
    pub fn openverse() -> Self {
        Self::openverse_at(OPENVERSE_BASE_URL)
    }

    pub fn wikimedia() -> Self {
        Self::wikimedia_at(WIKIMEDIA_BASE_URL)
    }

    pub fn pixabay(key: ApiKey) -> Self {
        Self::pixabay_at(PIXABAY_BASE_URL, key)
    }

    pub fn unsplash(key: ApiKey) -> Self {
        Self::unsplash_at(UNSPLASH_BASE_URL, key)
    }

    pub fn iconify() -> Self {
        Self::iconify_at(ICONIFY_BASE_URL)
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

    pub fn pixabay_at(base_url: &str, key: ApiKey) -> Self {
        Self::Pixabay {
            base_url: base_url.trim_end_matches('/').to_string(),
            key,
        }
    }

    pub fn unsplash_at(base_url: &str, key: ApiKey) -> Self {
        Self::Unsplash {
            base_url: base_url.trim_end_matches('/').to_string(),
            key,
        }
    }

    pub fn iconify_at(base_url: &str) -> Self {
        Self::Iconify {
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Stage name used in logs, `ResolvedImage::provider` and the popover's
    /// `source` field.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Openverse { .. } => "openverse",
            Self::Wikimedia { .. } => "wikimedia",
            Self::Pixabay { .. } => "pixabay",
            Self::Unsplash { .. } => "unsplash",
            Self::Iconify { .. } => "iconify",
        }
    }

    pub fn kind(&self) -> ProviderKind {
        match self {
            Self::Openverse { .. } | Self::Wikimedia { .. } | Self::Iconify { .. } => {
                ProviderKind::OpenCatalogue
            }
            Self::Pixabay { .. } | Self::Unsplash { .. } => ProviderKind::Stock,
        }
    }

    /// Whether the provider is unusable without a key. Openverse takes an
    /// OAuth client for higher limits but answers anonymous requests, and
    /// Commons and Iconify need nothing, so none of them requires one.
    pub fn needs_key(&self) -> bool {
        match self {
            Self::Openverse { .. } | Self::Wikimedia { .. } | Self::Iconify { .. } => false,
            Self::Pixabay { .. } | Self::Unsplash { .. } => true,
        }
    }

    /// Whether hits go through the lexical relevance fence. Iconify ranks by
    /// icon name itself, and an icon name is too short to satisfy the
    /// multi-word subject rule.
    pub(crate) fn fenced_by_relevance(&self) -> bool {
        !matches!(self, Self::Iconify { .. })
    }

    /// Whether the catalogue can filter by aspect ratio / orientation.
    pub(crate) fn supports_aspect(&self) -> bool {
        matches!(
            self,
            Self::Openverse { .. } | Self::Pixabay { .. } | Self::Unsplash { .. }
        )
    }

    /// Stage label of a list produced by the two-keyword retry. Openverse
    /// labels it (`openverse-retry`, with the retry query); Wikimedia has
    /// always reported the primary stage, so its label stays put.
    pub(crate) fn retry_stage(&self) -> Option<&'static str> {
        match self {
            Self::Openverse { .. } => Some("openverse-retry"),
            Self::Pixabay { .. } => Some("pixabay-retry"),
            Self::Unsplash { .. } => Some("unsplash-retry"),
            Self::Wikimedia { .. } | Self::Iconify { .. } => None,
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
                    ImageKind::Photo,
                )
                .await
                .is_some_and(|json| json.get("results").is_some_and(serde_json::Value::is_array))
            }
            Self::Wikimedia { base_url } => matches!(
                wikimedia::fetch_pages_json(probe.client, base_url, "test", probe.page_size).await,
                Ok(Some(_))
            ),
            Self::Pixabay { base_url, key } => pixabay::probe(probe.client, base_url, key).await,
            Self::Unsplash { base_url, key } => unsplash::probe(probe.client, base_url, key).await,
            Self::Iconify { base_url } => iconify::probe(probe.client, base_url).await,
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
        let now = Instant::now();
        let hits = match self {
            Self::Openverse { base_url } => {
                let json = openverse::fetch_list_json(
                    ctx.client,
                    base_url,
                    query,
                    aspect,
                    ctx.page_size,
                    ctx.credentials,
                    ctx.kind,
                )
                .await
                .ok_or(SearchError::RequestFailed)?;
                parse_openverse_results(&json)
            }
            Self::Wikimedia { base_url } => {
                // Commons has no aspect filter; the argument is ignored. A
                // vector request narrows the search to SVG drawings.
                let query = match ctx.kind {
                    ImageKind::Photo => query.to_string(),
                    ImageKind::Vector => format!("{query} filetype:drawing"),
                };
                let json = wikimedia::fetch_pages_json(ctx.client, base_url, &query, ctx.page_size)
                    .await?;
                json.map(|json| parse_wikimedia_results(&json))
                    .unwrap_or_default()
            }
            Self::Pixabay { base_url, key } => {
                let json = pixabay::fetch_list_json(
                    ctx.client,
                    &pixabay::runtime(base_url),
                    base_url,
                    key,
                    query,
                    ctx.kind,
                    aspect,
                    ctx.page_size,
                    now,
                )
                .await?;
                parse_pixabay_results(&json)
            }
            Self::Unsplash { base_url, key } => {
                let json = unsplash::fetch_list_json(
                    ctx.client,
                    &unsplash::runtime(base_url),
                    base_url,
                    key,
                    query,
                    aspect,
                    ctx.page_size,
                    now,
                )
                .await?;
                parse_unsplash_results(&json, base_url)
            }
            Self::Iconify { base_url } => {
                let json =
                    iconify::fetch_search_json(ctx.client, base_url, query, ctx.page_size).await?;
                parse_iconify_results(&json, base_url)
            }
        };
        let mut hits = retain_usable_license(hits, ctx.allow_share_alike);
        if matches!(self, Self::Iconify { .. }) {
            hits.truncate(ctx.page_size.max(1));
        }
        Ok(hits)
    }
}
