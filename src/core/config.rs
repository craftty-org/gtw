use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cache::CACHE_TTL_DAYS;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub network: NetworkConfig,
    pub cache: CacheConfig,
    pub render: RenderConfig,
    pub media: MediaConfig,
    pub effects: EffectsConfig,
    pub theme: ThemeConfig,
    pub keys: KeysConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub offline: bool,
    pub no_animations: bool,
    pub language: String,
    pub restore_session: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub user_agent: String,
    pub timeout_secs: u64,
    pub retries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CacheConfig {
    pub max_size_mb: u64,
    pub ttl_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderConfig {
    pub syntax_highlight: bool,
    pub image_width: u16,
    pub wrap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaConfig {
    pub enabled: bool,
    /// Explicit "no image" mode: skip fetching/decoding/rendering images and
    /// math thumbnails entirely (config.toml-driven).
    pub no_image: bool,
    pub autoplay: bool,
    pub max_megabytes: u64,
    pub image_columns: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectsConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeysConfig {
    /// Jump to next/previous article link (Alt+J / Alt+K).
    pub link_jump: bool,
    /// Show a link peek when jumping to a link.
    pub peek_on_jump: bool,
}

impl Default for KeysConfig {
    fn default() -> Self {
        Self {
            link_jump: true,
            peek_on_jump: true,
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            offline: false,
            no_animations: false,
            language: "en".to_string(),
            restore_session: true,
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            user_agent: format!(
                "gtw/{} (https://github.com/craftty-org/gtw; terminal reader)",
                env!("CARGO_PKG_VERSION")
            ),
            timeout_secs: 30,
            retries: 2,
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_size_mb: 512,
            ttl_days: CACHE_TTL_DAYS,
        }
    }
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            syntax_highlight: true,
            image_width: 72,
            wrap: true,
        }
    }
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            no_image: false,
            autoplay: false,
            max_megabytes: 128,
            image_columns: 48,
        }
    }
}

impl Default for EffectsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            name: "Chadrula".to_string(),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("gtw")
            .join("config.toml")
    }

    pub fn load(explicit: Option<&Path>) -> (Self, Option<PathBuf>) {
        let path = explicit.map(Path::to_path_buf).unwrap_or_else(Self::path);
        let cfg = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str::<Config>(&s).ok())
            .unwrap_or_default();
        (cfg, Some(path))
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, body)
    }

    pub fn ttl_secs(&self) -> i64 {
        i64::from(self.cache.ttl_days.max(1)) * 86_400
    }

    pub fn cache_dir() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("gtw")
    }

    pub fn media_dir() -> PathBuf {
        Self::cache_dir().join("media")
    }

    pub fn sanitized_user_agent(&self) -> String {
        let ua = self.network.user_agent.trim();
        if ua.is_empty() {
            NetworkConfig::default().user_agent
        } else {
            ua.replace(['\r', '\n'], " ")
        }
    }
}
