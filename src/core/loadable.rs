//! Load-state machine with the full loading/error surface used by all screens.
//! Some transition helpers are retained for later editors and diagnostics.
#![allow(dead_code)]
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    Interactive,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    Network,
    Cache,
    Prefetch,
    Local,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Self::Network => "live",
            Self::Cache => "cached",
            Self::Prefetch => "prefetched",
            Self::Local => "local",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgressUnit {
    Bytes,
    Items,
    Frames,
    Percent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    pub current: u64,
    pub total: Option<u64>,
    pub unit: ProgressUnit,
    pub label: String,
}

impl Progress {
    pub fn fraction(&self) -> Option<f32> {
        self.total
            .filter(|t| *t > 0)
            .map(|total| (self.current as f32 / total as f32).clamp(0.0, 1.0))
    }

    pub fn percent(&self) -> Option<u8> {
        self.fraction().map(|f| (f * 100.0).round() as u8)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadStage {
    Connecting,
    Downloading,
    Parsing,
    Extracting,
    LayingOut,
    Decoding,
    Buffering,
    Indexing,
    Saving,
    Committing,
}

impl LoadStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Downloading => "downloading",
            Self::Parsing => "parsing",
            Self::Extracting => "extracting",
            Self::LayingOut => "laying out",
            Self::Decoding => "decoding",
            Self::Buffering => "buffering",
            Self::Indexing => "indexing",
            Self::Saving => "saving",
            Self::Committing => "saving",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CancelHandle(pub CancellationToken);

impl Default for CancelHandle {
    fn default() -> Self {
        Self(CancellationToken::new())
    }
}

#[derive(Debug, Clone)]
pub enum Fallback {
    None,
    Placeholder,
    External,
    Cached,
}

#[derive(Debug, Clone)]
pub struct LoadError {
    pub message: String,
    pub retryable: bool,
    pub kind: LoadErrorKind,
}

impl LoadError {
    pub fn new(kind: LoadErrorKind, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: !matches!(kind, LoadErrorKind::NotFound | LoadErrorKind::Cancelled),
            kind,
        }
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(LoadErrorKind::Network, message)
    }

    pub fn parse(message: impl Into<String>) -> Self {
        Self::new(LoadErrorKind::Parse, message)
    }

    pub fn offline() -> Self {
        Self {
            message: "offline — no cached copy is available".to_string(),
            retryable: true,
            kind: LoadErrorKind::Offline,
        }
    }

    pub fn cancelled() -> Self {
        Self {
            message: "cancelled".to_string(),
            retryable: false,
            kind: LoadErrorKind::Cancelled,
        }
    }

    pub fn hint(&self) -> Option<&'static str> {
        match self.kind {
            LoadErrorKind::Network => Some("check the connection, then press r to retry"),
            LoadErrorKind::Offline => Some("connect to the network or open a cached article"),
            LoadErrorKind::RateLimited => Some("Wikipedia asked us to slow down; retry shortly"),
            LoadErrorKind::NotFound => Some("the page may have been renamed or deleted"),
            LoadErrorKind::Auth => Some("sign-in is not enabled in this release"),
            LoadErrorKind::Parse => Some("try the browser fallback with o"),
            LoadErrorKind::Cancelled => None,
            LoadErrorKind::Protected | LoadErrorKind::Unknown => Some("press r to retry"),
        }
    }
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LoadError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadErrorKind {
    Network,
    NotFound,
    Protected,
    Parse,
    Offline,
    Auth,
    RateLimited,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, Default)]
pub enum Loadable<T> {
    #[default]
    Idle,
    Queued {
        queued_at: Instant,
        priority: Priority,
    },
    Loading {
        stage: LoadStage,
        progress: Option<Progress>,
        started_at: Instant,
        previous: Option<Arc<T>>,
        cancel: CancellationToken,
    },
    Refreshing {
        stage: LoadStage,
        progress: Option<Progress>,
        previous: Arc<T>,
        started_at: Instant,
        cancel: CancellationToken,
    },
    Ready {
        value: Arc<T>,
        updated_at: SystemTime,
        source: Source,
        stale: bool,
    },
    Failed {
        error: LoadError,
        previous: Option<Arc<T>>,
        retryable: bool,
        failed_at: SystemTime,
    },
    Unsupported {
        reason: String,
        fallback: Fallback,
    },
}

