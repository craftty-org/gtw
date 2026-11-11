//! Background-to-UI message and command protocol.
//! Video-control variants are shared with the Media Lab and future M3 UI.
#![allow(dead_code)]
use std::sync::Arc;

use chrono::NaiveDate;
use image::RgbaImage;

use crate::core::{
    Article, Bookmark, Feed, HistoryEntry, LibrarySnapshot, LoadError, MediaKind, ReadingList,
    RequestId, SearchHit, TabId,
};
use crate::media::{AudioData, DecodedMedia};

#[derive(Debug, Clone)]
pub enum Msg {
    Tick,
    Resize(u16, u16),
    Quit,

    FeedLoaded {
        req: RequestId,
        date: NaiveDate,
        result: Result<Feed, LoadError>,
    },
    ArticleLoaded {
        req: RequestId,
        tab: TabId,
        result: Result<Article, LoadError>,
    },
    SearchResults {
        req: RequestId,
        query: String,
        result: Result<Vec<SearchHit>, LoadError>,
    },
    LocalSearchResults {
        query: String,
        result: Result<Vec<SearchHit>, LoadError>,
    },

    BookmarksLoaded(Result<Vec<Bookmark>, LoadError>),
    HistoryLoaded(Result<Vec<HistoryEntry>, LoadError>),
    ListsLoaded(Result<Vec<ReadingList>, LoadError>),
    LibraryLoaded(Result<LibrarySnapshot, LoadError>),
    ListItemsLoaded {
        list_id: i64,
        result: Result<Vec<String>, LoadError>,
    },
    ListPreviewLoaded {
        list_id: i64,
        result: Result<Vec<(String, Option<String>)>, LoadError>,
    },
    ListCreated(Result<ReadingList, LoadError>),
    ListDeleted {
        list_id: i64,
        result: Result<(), LoadError>,
    },
    ArticleAddedToList {
        list_id: i64,
        title: String,
        result: Result<(), LoadError>,
    },
    ArticleRemovedFromList {
        list_id: i64,
        title: String,
        result: Result<(), LoadError>,
    },
    BookmarkStateLoaded {
        title: String,
        bookmarked: bool,
    },
    PeekLoaded {
        req: RequestId,
        title: String,
        summary: String,
        image: Option<String>,
        error: Option<String>,
    },
    BookmarkToggled {
        title: String,
        bookmarked: bool,
        result: Result<(), LoadError>,
    },
    CachePurged(Result<u64, LoadError>),

    MediaLoaded {
        key: String,
        result: Result<DecodedMedia, LoadError>,
    },
    MathRendered {
        key: String,
        result: Result<DecodedMedia, LoadError>,
    },
    AudioLoaded {
        key: String,
        result: Result<AudioData, LoadError>,
    },
    VideoFrame {
        key: String,
        index: u32,
        total: u32,
        frame: Arc<RgbaImage>,
        position: f64,
    },
    VideoLoaded {
        key: String,
        total: u32,
        duration: f64,
    },
    VideoFailed {
        key: String,
        error: LoadError,
    },
    VideoStateChanged {
        key: String,
        playing: bool,
        position: f64,
        volume: f32,
    },

    Toast {
        text: String,
        error: bool,
    },
    OpenUrlResult {
        url: String,
        result: Result<(), LoadError>,
    },
}

#[derive(Debug, Clone)]
pub enum Cmd {
    LoadFeed {
        req: RequestId,
        date: NaiveDate,
        refresh: bool,
    },
    LoadArticle {
        req: RequestId,
        tab: TabId,
        title: String,
        refresh: bool,
    },
    Search {
        req: RequestId,
        query: String,
    },
    SearchLocal {
        query: String,
    },
    LoadBookmarks,
    LoadHistory,
    LoadLists,
    LoadLibrary,
    LoadListItems {
        list_id: i64,
    },
    LoadListPreview {
        list_id: i64,
    },
    CreateList {
        name: String,
    },
    DeleteList {
        list_id: i64,
    },
    AddToList {
        list_id: i64,
        title: String,
    },
    RemoveFromList {
        list_id: i64,
        title: String,
    },
    ToggleBookmark {
        title: String,
    },
    LoadBookmarkState {
        title: String,
    },
    LoadPeek {
        req: RequestId,
        title: String,
    },
    LoadMedia {
        key: String,
        url: String,
        kind: MediaKind,
    },
    RenderMath {
        key: String,
        tex: String,
        light: bool,
    },
    LoadAudio {
        key: String,
        url: String,
        title: String,
    },
    LoadVideo {
        key: String,
        url: String,
        title: String,
    },
    VideoControl(VideoControl),
    PurgeCache,
    OpenUrl {
        url: String,
    },
}

#[derive(Debug, Clone)]
pub enum VideoControl {
    PlayPause,
    Seek(f64),
    Stop,
    Volume(f32),
}
