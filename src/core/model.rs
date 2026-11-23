//! Core domain model shared by the API, cache, layout and UI layers.
//! A few convenience constructors are retained for the v0.2 editing work.
use std::fmt;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::core::Source;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LinkKind {
    Article,
    External,
    Media,
    Citation,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LinkTarget {
    pub kind: LinkKind,
    pub target: String,
    pub label: String,
}



#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InlineKind {
    Normal,
    Bold,
    Italic,
    BoldItalic,
    Code,
    Link(LinkTarget),
    Superscript,
    Subscript,
    Math(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inline {
    pub text: String,
    pub kind: InlineKind,
}

impl Inline {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: InlineKind::Normal,
        }
    }

    pub fn styled(text: impl Into<String>, kind: InlineKind) -> Self {
        Self {
            text: text.into(),
            kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub caption: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Block {
    Heading {
        level: u8,
        id: Option<String>,
        inlines: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    List {
        ordered: bool,
        items: Vec<Vec<Inline>>,
    },
    Quote(Vec<Block>),
    Code {
        language: Option<String>,
        text: String,
    },
    Table(Table),
    Infobox {
        title: Option<String>,
        rows: Vec<(String, String)>,
    },
    Gallery {
        images: Vec<MediaSpec>,
        caption: Option<String>,
    },
    Image(MediaSpec),
    Math {
        tex: String,
        display: bool,
    },
    Audio(MediaSpec),
    Video(MediaSpec),
    Rule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MediaKind {
    Image,
    Gif,
    Svg,
    Audio,
    Video,
    Document,
    Math,
}

impl MediaKind {
    pub fn from_mime(mime: &str, url: &str) -> Self {
        let mime = mime.to_ascii_lowercase();
        let lower = url.to_ascii_lowercase();
        if mime.contains("svg") || lower.ends_with(".svg") {
            Self::Svg
        } else if mime.contains("gif") || lower.ends_with(".gif") {
            Self::Gif
        } else if mime.starts_with("audio/")
            || [".ogg", ".oga", ".opus", ".mp3", ".flac", ".wav", ".m4a"]
                .iter()
                .any(|ext| lower.ends_with(ext))
        {
            Self::Audio
        } else if mime.starts_with("video/")
            || [".webm", ".ogv", ".mp4", ".m4v"]
                .iter()
                .any(|ext| lower.ends_with(ext))
        {
            Self::Video
        } else if mime == "application/pdf" || lower.ends_with(".pdf") {
            Self::Document
        } else {
            Self::Image
        }
    }

    pub fn is_visual(self) -> bool {
        matches!(self, Self::Image | Self::Gif | Self::Svg)
    }
}

impl fmt::Display for MediaKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Image => "image",
            Self::Gif => "animation",
            Self::Svg => "vector",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Document => "document",
            Self::Math => "math",
        };
        f.write_str(label)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaSpec {
    pub url: String,
    pub kind: MediaKind,
    pub title: String,
    pub description: String,
    pub mime: Option<String>,
    pub poster: Option<String>,
    pub duration: Option<f64>,
}

impl MediaSpec {
    pub fn new(url: impl Into<String>, kind: MediaKind) -> Self {
        Self {
            url: url.into(),
            kind,
            title: String::new(),
            description: String::new(),
            mime: None,
            poster: None,
            duration: None,
        }
    }

    pub fn key(&self) -> String {
        format!("{}:{}", self.kind, self.url)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Article {
    pub title: String,
    pub display_title: String,
    pub pageid: Option<u64>,
    pub revision: Option<u64>,
    pub language: String,
    pub html: String,
    pub summary: String,
    pub blocks: Vec<Block>,
    pub plain_text: String,
    pub media: Vec<MediaSpec>,
    pub fetched_at: i64,
    pub source: Source,
}

impl Article {
    pub fn math(&self) -> impl Iterator<Item = &str> {
        self.blocks.iter().filter_map(|b| match b {
            Block::Math { tex, .. } if !tex.trim().is_empty() => Some(tex.as_str()),
            _ => None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    pub title: String,
    pub snippet: String,
    pub pageid: Option<u64>,
    pub local: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResults {
    pub query: String,
    pub hits: Vec<SearchHit>,
    pub local_hits: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FeedKind {
    FeaturedArticle,
    PictureOfTheDay,
    DidYouKnow,
    OnThisDay,
    InTheNews,
    MostRead,
}

impl FeedKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::FeaturedArticle => "📰 Featured article",
            Self::PictureOfTheDay => "🖼️ Picture of the day",
            Self::DidYouKnow => "💡 Did you know",
            Self::OnThisDay => "📅 On this day",
            Self::InTheNews => "🗞️ In the news",
            Self::MostRead => "📈 Most read",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedItem {
    pub title: String,
    pub subtitle: String,
    pub extract: String,
    pub thumbnail: Option<String>,
    pub url: Option<String>,
    pub meta: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedSection {
    pub kind: FeedKind,
    pub items: Vec<FeedItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feed {
    pub date: NaiveDate,
    pub sections: Vec<FeedSection>,
    pub fetched_at: i64,
    #[serde(default = "default_source")]
    pub source: Source,
}

fn default_source() -> Source {
    Source::Network
}

impl Feed {
    pub fn item_count(&self) -> usize {
        self.sections.iter().map(|s| s.items.len()).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub title: String,
    pub added_at: i64,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub title: String,
    pub visited_at: i64,
    pub visits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadingList {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
    pub item_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedArticle {
    pub title: String,
    pub summary: String,
    pub plain_text: String,
    pub size_bytes: u64,
    pub fetched_at: i64,
    pub last_opened_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedDate {
    pub date: NaiveDate,
    pub sections: usize,
    pub items: usize,
    pub fetched_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibrarySnapshot {
    pub articles: Vec<CachedArticle>,
    pub feeds: Vec<FeedDate>,
    pub bookmarks: Vec<Bookmark>,
    pub history: Vec<HistoryEntry>,
    pub lists: Vec<ReadingList>,
    pub bytes: u64,
}