impl<T> Loadable<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Loadable::Ready { value, .. } => Some(value.as_ref()),
            Loadable::Loading {
                previous: Some(p), ..
            } => Some(p.as_ref()),
            Loadable::Refreshing { previous, .. } => Some(previous.as_ref()),
            Loadable::Failed {
                previous: Some(p), ..
            } => Some(p.as_ref()),
            _ => None,
        }
    }

    pub fn value_arc(&self) -> Option<Arc<T>> {
        match self {
            Loadable::Ready { value, .. } => Some(Arc::clone(value)),
            Loadable::Loading {
                previous: Some(p), ..
            } => Some(Arc::clone(p)),
            Loadable::Refreshing { previous, .. } => Some(Arc::clone(previous)),
            Loadable::Failed {
                previous: Some(p), ..
            } => Some(Arc::clone(p)),
            _ => None,
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(
            self,
            Loadable::Loading { .. } | Loadable::Refreshing { .. } | Loadable::Queued { .. }
        )
    }

    pub fn is_ready(&self) -> bool {
        matches!(self, Loadable::Ready { .. })
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Loadable::Failed { .. })
    }

    pub fn source(&self) -> Option<Source> {
        match self {
            Loadable::Ready { source, .. } => Some(*source),
            _ => None,
        }
    }

    pub fn error(&self) -> Option<&LoadError> {
        match self {
            Loadable::Failed { error, .. } => Some(error),
            _ => None,
        }
    }

    pub fn stage(&self) -> Option<LoadStage> {
        match self {
            Loadable::Loading { stage, .. } | Loadable::Refreshing { stage, .. } => Some(*stage),
            _ => None,
        }
    }

    pub fn started_at(&self) -> Option<Instant> {
        match self {
            Loadable::Loading { started_at, .. } | Loadable::Refreshing { started_at, .. } => {
                Some(*started_at)
            }
            Loadable::Queued { queued_at, .. } => Some(*queued_at),
            _ => None,
        }
    }

    pub fn begin_refresh(&mut self, previous: Arc<T>) {
        *self = Loadable::Refreshing {
            stage: LoadStage::Connecting,
            progress: None,
            previous,
            started_at: Instant::now(),
            cancel: CancellationToken::new(),
        };
    }

    pub fn begin_loading(&mut self) {
        let previous = self.value_arc();
        *self = Loadable::Loading {
            stage: LoadStage::Connecting,
            progress: None,
            started_at: Instant::now(),
            previous,
            cancel: CancellationToken::new(),
        };
    }

    pub fn ready(value: T, source: Source) -> Self {
        Loadable::Ready {
            value: Arc::new(value),
            updated_at: SystemTime::now(),
            source,
            stale: false,
        }
    }

    pub fn failed(error: LoadError) -> Self {
        let retryable = error.retryable;
        Loadable::Failed {
            error,
            previous: None,
            retryable,
            failed_at: SystemTime::now(),
        }
    }

    pub fn failed_with(error: LoadError, previous: Option<Arc<T>>) -> Self {
        let retryable = error.retryable;
        Loadable::Failed {
            error,
            previous,
            retryable,
            failed_at: SystemTime::now(),
        }
    }

    pub fn cancel(&mut self) {
        match self {
            Loadable::Loading { cancel, .. } => cancel.cancel(),
            Loadable::Refreshing { cancel, .. } => cancel.cancel(),
            _ => {}
        }
    }

    pub fn age(&self) -> Option<std::time::Duration> {
        match self {
            Loadable::Ready { updated_at, .. } => {
                SystemTime::now().duration_since(*updated_at).ok()
            }
            _ => None,
        }
    }
}
