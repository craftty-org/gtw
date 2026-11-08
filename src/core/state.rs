use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use chrono::{Datelike, Local, NaiveDate};
use ratatui::layout::Rect;

use crate::core::{
    Activity, ActivityCenter, ActivityKind, AppTheme, Article, Bookmark, Cmd, Config, Feed,
    HistoryEntry, LibrarySnapshot, LinkKind, LinkTarget, LoadError, LoadStage, Loadable, MediaKind,
    MediaSpec, Msg, Priority, ReadingList, RequestId, SearchHit, SearchResults, Source, SurfaceId,
    TabId, ThemeEntry, VideoControl,
};
use crate::media::{AudioState, DecodedMedia, VideoState};
use crate::render::layout::{ArticleDoc, build_document};

const SPLASH_TICKS: u64 = 18;
const TOAST_TTL: Duration = Duration::from_secs(5);
/// Activities older than this are considered failed (spinners stop).
const ACTIVITY_TIMEOUT: Duration = Duration::from_secs(30);
/// Loadable surfaces stay spinning until this many seconds have elapsed.
const LOADING_TIMEOUT_SECS: u64 = 45;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    Splash,
    Home,
    Article,
    Bookmarks,
    History,
    Lists,
    ListDetail,
    MediaLab,
}

impl Screen {
    pub fn title(self) -> &'static str {
        match self {
            Self::Splash => "gtw",
            Self::Home => "Home",
            Self::Article => "Article",
            Self::Bookmarks => "Saved",
            Self::History => "History",
            Self::Lists => "Reading lists",
            Self::ListDetail => "Reading list",
            Self::MediaLab => "Media lab",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tab {
    pub id: TabId,
    pub title: String,
    pub article: Loadable<Article>,
    pub request: RequestId,
    pub scroll: u16,
    pub doc: Option<ArticleDoc>,
    pub doc_width: u16,
    pub doc_media_enabled: bool,
    pub doc_max_media_height: u16,
    pub bookmarked: bool,
    pub summary: String,
    pub media: Vec<MediaSpec>,
}

impl Tab {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            id: TabId::new(),
            title: title.into(),
            article: Loadable::Idle,
            request: RequestId::new(),
            scroll: 0,
            doc: None,
            doc_width: 0,
            doc_media_enabled: true,
            doc_max_media_height: 18,
            bookmarked: false,
            summary: String::new(),
            media: Vec::new(),
        }
    }

    pub fn source_label(&self) -> &'static str {
        self.article
            .source()
            .map(|source| match source {
                Source::Network => "online",
                Source::Cache => "cached",
                Source::Prefetch => "prefetched",
                Source::Local => "local",
            })
            .unwrap_or("…")
    }
}

#[derive(Debug, Clone)]
pub struct LineEditor {
    pub chars: Vec<char>,
    pub cursor: usize,
}

impl LineEditor {
    pub fn new() -> Self {
        Self {
            chars: Vec::new(),
            cursor: 0,
        }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn clear(&mut self) {
        self.chars.clear();
        self.cursor = 0;
    }

    pub fn insert(&mut self, c: char) {
        self.chars.insert(self.cursor.min(self.chars.len()), c);
        self.cursor = (self.cursor + 1).min(self.chars.len());
    }

    pub fn paste(&mut self, text: &str) {
        for c in text.chars().filter(|c| !c.is_control()) {
            self.insert(c);
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.chars.len());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.chars.len();
    }

    pub fn delete_word(&mut self) {
        while self.cursor > 0 && self.chars[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
        while self.cursor > 0 && !self.chars[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
    }

    pub fn display_with_cursor(&self) -> String {
        let mut s: String = self.chars[..self.cursor].iter().collect();
        s.push('▏');
        s.extend(&self.chars[self.cursor..]);
        s
    }
}

impl Default for LineEditor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub error: bool,
    pub created: Instant,
}

impl Toast {
    pub fn expired(&self) -> bool {
        self.created.elapsed() > TOAST_TTL
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerMode {
    Search,
    Saved,
    Lists,
    AddToList,
    Theme,
    Navigation,
    ArticleElements,
    Tabs,
    Command,
    History,
}

impl PickerMode {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Search => "Search Wikipedia",
            Self::Saved => "Saved & offline",
            Self::Lists => "Reading lists",
            Self::AddToList => "Add to reading list",
            Self::Theme => "Themes",
            Self::Navigation => "Jump to",
            Self::ArticleElements => "Article outline",
            Self::Tabs => "Open tabs",
            Self::Command => "Command palette",
            Self::History => "History",
        }
    }
}

/// Filter buckets for the in-article element picker. `[` and `]` cycle them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementFilter {
    RelatedLinks,
    Headings,
    Images,
    Code,
    Audio,
}

impl ElementFilter {
    pub const ALL: [ElementFilter; 5] = [
        ElementFilter::RelatedLinks,
        ElementFilter::Headings,
        ElementFilter::Images,
        ElementFilter::Code,
        ElementFilter::Audio,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::RelatedLinks => "related links",
            Self::Headings => "headings",
            Self::Images => "images",
            Self::Code => "code",
            Self::Audio => "audio",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::RelatedLinks => "↗",
            Self::Headings => "¶",
            Self::Images => "▧",
            Self::Code => "⌘",
            Self::Audio => "♪",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let index = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// Category buckets for the merged Saved/Offline picker (`Alt+O`), cycled with
/// `[` and `]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedFilter {
    All,
    Bookmarks,
    Offline,
}

impl SavedFilter {
    pub const ALL: [SavedFilter; 3] = [
        SavedFilter::All,
        SavedFilter::Bookmarks,
        SavedFilter::Offline,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Bookmarks => "bookmarks",
            Self::Offline => "offline",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::All => "•",
            Self::Bookmarks => "★",
            Self::Offline => "⬇",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let index = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone)]
pub enum PickerAction {
    OpenArticle(String),
    OpenList(i64),
    OpenFeed(NaiveDate),
    CreateList,
    AddToList(i64),
    SetTheme(usize),
    GoTo(Screen),
    /// Scroll the active article to a document line.
    ScrollToLine(u16),
    /// Open a link (article, external, or media) from the outline.
    OpenLink(LinkTarget),
    /// Switch to an open tab.
    SwitchTab(TabId),
    /// Run a named command from the command palette.
    RunCommand(CommandKind),
}

/// Everything runnable from the command palette (`;` / `:`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Search,
    Saved,
    Home,
    History,
    Lists,
    MediaLab,
    Themes,
    Tabs,
    Calendar,
    ToggleBookmark,
    AddToList,
    Refresh,
    OpenBrowser,
    NextTheme,
    PrevTheme,
    PurgeCache,
    Help,
    Activity,
    Toc,
    Outline,
    Quit,
}

impl CommandKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Search => "Search Wikipedia",
            Self::Saved => "Saved & offline",
            Self::Home => "Go to Home",
            Self::History => "History",
            Self::Lists => "Reading lists",
            Self::MediaLab => "Go to Media lab",
            Self::Themes => "Switch theme",
            Self::Tabs => "List open tabs",
            Self::Calendar => "Jump to date",
            Self::ToggleBookmark => "Toggle bookmark",
            Self::AddToList => "Add to reading list",
            Self::Refresh => "Refresh",
            Self::OpenBrowser => "Open in browser",
            Self::NextTheme => "Next theme",
            Self::PrevTheme => "Previous theme",
            Self::PurgeCache => "Purge expired cache",
            Self::Help => "Show help",
            Self::Activity => "Show activity",
            Self::Toc => "Table of contents",
            Self::Outline => "Article outline",
            Self::Quit => "Quit gtw",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Search | Self::Saved | Self::History | Self::Lists => "picker",
            Self::Home | Self::MediaLab => "navigate",
            Self::Themes
            | Self::Tabs
            | Self::Calendar
            | Self::Outline
            | Self::Toc
            | Self::Activity
            | Self::Help => "overlay",
            Self::ToggleBookmark | Self::AddToList | Self::Refresh | Self::OpenBrowser => "article",
            Self::NextTheme | Self::PrevTheme | Self::PurgeCache => "action",
            Self::Quit => "quit",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PickerEntry {
    pub label: String,
    pub hint: String,
    pub action: PickerAction,
    pub local: bool,
}

impl PickerEntry {
    pub fn new(label: impl Into<String>, hint: impl Into<String>, action: PickerAction) -> Self {
        Self {
            label: label.into(),
            hint: hint.into(),
            action,
            local: false,
        }
    }

    pub fn local(mut self, local: bool) -> Self {
        self.local = local;
        self
    }
}

#[derive(Debug, Clone)]
pub struct Picker {
    pub mode: PickerMode,
    pub input: LineEditor,
    pub base: Vec<PickerEntry>,
    pub entries: Vec<PickerEntry>,
    pub selected: usize,
    pub loading: bool,
    pub online_req: Option<RequestId>,
    pub error: Option<String>,
    /// Active filter bucket for `PickerMode::ArticleElements`.
    pub element_filter: ElementFilter,
    /// Entries for each element bucket, kept separate from the text filter.
    pub element_base: Vec<(ElementFilter, PickerEntry)>,
    /// Active category for `PickerMode::Saved` (`Alt+O`).
    pub saved_filter: SavedFilter,
    pub saved_base: Vec<(SavedFilter, PickerEntry)>,
}

impl Picker {
    pub fn new(mode: PickerMode) -> Self {
        Self {
            mode,
            input: LineEditor::new(),
            base: Vec::new(),
            entries: Vec::new(),
            selected: 0,
            loading: false,
            online_req: None,
            error: None,
            element_filter: ElementFilter::RelatedLinks,
            element_base: Vec::new(),
            saved_filter: SavedFilter::All,
            saved_base: Vec::new(),
        }
    }

    /// Rebuild visible entries for the current saved category, then text-filter.
    pub fn apply_saved_filter(&mut self) {
        self.base = self
            .saved_base
            .iter()
            .filter(|(filter, _)| {
                self.saved_filter == SavedFilter::All || *filter == self.saved_filter
            })
            .map(|(_, entry)| entry.clone())
            .collect();
        self.filter();
        self.selected = 0;
    }

    pub fn cycle_saved_filter(&mut self, forward: bool) {
        self.saved_filter = if forward {
            self.saved_filter.next()
        } else {
            self.saved_filter.prev()
        };
        self.apply_saved_filter();
    }

    pub fn set_base(&mut self, entries: Vec<PickerEntry>) {
        self.base = entries;
        self.filter();
    }

    /// Rebuild the visible entries for the current element bucket, then apply
    /// the text filter within it.
    pub fn apply_element_filter(&mut self) {
        self.base = self
            .element_base
            .iter()
            .filter(|(filter, _)| *filter == self.element_filter)
            .map(|(_, entry)| entry.clone())
            .collect();
        self.filter();
        self.selected = 0;
    }

    pub fn cycle_element_filter(&mut self, forward: bool) {
        self.element_filter = if forward {
            self.element_filter.next()
        } else {
            self.element_filter.prev()
        };
        self.apply_element_filter();
    }

    pub fn filter(&mut self) {
        let query = self.input.text().to_lowercase();
        if query.trim().is_empty() {
            self.entries = self.base.clone();
        } else {
            let words: Vec<&str> = query.split_whitespace().collect();
            self.entries = self
                .base
                .iter()
                .filter(|e| {
                    let hay = format!("{} {}", e.label, e.hint).to_lowercase();
                    words.iter().all(|w| hay.contains(w))
                })
                .cloned()
                .collect();
        }
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }

    pub fn select_next(&mut self) {
        if !self.entries.is_empty() {
            self.selected = (self.selected + 1) % self.entries.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.entries.is_empty() {
            self.selected = self
                .selected
                .checked_sub(1)
                .unwrap_or(self.entries.len() - 1);
        }
    }

    pub fn selected_entry(&self) -> Option<&PickerEntry> {
        self.entries.get(self.selected)
    }
}

#[derive(Debug, Clone)]
pub struct NewListState {
    pub input: LineEditor,
    pub add_title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    Help,
    Activity,
    Toc,
    MediaViewer,
    NewList,
    ConfirmDeleteList,
    Peek,
    Calendar,
}

/// Cursor + reference "today" for the jump-to-date calendar overlay.
#[derive(Debug, Clone, Copy)]
pub struct CalendarState {
    pub cursor: NaiveDate,
    pub today: NaiveDate,
}

impl CalendarState {
    pub fn new(today: NaiveDate) -> Self {
        Self {
            cursor: today,
            today,
        }
    }
}

/// A Ctrl+K preview of the link under the cursor: the target article's summary
/// plus an optional preview image, loaded from the network or cache.
#[derive(Debug, Clone, Default)]
pub struct PeekState {
    pub title: String,
    pub target: Option<LinkTarget>,
    pub summary: Option<String>,
    pub image: Option<String>,
    pub req: Option<RequestId>,
    pub error: Option<String>,
    pub loading: bool,
}

/// Preview pane used by the Alt+e outline picker. Article-link previews fetch
/// a summary in the background; images reuse the queue/media store; code and
/// audio render locally (no network).
#[derive(Debug, Clone, Default)]
pub struct PickerPreview {
    pub req: Option<RequestId>,
    pub title: String,
    pub summary: Option<String>,
    pub image: Option<String>,
    pub error: Option<String>,
    pub loading: bool,
}

/// Floating in-page search state.
#[derive(Debug, Clone)]
pub struct FinderState {
    pub input: LineEditor,
    /// Document line indices that contain the query.
    pub matches: Vec<usize>,
    pub current: usize,
}

impl FinderState {
    fn new() -> Self {
        Self {
            input: LineEditor::new(),
            matches: Vec::new(),
            current: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Hover {
    pub target: LinkTarget,
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone)]
pub struct DemoItem {
    pub label: &'static str,
    pub kind: DemoKind,
    pub url: Option<&'static str>,
    pub text: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemoKind {
    Image,
    Gif,
    Svg,
    Code,
    Math,
    Audio,
    Video,
}

#[derive(Debug, Clone)]
pub struct MediaLabState {
    pub items: Vec<DemoItem>,
    pub selected: usize,
}

impl Default for MediaLabState {
    fn default() -> Self {
        Self {
            items: vec![
                DemoItem {
                    label: "Image · PNG / JPEG",
                    kind: DemoKind::Image,
                    url: Some(
                        "https://upload.wikimedia.org/wikipedia/commons/thumb/8/8a/Banana-Single.jpg/960px-Banana-Single.jpg",
                    ),
                    text: None,
                },
                DemoItem {
                    label: "Animation · GIF",
                    kind: DemoKind::Gif,
                    url: Some(
                        "https://upload.wikimedia.org/wikipedia/commons/2/2c/Rotating_earth_%28large%29.gif",
                    ),
                    text: None,
                },
                DemoItem {
                    label: "Vector · SVG",
                    kind: DemoKind::Svg,
                    url: Some(
                        "https://upload.wikimedia.org/wikipedia/commons/6/6b/Bitmap_VS_SVG.svg",
                    ),
                    text: None,
                },
                DemoItem {
                    label: "Code · syntax highlighting",
                    kind: DemoKind::Code,
                    url: None,
                    text: Some(
                        "fn main() {\n    // gtw renders fenced code with syntect\n    let greeting = \"hello, terminal\";\n    for ch in greeting.chars().rev() {\n        print!(\"{ch}\");\n    }\n    println!();\n}\n",
                    ),
                },
                DemoItem {
                    label: "Math · KaTeX via RaTeX",
                    kind: DemoKind::Math,
                    url: None,
                    text: Some("\\int_{-\\infty}^{\\infty} e^{-x^2}\\,dx = \\sqrt{\\pi}"),
                },
                DemoItem {
                    label: "Audio · Symphonia + rodio",
                    kind: DemoKind::Audio,
                    url: Some("https://upload.wikimedia.org/wikipedia/commons/c/c8/Example.ogg"),
                    text: None,
                },
                DemoItem {
                    label: "Video · VP9 / Opus WebM",
                    kind: DemoKind::Video,
                    url: Some(
                        "https://upload.wikimedia.org/wikipedia/commons/transcoded/7/79/Big_Buck_Bunny_small.ogv/Big_Buck_Bunny_small.ogv.240p.vp9.webm",
                    ),
                    text: None,
                },
            ],
            selected: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum HitAction {
    Sidebar(Screen),
    Tab(TabId),
    CloseTab(TabId),
    FeedSection(usize),
    FeedItem {
        section: usize,
        item: usize,
    },
    FeedPrev,
    FeedNext,
    OpenCalendar,
    CalendarDay(NaiveDate),
    CalendarMonth(i32),
    ArticleLink(LinkTarget),
    ArticleMedia(String),
    Toc(usize),
    PickerEntry(usize),
    #[allow(dead_code)]
    PickerNewList,
    PickerClose,
    PickerElementFilter(ElementFilter),
    BookmarkRow(usize),
    HistoryRow(usize),
    ListRow(usize),
    ListOpen(i64),
    ListDelete(i64),
    ListItemRow(usize),
    MediaLabItem(usize),
    CloseOverlay,
    HelpClose,
    ActivityClose,
    PeekClose,
    MediaPlayPause,
    MediaSeek(f64),
    NewListSubmit,
    ConfirmDelete(bool),
}

#[derive(Debug, Clone)]
pub struct HitRegion {
    pub area: Rect,
    pub action: HitAction,
    pub z: u8,
}

impl HitRegion {
    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.area.x
            && x < self.area.x.saturating_add(self.area.width)
            && y >= self.area.y
            && y < self.area.y.saturating_add(self.area.height)
    }
}

pub struct AppState {
    pub screen: Screen,
    pub tabs: Vec<Tab>,
    pub active_tab: Option<TabId>,
    pub home: Loadable<Feed>,
    pub feed_date: NaiveDate,
    pub feed_req: RequestId,
    pub feed_section: usize,
    pub feed_item: usize,
    pub feed_scroll: u16,
    pub article_view_height: u16,
    pub list_view_height: u16,

    pub search_req: RequestId,
    pub search_results: Loadable<SearchResults>,
    pub search_due: Option<Instant>,

    pub bookmarks: Loadable<Vec<Bookmark>>,
    pub history: Loadable<Vec<HistoryEntry>>,
    pub lists: Loadable<Vec<ReadingList>>,
    pub list_items: Loadable<Vec<String>>,
    pub library: Loadable<LibrarySnapshot>,
    pub active_list: Option<i64>,
    pub pending_list_add: Option<String>,

    pub bookmark_index: usize,
    pub history_index: usize,
    pub list_index: usize,
    pub list_item_index: usize,

    pub picker: Option<Picker>,
    pub overlay: Overlay,
    pub peek: PeekState,
    pub picker_preview: PickerPreview,
    pub finder: Option<FinderState>,
    pub calendar: CalendarState,
    /// Saved theme index captured when the theme picker opens, so `esc` can
    /// restore it after a live, unsaved preview.
    pub theme_revert: Option<usize>,
    pub new_list: Option<NewListState>,
    pub confirm_list: Option<i64>,
    pub media_viewer: Option<MediaSpec>,
    pub help_scroll: u16,
    pub toc_index: usize,
    /// Index into the active document's `links` for vim-style link jumping.
    pub link_cursor: Option<usize>,

    pub theme_idx: usize,
    pub themes: Vec<ThemeEntry>,
    pub images: HashMap<String, Loadable<DecodedMedia>>,
    pub math_images: HashMap<String, Loadable<DecodedMedia>>,
    pub audio: AudioState,
    pub video: VideoState,
    pub media_lab: MediaLabState,
    pub media_lab_loaded: bool,

    pub activity: ActivityCenter,
    pub toast: Option<Toast>,
    pub hover: Option<Hover>,
    pub last_click: Option<(Instant, u16, u16)>,
    pub hitmap: Vec<HitRegion>,
    pub pending_cmds: Vec<Cmd>,

    pub should_quit: bool,
    pub offline: bool,
    pub config: Config,
    pub width: u16,
    pub height: u16,
    pub tick: u64,
    pub last_saved_theme: Option<String>,
    pub debug_status: String,
}

impl AppState {
    pub fn new(config: Config, offline: bool, demo_media: bool) -> Self {
        let themes = crate::render::theme::merged_themes();
        let configured_theme = config.theme.name.to_lowercase();
        let theme_idx = themes
            .iter()
            .position(|t| t.name.to_lowercase() == configured_theme)
            .unwrap_or(0);
        let screen = if demo_media {
            Screen::MediaLab
        } else {
            Screen::Splash
        };
        let today = Local::now().date_naive();
        let mut state = Self {
            screen,
            tabs: Vec::new(),
            active_tab: None,
            home: Loadable::Idle,
            feed_date: today,
            feed_req: RequestId::new(),
            feed_section: 0,
            feed_item: 0,
            feed_scroll: 0,
            article_view_height: 20,
            list_view_height: 20,
            search_req: RequestId::new(),
            search_results: Loadable::Idle,
            search_due: None,
            bookmarks: Loadable::Idle,
            history: Loadable::Idle,
            lists: Loadable::Idle,
            list_items: Loadable::Idle,
            library: Loadable::Idle,
            active_list: None,
            pending_list_add: None,
            bookmark_index: 0,
            history_index: 0,
            list_index: 0,
            list_item_index: 0,
            picker: None,
            overlay: Overlay::None,
            peek: PeekState::default(),
            picker_preview: PickerPreview::default(),
            finder: None,
            calendar: CalendarState::new(today),
            theme_revert: None,
            new_list: None,
            confirm_list: None,
            media_viewer: None,
            help_scroll: 0,
            toc_index: 0,
            link_cursor: None,
            theme_idx,
            themes,
            images: HashMap::new(),
            math_images: HashMap::new(),
            audio: AudioState::default(),
            video: VideoState::default(),
            media_lab: MediaLabState::default(),
            media_lab_loaded: false,
            activity: ActivityCenter::new(),
            toast: None,
            hover: None,
            last_click: None,
            hitmap: Vec::new(),
            pending_cmds: Vec::new(),
            should_quit: false,
            offline: offline || config.general.offline,
            config,
            width: 100,
            height: 30,
            tick: 0,
            last_saved_theme: None,
            debug_status: String::new(),
        };
        if state.offline {
            state.debug_status = "offline mode".to_string();
        }
        state
    }

    pub fn current_theme(&self) -> &ThemeEntry {
        self.themes
            .get(self.theme_idx)
            .or_else(|| self.themes.first())
            .expect("bundled themes")
    }

    pub fn theme(&self) -> AppTheme {
        self.current_theme().theme
    }

    pub fn is_light(&self) -> bool {
        self.current_theme().light
    }

    /// Images/math thumbnails enabled (and not in `no_image` mode).
    pub fn images_enabled(&self) -> bool {
        self.config.media.enabled && !self.config.media.no_image
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.active_tab
            .and_then(|id| self.tabs.iter().find(|t| t.id == id))
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        let id = self.active_tab?;
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn set_screen(&mut self, screen: Screen) -> Vec<Cmd> {
        self.screen = screen;
        self.overlay = Overlay::None;
        self.picker = None;
        let mut cmds = Vec::new();
        match screen {
            Screen::Home => {
                if self.home.value().is_none() {
                    cmds.extend(self.load_feed(self.feed_date, false));
                }
            }
            Screen::Bookmarks => cmds.push(Cmd::LoadBookmarks),
            Screen::History => cmds.push(Cmd::LoadHistory),
            Screen::Lists | Screen::ListDetail => {
                cmds.push(Cmd::LoadLists);
                cmds.push(Cmd::LoadLibrary);
            }
            Screen::MediaLab if !self.media_lab_loaded => {
                self.media_lab_loaded = true;
                cmds.extend(self.load_media_lab());
            }
            _ => {}
        }
        cmds
    }

    pub fn bootstrap(&mut self) -> Vec<Cmd> {
        let mut cmds = vec![
            Cmd::LoadBookmarks,
            Cmd::LoadHistory,
            Cmd::LoadLists,
            Cmd::LoadLibrary,
            Cmd::PurgeCache,
        ];
        cmds.extend(self.load_feed(self.feed_date, false));
        if matches!(self.screen, Screen::MediaLab) && !self.media_lab_loaded {
            self.media_lab_loaded = true;
            cmds.extend(self.load_media_lab());
        }
        self.start_activity(
            RequestId(0),
            ActivityKind::Cache,
            "library",
            SurfaceId::Other,
            false,
        );
        cmds
    }

    fn start_activity(
        &mut self,
        req: RequestId,
        kind: ActivityKind,
        label: impl Into<String>,
        surface: SurfaceId,
        cancellable: bool,
    ) {
        let label = label.into();
        self.activity.push(Activity {
            id: req,
            kind,
            label,
            surface,
            priority: Priority::Interactive,
            progress: None,
            cancellable,
            started_at: Instant::now(),
            cancel: tokio_util::sync::CancellationToken::new(),
        });
    }

    fn finish_activity(&mut self, req: RequestId) {
        self.activity.remove(req);
    }

    /// Stop spinners whose request never produced a message. Anything older
    /// than `ACTIVITY_TIMEOUT` is dropped and the matching loading surface is
    /// flipped to a failed/timeout state so the user can retry.
    fn prune_stale_activity(&mut self) {
        let cutoff = ACTIVITY_TIMEOUT;
        let stale: Vec<RequestId> = self
            .activity
            .activities
            .iter()
            .filter(|activity| activity.started_at.elapsed() > cutoff)
            .map(|activity| activity.id)
            .collect();
        if stale.is_empty() {
            return;
        }
        for req in stale {
            self.activity.remove(req);
        }
        if let Loadable::Loading { started_at, .. } | Loadable::Refreshing { started_at, .. } =
            &self.home
            && started_at.elapsed() > Duration::from_secs(LOADING_TIMEOUT_SECS)
        {
            let err = LoadError::new(
                crate::core::LoadErrorKind::Network,
                "feed request timed out — press r to retry",
            );
            self.home = match self.home.value_arc() {
                Some(prev) => Loadable::failed_with(err, Some(prev)),
                None => Loadable::failed(err),
            };
        }
    }

    pub fn load_feed(&mut self, date: NaiveDate, refresh: bool) -> Vec<Cmd> {
        let req = RequestId::new();
        self.feed_req = req;
        self.feed_date = date;
        if let Some(prev) = self.home.value_arc() {
            self.home.begin_refresh(prev);
        } else {
            self.home = Loadable::Loading {
                stage: LoadStage::Connecting,
                progress: None,
                started_at: Instant::now(),
                previous: None,
                cancel: tokio_util::sync::CancellationToken::new(),
            };
        }
        self.start_activity(
            req,
            ActivityKind::FetchArticle,
            format!("feed {date}"),
            SurfaceId::Home,
            true,
        );
        vec![Cmd::LoadFeed { req, date, refresh }]
    }

    pub fn feed_next(&mut self) -> Vec<Cmd> {
        let date = self.feed_date.succ_opt().unwrap_or(self.feed_date);
        self.load_feed(date, false)
    }

    pub fn feed_prev(&mut self) -> Vec<Cmd> {
        let date = self.feed_date.pred_opt().unwrap_or(self.feed_date);
        self.load_feed(date, false)
    }

    /// Open the jump-to-date calendar, seeded at the current feed date.
    pub fn open_calendar(&mut self) {
        let today = Local::now().date_naive();
        self.calendar = CalendarState {
            cursor: self.feed_date,
            today,
        };
        self.overlay = Overlay::Calendar;
        self.picker = None;
    }

    /// Open the feed for a specific date chosen from the calendar.
    pub fn jump_to_date(&mut self, date: NaiveDate) -> Vec<Cmd> {
        self.overlay = Overlay::None;
        let date = if date > Local::now().date_naive() {
            Local::now().date_naive()
        } else {
            date
        };
        self.screen = Screen::Home;
        self.load_feed(date, false)
    }

    pub fn calendar_move(&mut self, days: i64) {
        if let Some(next) = self
            .calendar
            .cursor
            .checked_add_signed(chrono::Duration::days(days))
        {
            self.calendar.cursor = next;
        }
    }

    pub fn calendar_move_months(&mut self, months: i32) {
        let mut year = self.calendar.cursor.year();
        let mut month = self.calendar.cursor.month0() as i32 + months;
        while month < 0 {
            month += 12;
            year -= 1;
        }
        while month > 11 {
            month -= 12;
            year += 1;
        }
        let day = self.calendar.cursor.day();
        let last = days_in_month(year, month as u32 + 1);
        let date = NaiveDate::from_ymd_opt(year, month as u32 + 1, day.min(last));
        if let Some(date) = date {
            self.calendar.cursor = date;
        }
    }

    fn create_tab(&mut self, title: impl Into<String>) -> TabId {
        let tab = Tab::new(title);
        let id = tab.id;
        self.tabs.push(tab);
        self.active_tab = Some(id);
        id
    }

    pub fn open_article(&mut self, title: impl Into<String>, new_tab: bool) -> Vec<Cmd> {
        let title = title.into();
        if title.trim().is_empty() {
            return vec![];
        }
        let active_exists = self.active_tab.is_some();
        let tab_id =
            if !new_tab && !matches!(self.screen, Screen::Home | Screen::MediaLab) && active_exists
            {
                self.active_tab.unwrap()
            } else {
                self.create_tab(title.clone())
            };
        let req = RequestId::new();
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.title = title.clone();
            tab.request = req;
            tab.scroll = 0;
            tab.doc = None;
            tab.doc_width = 0;
            tab.summary.clear();
            tab.media.clear();
            tab.article = Loadable::Loading {
                stage: LoadStage::Connecting,
                progress: None,
                started_at: Instant::now(),
                previous: None,
                cancel: tokio_util::sync::CancellationToken::new(),
            };
        }
        self.active_tab = Some(tab_id);
        self.screen = Screen::Article;
        self.overlay = Overlay::None;
        self.picker = None;
        self.hover = None;
        self.start_activity(
            req,
            ActivityKind::FetchArticle,
            title.clone(),
            SurfaceId::Article,
            true,
        );
        vec![Cmd::LoadArticle {
            req,
            tab: tab_id,
            title,
            refresh: false,
        }]
    }

    pub fn refresh_article(&mut self) -> Vec<Cmd> {
        let Some(tab) = self.active_tab() else {
            return vec![];
        };
        let title = tab.title.clone();
        let req = RequestId::new();
        let tab_id = tab.id;
        let previous = tab.article.value_arc();
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.request = req;
            tab.article = match previous {
                Some(prev) => Loadable::Refreshing {
                    stage: LoadStage::Connecting,
                    progress: None,
                    previous: prev,
                    started_at: Instant::now(),
                    cancel: tokio_util::sync::CancellationToken::new(),
                },
                None => Loadable::Loading {
                    stage: LoadStage::Connecting,
                    progress: None,
                    started_at: Instant::now(),
                    previous: None,
                    cancel: tokio_util::sync::CancellationToken::new(),
                },
            };
        }
        self.start_activity(
            req,
            ActivityKind::FetchArticle,
            title.clone(),
            SurfaceId::Article,
            true,
        );
        vec![Cmd::LoadArticle {
            req,
            tab: tab_id,
            title,
            refresh: true,
        }]
    }

    pub fn close_tab(&mut self, id: TabId) {
        let Some(pos) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        self.tabs.remove(pos);
        if self.active_tab == Some(id) {
            let next = self
                .tabs
                .get(pos.saturating_sub(1))
                .or_else(|| self.tabs.first())
                .map(|t| t.id);
            self.active_tab = next;
            if self.active_tab.is_none() {
                self.screen = Screen::Home;
            }
        }
    }

    pub fn switch_tab(&mut self, id: TabId) {
        if self.tabs.iter().any(|t| t.id == id) {
            self.active_tab = Some(id);
            self.screen = Screen::Article;
            self.overlay = Overlay::None;
        }
    }

    pub fn cycle_tab(&mut self, delta: isize) {
        if self.tabs.is_empty() {
            return;
        }
        let current = self
            .active_tab
            .and_then(|id| self.tabs.iter().position(|t| t.id == id))
            .unwrap_or(0) as isize;
        let len = self.tabs.len() as isize;
        let next = (current + delta).rem_euclid(len) as usize;
        self.active_tab = Some(self.tabs[next].id);
        self.screen = Screen::Article;
    }

    pub fn open_search_picker(&mut self) -> Vec<Cmd> {
        let mut picker = Picker::new(PickerMode::Search);
        let entries = self.offline_entries();
        picker.set_base(entries);
        self.picker = Some(picker);
        self.overlay = Overlay::None;
        if self.library.value().is_none() {
            vec![Cmd::LoadLibrary]
        } else {
            vec![]
        }
    }

    /// Merged Saved (bookmarks) + Offline (cached) picker (`Alt+O`).
    /// `[`/`]` toggle categories.
    pub fn open_saved_picker(&mut self) -> Vec<Cmd> {
        let mut picker = Picker::new(PickerMode::Saved);
        picker.saved_base = self.saved_picker_entries();
        picker.apply_saved_filter();
        self.picker = Some(picker);
        self.overlay = Overlay::None;
        vec![Cmd::LoadLibrary, Cmd::LoadBookmarks, Cmd::LoadHistory]
    }

    /// Compatibility alias.
    pub fn open_offline_picker(&mut self) -> Vec<Cmd> {
        self.open_saved_picker()
    }

    pub fn open_history_picker(&mut self) -> Vec<Cmd> {
        let mut picker = Picker::new(PickerMode::History);
        let entries = self
            .history
            .value()
            .map(|h| {
                h.iter()
                    .map(|entry| {
                        PickerEntry::new(
                            entry.title.clone(),
                            relative_time(entry.visited_at),
                            PickerAction::OpenArticle(entry.title.clone()),
                        )
                        .local(true)
                    })
                    .collect()
            })
            .unwrap_or_default();
        picker.set_base(entries);
        self.picker = Some(picker);
        self.overlay = Overlay::None;
        vec![Cmd::LoadHistory]
    }

    pub fn open_lists_picker(&mut self) -> Vec<Cmd> {
        let mut picker = Picker::new(PickerMode::Lists);
        picker.set_base(self.list_entries());
        self.picker = Some(picker);
        self.overlay = Overlay::None;
        vec![Cmd::LoadLists]
    }

    pub fn open_add_to_list(&mut self, title: String) -> Vec<Cmd> {
        self.pending_list_add = Some(title);
        let mut picker = Picker::new(PickerMode::AddToList);
        let mut entries = self.list_entries();
        for entry in &mut entries {
            if let PickerAction::OpenList(id) = entry.action {
                entry.action = PickerAction::AddToList(id);
            }
        }
        picker.set_base(entries);
        self.picker = Some(picker);
        self.overlay = Overlay::None;
        vec![Cmd::LoadLists]
    }

    pub fn open_theme_picker(&mut self) -> Vec<Cmd> {
        // Snapshot the active palette so `esc` can restore it after a live,
        // unsaved preview.
        self.theme_revert = Some(self.theme_idx);
        let mut picker = Picker::new(PickerMode::Theme);
        picker.set_base(self.theme_entries());
        self.picker = Some(picker);
        self.overlay = Overlay::None;
        vec![]
    }

    /// Apply the highlighted palette to the whole UI immediately (preview only;
    /// persisting happens on `enter`).
    pub fn preview_picker_theme(&mut self) {
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        if picker.mode != PickerMode::Theme {
            return;
        }
        let Some(entry) = picker.selected_entry() else {
            return;
        };
        if let PickerAction::SetTheme(idx) = entry.action
            && idx < self.themes.len()
            && idx != self.theme_idx
        {
            self.theme_idx = idx;
            self.invalidate_layout();
        }
    }

    /// Restore the palette captured when the theme picker was opened.
    pub fn revert_picker_theme(&mut self) {
        if let Some(idx) = self.theme_revert.take()
            && idx < self.themes.len()
            && idx != self.theme_idx
        {
            self.theme_idx = idx;
            self.invalidate_layout();
        }
    }

    /// Floating replacement for the old left sidebar: one picker that jumps to
    /// every top-level destination.
    pub fn open_nav_picker(&mut self) -> Vec<Cmd> {
        let entries = vec![
            PickerEntry::new(
                "Home",
                "daily Wikipedia feed",
                PickerAction::GoTo(Screen::Home),
            ),
            PickerEntry::new(
                "Saved",
                "bookmarked articles",
                PickerAction::GoTo(Screen::Bookmarks),
            ),
            PickerEntry::new(
                "History",
                "recently read",
                PickerAction::GoTo(Screen::History),
            ),
            PickerEntry::new(
                "Reading lists",
                "curated collections",
                PickerAction::GoTo(Screen::Lists),
            ),
            PickerEntry::new(
                "Media lab",
                "preview every media path",
                PickerAction::GoTo(Screen::MediaLab),
            ),
            PickerEntry::new(
                "Themes",
                "switch palette",
                PickerAction::RunCommand(CommandKind::Themes),
            ),
        ];
        let mut picker = Picker::new(PickerMode::Navigation);
        picker.set_base(entries);
        self.picker = Some(picker);
        self.overlay = Overlay::None;
        vec![]
    }

    /// Build the in-article element picker from the active document: related
    /// links, headings, images, code and audio, cycled with `[` and `]`.
    pub fn open_element_picker(&mut self) -> Vec<Cmd> {
        if self.active_tab().and_then(|tab| tab.doc.as_ref()).is_none() {
            self.toast("article is still laying out", true);
            return vec![];
        }
        // Build the buckets in a borrowed scope so `self` is free to mutate
        // once the picker is installed.
        let element_base = {
            let doc = self
                .active_tab()
                .and_then(|tab| tab.doc.as_ref())
                .expect("checked above");
            let mut element_base: Vec<(ElementFilter, PickerEntry)> = Vec::new();
            for link in doc.links.iter() {
                let bucket = match link.target.kind {
                    LinkKind::Article | LinkKind::External => ElementFilter::RelatedLinks,
                    LinkKind::Media => ElementFilter::Audio,
                    LinkKind::Citation => continue,
                };
                let detail = match link.target.kind {
                    LinkKind::Article => format!("line {} · article", link.line + 1),
                    LinkKind::External => format!("line {} · web", link.line + 1),
                    LinkKind::Media => format!("line {} · media", link.line + 1),
                    LinkKind::Citation => String::new(),
                };
                element_base.push((
                    bucket,
                    PickerEntry::new(
                        link.target.label.clone(),
                        detail,
                        PickerAction::OpenLink(link.target.clone()),
                    ),
                ));
            }
            for entry in &doc.toc {
                element_base.push((
                    ElementFilter::Headings,
                    PickerEntry::new(
                        entry.title.clone(),
                        format!("line {} · h{}", entry.line + 1, entry.level),
                        PickerAction::ScrollToLine(entry.line),
                    ),
                ));
            }
            for placement in &doc.media {
                let bucket = match placement.kind {
                    MediaKind::Audio | MediaKind::Video => ElementFilter::Audio,
                    _ => ElementFilter::Images,
                };
                element_base.push((
                    bucket,
                    PickerEntry::new(
                        if placement.alt.is_empty() {
                            placement.kind.to_string()
                        } else {
                            placement.alt.clone()
                        },
                        format!("line {} · {}", placement.line + 1, placement.kind),
                        PickerAction::ScrollToLine(placement.line as u16),
                    ),
                ));
            }
            for code in &doc.code {
                element_base.push((
                    ElementFilter::Code,
                    PickerEntry::new(
                        code.label.clone(),
                        format!("line {} · {} lines", code.line + 1, code.lines),
                        PickerAction::ScrollToLine(code.line),
                    ),
                ));
            }
            element_base
        };

        // Collapse repeats: a link/heading/image often appears many times.
        let element_base = dedupe_elements(element_base);

        let mut picker = Picker::new(PickerMode::ArticleElements);
        picker.element_base = element_base;
        picker.apply_element_filter();
        self.picker = Some(picker);
        self.overlay = Overlay::None;
        self.refresh_picker_preview()
    }

    /// Picker over the currently open tabs (`Alt+``).
    pub fn open_tabs_picker(&mut self) -> Vec<Cmd> {
        if self.tabs.is_empty() {
            self.toast("no open tabs", true);
            return vec![];
        }
        let mut picker = Picker::new(PickerMode::Tabs);
        picker.set_base(self.tab_entries());
        self.picker = Some(picker);
        self.overlay = Overlay::None;
        vec![]
    }

    fn tab_entries(&self) -> Vec<PickerEntry> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let active = self.active_tab == Some(tab.id);
                PickerEntry::new(
                    format!("{}{}", if active { "● " } else { "  " }, tab.title),
                    format!("tab {} · {}", index + 1, tab.source_label()),
                    PickerAction::SwitchTab(tab.id),
                )
            })
            .collect()
    }

    /// The `;` / `:` command palette: navigation destinations and runnable
    /// actions in one searchable list.
    pub fn open_command_palette(&mut self) -> Vec<Cmd> {
        let commands = [
            CommandKind::Search,
            CommandKind::Saved,
            CommandKind::Home,
            CommandKind::History,
            CommandKind::Lists,
            CommandKind::MediaLab,
            CommandKind::Themes,
            CommandKind::Tabs,
            CommandKind::Calendar,
            CommandKind::ToggleBookmark,
            CommandKind::AddToList,
            CommandKind::Refresh,
            CommandKind::OpenBrowser,
            CommandKind::NextTheme,
            CommandKind::PrevTheme,
            CommandKind::PurgeCache,
            CommandKind::Help,
            CommandKind::Activity,
            CommandKind::Toc,
            CommandKind::Outline,
            CommandKind::Quit,
        ];
        let entries = commands
            .into_iter()
            .map(|command| {
                PickerEntry::new(
                    command.label(),
                    command.hint(),
                    PickerAction::RunCommand(command),
                )
            })
            .collect();
        let mut picker = Picker::new(PickerMode::Command);
        picker.set_base(entries);
        self.picker = Some(picker);
        self.overlay = Overlay::None;
        vec![]
    }

    /// The element bucket + entry behind the currently selected outline row.
    pub fn selected_element(&self) -> Option<(ElementFilter, PickerEntry)> {
        let picker = self.picker.as_ref()?;
        let selected = picker.selected_entry()?;
        let selected_key = format!("{}|{:?}", selected.label, selected.action);
        picker
            .element_base
            .iter()
            .find(|(_, entry)| format!("{}|{:?}", entry.label, entry.action) == selected_key)
            .cloned()
    }

    /// Kick a summary fetch for the selected outline entry when it is a
    /// related article link. Called after opening the outline and on every
    /// selection change; results land in `picker_preview`.
    pub fn refresh_picker_preview(&mut self) -> Vec<Cmd> {
        if self.picker.as_ref().map(|p| &p.mode) != Some(&PickerMode::ArticleElements) {
            return vec![];
        }
        let Some((filter, entry)) = self.selected_element() else {
            self.picker_preview = PickerPreview::default();
            return vec![];
        };
        match filter {
            ElementFilter::RelatedLinks => {
                if let PickerAction::OpenLink(target) = &entry.action
                    && target.kind == LinkKind::Article
                {
                    // Ignore re-fetches while a preview for the same link loads.
                    if self.picker_preview.loading && self.picker_preview.title == target.label {
                        return vec![];
                    }
                    let req = RequestId::new();
                    self.picker_preview = PickerPreview {
                        req: Some(req),
                        title: target.label.clone(),
                        loading: true,
                        ..Default::default()
                    };
                    self.start_activity(
                        req,
                        ActivityKind::FetchArticle,
                        format!("preview {}", target.target),
                        SurfaceId::Article,
                        true,
                    );
                    return vec![Cmd::LoadPeek {
                        req,
                        title: target.target.clone(),
                    }];
                }
                self.picker_preview = PickerPreview::default();
                vec![]
            }
            _ => {
                // Images, code and audio render from the document itself; no
                // network fetch needed.
                self.picker_preview = PickerPreview::default();
                vec![]
            }
        }
    }

    /// `p` in the outline picker: play/pause the audio attached to the
    /// selected entry.
    pub fn toggle_outline_audio(&mut self) -> Vec<Cmd> {
        let Some((filter, entry)) = self.selected_element() else {
            return vec![];
        };
        if filter != ElementFilter::Audio {
            self.toast("the selected entry is not audio", true);
            return vec![];
        }
        let line = match entry.action {
            PickerAction::ScrollToLine(line) => line,
            _ => {
                self.toast("this audio entry has no document anchor", true);
                return vec![];
            }
        };
        let Some(tab) = self.active_tab() else {
            return vec![];
        };
        let placement = tab
            .doc
            .as_ref()
            .and_then(|doc| {
                doc.media
                    .iter()
                    .find(|placement| placement.line as u16 == line)
            })
            .cloned();
        let Some(placement) = placement else {
            self.toast("audio is still laying out", true);
            return vec![];
        };
        let (kind, url) = parse_media_key(&placement.key);
        let key = format!("{kind}:{url}");
        if self.audio.key.as_deref() == Some(key.as_str()) && self.audio.data.is_some() {
            return self.toggle_audio_playback();
        }
        let mut spec = MediaSpec::new(url, kind);
        spec.title = placement.alt.clone();
        self.queue_media(&spec);
        // Trigger playback as soon as the decode finishes.
        self.audio.autoplay_intent = true;
        vec![]
    }

    /// Entries for the merged Saved picker, tagged by category for `[`/`]`.
    fn saved_picker_entries(&self) -> Vec<(SavedFilter, PickerEntry)> {
        let mut out = Vec::new();
        if let Some(bookmarks) = self.bookmarks.value() {
            for bookmark in bookmarks {
                out.push((
                    SavedFilter::Bookmarks,
                    PickerEntry::new(
                        bookmark.title.clone(),
                        "bookmark",
                        PickerAction::OpenArticle(bookmark.title.clone()),
                    )
                    .local(true),
                ));
            }
        }
        if let Some(lib) = self.library.value() {
            for article in &lib.articles {
                out.push((
                    SavedFilter::Offline,
                    PickerEntry::new(
                        article.title.clone(),
                        format!(
                            "{} · cached {}",
                            article.size_bytes_label(),
                            relative_time(article.last_opened_at)
                        ),
                        PickerAction::OpenArticle(article.title.clone()),
                    )
                    .local(true),
                ));
            }
        }
        out
    }

    fn offline_entries(&self) -> Vec<PickerEntry> {
        let mut out = Vec::new();
        if let Some(lib) = self.library.value() {
            for article in &lib.articles {
                out.push(
                    PickerEntry::new(
                        article.title.clone(),
                        format!(
                            "{} · opened {}",
                            article.size_bytes_label(),
                            relative_time(article.last_opened_at)
                        ),
                        PickerAction::OpenArticle(article.title.clone()),
                    )
                    .local(true),
                );
            }
        }
        if let Some(home) = self.home.value() {
            out.push(PickerEntry::new(
                format!("Feed · {}", home.date),
                format!("{} items", home.item_count()),
                PickerAction::OpenFeed(home.date),
            ));
        }
        if let Some(bookmarks) = self.bookmarks.value() {
            for bookmark in bookmarks {
                out.push(
                    PickerEntry::new(
                        bookmark.title.clone(),
                        "bookmark",
                        PickerAction::OpenArticle(bookmark.title.clone()),
                    )
                    .local(true),
                );
            }
        }
        if let Some(history) = self.history.value() {
            for entry in history {
                out.push(
                    PickerEntry::new(
                        entry.title.clone(),
                        "history",
                        PickerAction::OpenArticle(entry.title.clone()),
                    )
                    .local(true),
                );
            }
        }
        if let Some(lists) = self.lists.value() {
            for list in lists {
                out.push(PickerEntry::new(
                    list.name.clone(),
                    format!("{} articles", list.item_count),
                    PickerAction::OpenList(list.id),
                ));
            }
        }
        out
    }

    fn list_entries(&self) -> Vec<PickerEntry> {
        let mut out = Vec::new();
        if let Some(lists) = self.lists.value() {
            for list in lists {
                out.push(PickerEntry::new(
                    list.name.clone(),
                    format!("{} articles", list.item_count),
                    PickerAction::OpenList(list.id),
                ));
            }
        }
        out.push(PickerEntry::new(
            "＋ New reading list",
            "create a local list",
            PickerAction::CreateList,
        ));
        out
    }

    fn theme_entries(&self) -> Vec<PickerEntry> {
        self.themes
            .iter()
            .enumerate()
            .map(|(i, theme)| {
                PickerEntry::new(
                    theme.name.clone(),
                    if theme.light { "light" } else { "dark" },
                    PickerAction::SetTheme(i),
                )
            })
            .collect()
    }

    pub fn apply_picker_entries(&mut self) {
        let mode = self.picker.as_ref().map(|p| p.mode.clone());
        match mode {
            Some(PickerMode::Saved) => {
                let entries = self.saved_picker_entries();
                if let Some(picker) = self.picker.as_mut() {
                    picker.saved_base = entries;
                    picker.apply_saved_filter();
                }
            }
            Some(PickerMode::Lists) | Some(PickerMode::AddToList) => {
                let entries = self.list_entries();
                if let Some(picker) = self.picker.as_mut() {
                    picker.set_base(entries);
                }
            }
            Some(PickerMode::Theme) => {
                let entries = self.theme_entries();
                if let Some(picker) = self.picker.as_mut() {
                    picker.set_base(entries);
                }
            }
            Some(PickerMode::History) => {
                let entries = self
                    .history
                    .value()
                    .map(|h| {
                        h.iter()
                            .map(|entry| {
                                PickerEntry::new(
                                    entry.title.clone(),
                                    relative_time(entry.visited_at),
                                    PickerAction::OpenArticle(entry.title.clone()),
                                )
                                .local(true)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(picker) = self.picker.as_mut() {
                    picker.set_base(entries);
                }
            }
            _ => {}
        }
    }

    pub fn set_picker_search_results(&mut self, query: &str, hits: Vec<SearchHit>) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        if picker.mode != PickerMode::Search || picker.input.text() != query {
            return;
        }
        let mut entries: Vec<PickerEntry> = hits
            .iter()
            .map(|hit| {
                PickerEntry::new(
                    hit.title.clone(),
                    strip_snippet(&hit.snippet),
                    PickerAction::OpenArticle(hit.title.clone()),
                )
                .local(hit.local)
            })
            .collect();
        let local: Vec<PickerEntry> = picker.base.iter().filter(|e| e.local).cloned().collect();
        if !local.is_empty() {
            entries.extend(local.into_iter().take(5));
        }
        if entries.is_empty() && !query.trim().is_empty() {
            entries.push(PickerEntry::new(
                format!("Open “{query}”"),
                "no search results · open the exact title",
                PickerAction::OpenArticle(query.to_string()),
            ));
        }
        picker.loading = false;
        picker.error = None;
        picker.set_base(entries);
    }

    pub fn set_local_search_results(&mut self, query: &str, hits: Vec<SearchHit>) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        if picker.mode != PickerMode::Search || picker.input.text() != query {
            return;
        }
        let local: Vec<PickerEntry> = hits
            .iter()
            .map(|hit| {
                PickerEntry::new(
                    hit.title.clone(),
                    format!("cached · {}", strip_snippet(&hit.snippet)),
                    PickerAction::OpenArticle(hit.title.clone()),
                )
                .local(true)
            })
            .collect();
        if local.is_empty() {
            return;
        }
        let mut merged = local;
        for entry in &picker.base {
            if !entry.local
                && !merged
                    .iter()
                    .any(|e| e.label.eq_ignore_ascii_case(&entry.label))
            {
                merged.push(entry.clone());
            }
        }
        picker.set_base(merged);
    }

    pub fn close_picker(&mut self) {
        // `esc`/`PickerClose` reverts a live theme preview; `enter` clears the
        // revert snapshot first so the chosen palette sticks.
        let previewing = self
            .picker
            .as_ref()
            .map(|picker| picker.mode == PickerMode::Theme)
            .unwrap_or(false);
        if previewing {
            self.revert_picker_theme();
        }
        self.picker = None;
        self.overlay = Overlay::None;
        self.search_due = None;
        self.search_results = Loadable::Idle;
        self.picker_preview = PickerPreview::default();
    }

    pub fn take_pending(&mut self) -> Vec<Cmd> {
        std::mem::take(&mut self.pending_cmds)
    }

    pub fn queue(&mut self, cmd: Cmd) {
        self.pending_cmds.push(cmd);
    }

    pub fn queue_media(&mut self, spec: &MediaSpec) {
        if !self.images_enabled() {
            return;
        }
        let key = spec.key();
        if self.images.contains_key(&key) {
            return;
        }
        self.images.insert(
            key.clone(),
            Loadable::Loading {
                stage: LoadStage::Downloading,
                progress: None,
                started_at: Instant::now(),
                previous: None,
                cancel: tokio_util::sync::CancellationToken::new(),
            },
        );
        match spec.kind {
            MediaKind::Audio => {
                self.audio.key = Some(key.clone());
                self.audio.title = spec.title.clone();
                self.audio.status = crate::media::AudioStatus::Loading;
                self.audio.data = None;
                self.audio.error = None;
                self.audio.position = 0.0;
                self.queue(Cmd::LoadAudio {
                    key,
                    url: spec.url.clone(),
                    title: spec.title.clone(),
                });
            }
            MediaKind::Video => {
                self.video.key = Some(key.clone());
                self.video.title = spec.title.clone();
                self.video.status = crate::media::VideoStatus::Loading;
                self.video.frame = None;
                self.video.position = 0.0;
                self.video.error = None;
                self.queue(Cmd::LoadVideo {
                    key,
                    url: spec.url.clone(),
                    title: spec.title.clone(),
                });
            }
            MediaKind::Document => {}
            _ => self.queue(Cmd::LoadMedia {
                key,
                url: spec.url.clone(),
                kind: spec.kind,
            }),
        }
    }

    pub fn queue_math(&mut self, tex: &str) {
        if tex.trim().is_empty() || !self.config.media.enabled {
            return;
        }
        let key = math_key(tex, self.is_light());
        if self.math_images.contains_key(&key) {
            return;
        }
        self.math_images.insert(
            key.clone(),
            Loadable::Loading {
                stage: LoadStage::Decoding,
                progress: None,
                started_at: Instant::now(),
                previous: None,
                cancel: tokio_util::sync::CancellationToken::new(),
            },
        );
        self.queue(Cmd::RenderMath {
            key,
            tex: tex.to_string(),
            light: self.is_light(),
        });
    }

    pub fn queue_article_assets(&mut self, article: &Article) {
        for spec in article.media.iter().take(24) {
            self.queue_media(spec);
        }
        for tex in article.math().take(32) {
            self.queue_math(tex);
        }
    }

    pub fn ensure_layout(&mut self) {
        let width = self.width.max(24);
        let theme = self.theme();
        let light = self.is_light();
        let media_enabled = self.images_enabled();
        let syntax_enabled = self.config.render.syntax_highlight;
        let max_media_height = self.article_view_height.saturating_sub(4).clamp(6, 22);
        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        if tab.doc_width == width
            && tab.doc_media_enabled == media_enabled
            && tab.doc_max_media_height == max_media_height
        {
            return;
        }
        let Some(article) = tab.article.value() else {
            return;
        };
        let opts = crate::render::layout::LayoutOptions {
            light,
            media_enabled,
            syntax_enabled,
            max_media_height,
        };
        let doc = build_document(article, width.saturating_sub(6).max(24), theme, opts);
        tab.doc_width = width;
        tab.doc_media_enabled = media_enabled;
        tab.doc_max_media_height = max_media_height;
        tab.doc = Some(doc);
    }

    pub fn article_total_height(&self) -> u16 {
        self.active_tab()
            .and_then(|t| t.doc.as_ref())
            .map(|d| d.lines.len() as u16)
            .unwrap_or(0)
    }

    pub fn scroll_article(&mut self, delta: i32) {
        let total = self.article_total_height() as i32;
        let view = self.article_view_height.max(1) as i32;
        let max = (total - view + 4).max(0);
        if let Some(tab) = self.active_tab_mut() {
            tab.scroll = (tab.scroll as i32 + delta).clamp(0, max) as u16;
        }
    }

    pub fn scroll_to_article_line(&mut self, line: u16) {
        if let Some(tab) = self.active_tab_mut() {
            tab.scroll = line.saturating_sub(2);
        }
    }

    /// Open the floating in-page search bar on the current article.
    pub fn open_finder(&mut self) {
        if self.screen != Screen::Article {
            return;
        }
        if self.active_tab().is_none_or(|tab| tab.doc.is_none()) {
            self.toast("article is still laying out", true);
            return;
        }
        self.finder = Some(FinderState::new());
    }

    pub fn close_finder(&mut self) {
        self.finder = None;
    }

    /// Recompute the matched line indices from the current query.
    pub fn refresh_finder_matches(&mut self) {
        let Some(finder) = self.finder.as_ref() else {
            return;
        };
        let query = finder.input.text().to_lowercase();
        if query.is_empty() {
            if let Some(finder) = self.finder.as_mut() {
                finder.matches.clear();
                finder.current = 0;
            }
            return;
        }
        let query_chars: Vec<char> = query.chars().collect();
        let matches = {
            let Some(doc) = self.active_tab().and_then(|tab| tab.doc.as_ref()) else {
                return;
            };
            let mut matches = Vec::new();
            for (index, line) in doc.lines.iter().enumerate() {
                let text: String = line
                    .spans
                    .iter()
                    .flat_map(|span| span.content.chars())
                    .collect::<String>()
                    .to_lowercase();
                let chars: Vec<char> = text.chars().collect();
                if chars.len() >= query_chars.len()
                    && chars
                        .windows(query_chars.len())
                        .any(|window| window == query_chars.as_slice())
                {
                    matches.push(index);
                }
            }
            matches
        };
        if let Some(finder) = self.finder.as_mut() {
            finder.matches = matches;
            finder.current = finder.current.min(finder.matches.len().saturating_sub(1));
        }
    }

    /// Jump the article view to the highlighted match.
    pub fn finder_reveal(&mut self) {
        let Some(finder) = &self.finder else {
            return;
        };
        let Some(&line) = finder.matches.get(finder.current) else {
            return;
        };
        let view = self.article_view_height.max(1) as usize;
        if let Some(tab) = self.active_tab_mut() {
            let top = tab.scroll as usize;
            if line < top + 1 {
                tab.scroll = line.saturating_sub(1) as u16;
            } else if line >= top + view.saturating_sub(1) {
                tab.scroll = (line + 2).saturating_sub(view) as u16;
            }
        }
    }

    pub fn finder_step(&mut self, delta: i32) {
        let next = {
            let Some(finder) = self.finder.as_mut() else {
                return;
            };
            if finder.matches.is_empty() {
                return;
            }
            let len = finder.matches.len() as i32;
            (finder.current as i32 + delta).rem_euclid(len) as usize
        };
        if let Some(finder) = self.finder.as_mut() {
            finder.current = next;
        }
        self.finder_reveal();
    }

    pub fn handle_finder_key(
        &mut self,
        code: crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Vec<Cmd> {
        use crossterm::event::KeyCode;
        let ctrl = modifiers.contains(crossterm::event::KeyModifiers::CONTROL);
        match code {
            KeyCode::Esc => {
                self.close_finder();
                vec![]
            }
            KeyCode::Enter => {
                self.finder_step(
                    if modifiers.contains(crossterm::event::KeyModifiers::SHIFT) {
                        -1
                    } else {
                        1
                    },
                );
                vec![]
            }
            KeyCode::Down | KeyCode::F(3) => {
                self.finder_step(1);
                vec![]
            }
            KeyCode::Up | KeyCode::BackTab => {
                self.finder_step(-1);
                vec![]
            }
            KeyCode::Backspace => {
                if let Some(finder) = self.finder.as_mut() {
                    finder.input.backspace();
                }
                self.refresh_finder_matches();
                self.finder_reveal();
                vec![]
            }
            KeyCode::Delete => {
                if let Some(finder) = self.finder.as_mut() {
                    finder.input.delete();
                }
                self.refresh_finder_matches();
                vec![]
            }
            KeyCode::Left => {
                if let Some(finder) = self.finder.as_mut() {
                    finder.input.left();
                }
                vec![]
            }
            KeyCode::Right => {
                if let Some(finder) = self.finder.as_mut() {
                    finder.input.right();
                }
                vec![]
            }
            KeyCode::Home => {
                if let Some(finder) = self.finder.as_mut() {
                    finder.input.home();
                }
                vec![]
            }
            KeyCode::End => {
                if let Some(finder) = self.finder.as_mut() {
                    finder.input.end();
                }
                vec![]
            }
            KeyCode::Char('u') if ctrl => {
                if let Some(finder) = self.finder.as_mut() {
                    finder.input.clear();
                }
                self.refresh_finder_matches();
                vec![]
            }
            KeyCode::Char(c) if !ctrl => {
                if let Some(finder) = self.finder.as_mut() {
                    finder.input.insert(c);
                }
                self.refresh_finder_matches();
                self.finder_reveal();
                vec![]
            }
            _ => vec![],
        }
    }

    /// Snapshot the link to peek at with Ctrl+K: the hovered link, else the
    /// first link visible in the current viewport.
    pub fn cursor_link_target(&self) -> Option<LinkTarget> {
        if let Some(hover) = &self.hover
            && hover.target.kind != LinkKind::Citation
        {
            return Some(hover.target.clone());
        }
        let tab = self.active_tab()?;
        let doc = tab.doc.as_ref()?;
        let top = tab.scroll as usize;
        let bottom = top + self.article_view_height.max(1) as usize;
        doc.links
            .iter()
            .find(|link| link.line >= top && link.line < bottom)
            .map(|link| link.target.clone())
    }

    /// Jump to the next/previous article link (Alt+J/Alt+K).
    pub fn jump_link(&mut self, forward: bool) -> Vec<Cmd> {
        if !self.config.keys.link_jump {
            self.toast("link jumping is disabled (config [keys].link_jump)", true);
            return vec![];
        }
        let (len, top, next_line, next_target) = {
            let Some(tab) = self.active_tab() else {
                return vec![];
            };
            let Some(doc) = tab.doc.as_ref() else {
                self.toast("article is still loading", true);
                return vec![];
            };
            if doc.links.is_empty() {
                self.toast("no links in this article", true);
                return vec![];
            }
            let len = doc.links.len();
            let top = tab.scroll as usize;
            let current = self.link_cursor.filter(|&i| i < len);
            let next = match (current, forward) {
                (Some(i), true) => (i + 1) % len,
                (Some(i), false) => (i + len - 1) % len,
                (None, true) => doc.links.iter().position(|l| l.line >= top).unwrap_or(0),
                (None, false) => doc
                    .links
                    .iter()
                    .rposition(|l| l.line <= top + 1)
                    .unwrap_or(len.saturating_sub(1)),
            };
            (
                len,
                top,
                doc.links[next].line as u16,
                doc.links[next].target.clone(),
            )
        };
        let _ = (len, top);
        self.link_cursor = Some(
            self.active_tab()
                .and_then(|t| t.doc.as_ref())
                .and_then(|d| d.links.iter().position(|l| l.target == next_target))
                .unwrap_or(0),
        );
        let view = self.article_view_height.saturating_sub(2).max(1);
        let target_scroll = next_line.saturating_sub(view / 3);
        if let Some(tab) = self.active_tab_mut() {
            tab.scroll = target_scroll;
        }
        if self.config.keys.peek_on_jump {
            return self.peek_target(next_target);
        }
        vec![]
    }

    fn peek_target(&mut self, target: LinkTarget) -> Vec<Cmd> {
        self.peek = PeekState {
            title: target.label.clone(),
            target: Some(target.clone()),
            summary: None,
            image: None,
            req: None,
            error: None,
            loading: false,
        };
        self.overlay = Overlay::Peek;
        if target.kind == LinkKind::Article {
            let req = RequestId::new();
            self.peek.req = Some(req);
            self.peek.loading = true;
            self.start_activity(
                req,
                ActivityKind::FetchArticle,
                format!("peek {}", target.target),
                SurfaceId::Article,
                true,
            );
            return vec![Cmd::LoadPeek {
                req,
                title: target.target,
            }];
        }
        vec![]
    }

    /// Open the peek overlay for the link under the cursor and kick off a
    /// summary fetch (article links only; web/media links show their label).
    pub fn peek_cursor_link(&mut self) -> Vec<Cmd> {
        if self.config.keys.link_jump
            && let Some(idx) = self.link_cursor
            && let Some(target) = self
                .active_tab()
                .and_then(|t| t.doc.as_ref())
                .and_then(|d| d.links.get(idx))
                .map(|l| l.target.clone())
        {
            return self.peek_target(target);
        }
        let Some(target) = self.cursor_link_target() else {
            self.toast("no link under the cursor", true);
            return vec![];
        };
        self.peek_target(target)
    }

    pub fn toggle_bookmark_current(&mut self) -> Vec<Cmd> {
        let Some(tab) = self.active_tab() else {
            return vec![];
        };
        let title = tab.title.clone();
        vec![Cmd::ToggleBookmark { title }]
    }

    pub fn add_current_to_list(&mut self) -> Vec<Cmd> {
        let Some(tab) = self.active_tab() else {
            return vec![];
        };
        if tab.article.is_ready() {
            let title = tab.title.clone();
            self.open_add_to_list(title)
        } else {
            vec![]
        }
    }

    pub fn next_theme(&mut self) -> Vec<Cmd> {
        if self.themes.is_empty() {
            return vec![];
        }
        self.theme_idx = (self.theme_idx + 1) % self.themes.len();
        self.persist_theme();
        self.invalidate_layout();
        vec![]
    }

    pub fn prev_theme(&mut self) -> Vec<Cmd> {
        if self.themes.is_empty() {
            return vec![];
        }
        self.theme_idx = self
            .theme_idx
            .checked_sub(1)
            .unwrap_or(self.themes.len() - 1);
        self.persist_theme();
        self.invalidate_layout();
        vec![]
    }

    pub fn set_theme(&mut self, idx: usize) -> Vec<Cmd> {
        if idx < self.themes.len() {
            self.theme_idx = idx;
            self.persist_theme();
            self.invalidate_layout();
            self.toast(format!("theme · {}", self.themes[idx].name), false);
        }
        vec![]
    }

    fn persist_theme(&mut self) {
        let name = self.current_theme().name.clone();
        if self.last_saved_theme.as_deref() == Some(name.as_str()) {
            return;
        }
        self.config.theme.name = name.clone();
        let _ = self.config.save();
        self.last_saved_theme = Some(name);
    }

    pub fn invalidate_layout(&mut self) {
        for tab in &mut self.tabs {
            tab.doc_width = 0;
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toast = Some(Toast {
            text: text.into(),
            error,
            created: Instant::now(),
        });
    }

    pub fn handle_msg(&mut self, msg: Msg) -> Vec<Cmd> {
        match msg {
            Msg::Tick => self.tick(),
            Msg::Resize(w, h) => {
                self.width = w;
                self.height = h;
                self.invalidate_layout();
                vec![]
            }
            Msg::Quit => {
                self.should_quit = true;
                vec![]
            }
            Msg::FeedLoaded { req, date, result } => {
                if req != self.feed_req || date != self.feed_date {
                    return vec![];
                }
                self.finish_activity(req);
                match result {
                    Ok(feed) => {
                        let source = feed.source;
                        let featured = feed
                            .sections
                            .iter()
                            .position(|s| s.kind == crate::core::FeedKind::FeaturedArticle)
                            .unwrap_or(0);
                        self.home = Loadable::ready(feed, source);
                        self.feed_section = featured;
                        self.feed_item = 0;
                        self.feed_scroll = 0;
                        self.queue_feed_thumbnails();
                        self.toast(format!("feed · {date}"), false);
                    }
                    Err(err) => {
                        if let Some(cached) = self.home.value_arc() {
                            self.home = Loadable::failed_with(err.clone(), Some(cached));
                        } else {
                            self.home = Loadable::failed(err.clone());
                        }
                        if err.kind != crate::core::LoadErrorKind::Cancelled {
                            self.toast(err.message.clone(), true);
                        }
                    }
                }
                vec![]
            }
            Msg::ArticleLoaded { req, tab, result } => {
                self.finish_activity(req);
                let Some(target) = self.tabs.iter_mut().find(|t| t.id == tab) else {
                    return vec![];
                };
                if target.request != req {
                    return vec![];
                }
                match result {
                    Ok(article) => {
                        let source = article.source;
                        target.title = article.display_title.clone();
                        target.summary = article.summary.clone();
                        target.media = article.media.clone();
                        target.article = Loadable::ready(article.clone(), source);
                        target.scroll = 0;
                        target.doc = None;
                        target.doc_width = 0;
                        self.active_tab = Some(tab);
                        self.screen = Screen::Article;
                        self.overlay = Overlay::None;
                        self.queue_article_assets(&article);
                        self.toast(format!("opened · {}", article.title), false);
                        let mut cmds = self.take_pending();
                        cmds.push(Cmd::LoadBookmarkState {
                            title: article.title.clone(),
                        });
                        cmds
                    }
                    Err(err) => {
                        let previous = target.article.value_arc();
                        target.article = if let Some(prev) = previous {
                            Loadable::failed_with(err.clone(), Some(prev))
                        } else {
                            Loadable::failed(err.clone())
                        };
                        if err.kind != crate::core::LoadErrorKind::Cancelled {
                            self.toast(err.message, true);
                        }
                        vec![]
                    }
                }
            }
            Msg::SearchResults { req, query, result } => {
                if let Some(picker) = self.picker.as_ref()
                    && picker.online_req == Some(req)
                {
                    self.finish_activity(req);
                    match result {
                        Ok(hits) => self.set_picker_search_results(&query, hits),
                        Err(err) => {
                            if let Some(picker) = self.picker.as_mut() {
                                picker.loading = false;
                                picker.error = Some(err.message);
                            }
                        }
                    }
                }
                vec![]
            }
            Msg::LocalSearchResults { query, result } => {
                if let Ok(hits) = result {
                    self.set_local_search_results(&query, hits);
                }
                vec![]
            }
            Msg::BookmarksLoaded(result) => {
                match result {
                    Ok(items) => {
                        self.bookmarks = Loadable::ready(items, Source::Local);
                        self.bookmark_index = self.bookmark_index.min(
                            self.bookmarks
                                .value()
                                .map(|b| b.len())
                                .unwrap_or(0)
                                .saturating_sub(1),
                        );
                    }
                    Err(err) => {
                        self.bookmarks = Loadable::failed(err.clone());
                        self.toast(err.message, true);
                    }
                }
                self.apply_picker_entries();
                vec![]
            }
            Msg::HistoryLoaded(result) => {
                match result {
                    Ok(items) => {
                        self.history = Loadable::ready(items, Source::Local);
                        self.history_index = self.history_index.min(
                            self.history
                                .value()
                                .map(|h| h.len())
                                .unwrap_or(0)
                                .saturating_sub(1),
                        );
                    }
                    Err(err) => {
                        self.history = Loadable::failed(err.clone());
                        self.toast(err.message, true);
                    }
                }
                self.apply_picker_entries();
                vec![]
            }
            Msg::ListsLoaded(result) => {
                match result {
                    Ok(items) => {
                        self.lists = Loadable::ready(items, Source::Local);
                        self.list_index = self.list_index.min(
                            self.lists
                                .value()
                                .map(|l| l.len())
                                .unwrap_or(0)
                                .saturating_sub(1),
                        );
                    }
                    Err(err) => {
                        self.lists = Loadable::failed(err.clone());
                        self.toast(err.message, true);
                    }
                }
                self.apply_picker_entries();
                vec![]
            }
            Msg::LibraryLoaded(result) => {
                match result {
                    Ok(lib) => {
                        self.library = Loadable::ready(lib, Source::Local);
                        self.apply_picker_entries();
                    }
                    Err(err) => {
                        self.library = Loadable::failed(err.clone());
                    }
                }
                vec![]
            }
            Msg::ListItemsLoaded { list_id, result } => {
                if self.active_list != Some(list_id) {
                    return vec![];
                }
                match result {
                    Ok(items) => {
                        self.list_items = Loadable::ready(items, Source::Local);
                        self.list_item_index = self.list_item_index.min(
                            self.list_items
                                .value()
                                .map(|l| l.len())
                                .unwrap_or(0)
                                .saturating_sub(1),
                        );
                    }
                    Err(err) => {
                        self.list_items = Loadable::failed(err.clone());
                        self.toast(err.message, true);
                    }
                }
                vec![]
            }
            Msg::ListCreated(result) => match result {
                Ok(list) => {
                    self.toast(format!("created · {}", list.name), false);
                    let mut cmds = vec![Cmd::LoadLists, Cmd::LoadLibrary];
                    if let Some(title) = self.pending_list_add.take() {
                        cmds.push(Cmd::AddToList {
                            list_id: list.id,
                            title,
                        });
                    }
                    cmds
                }
                Err(err) => {
                    self.toast(err.message, true);
                    vec![]
                }
            },
            Msg::ListDeleted { list_id, result } => {
                if let Err(err) = result {
                    self.toast(err.message, true);
                } else {
                    if self.active_list == Some(list_id) {
                        self.active_list = None;
                        self.screen = Screen::Lists;
                    }
                    self.toast("reading list deleted", false);
                }
                self.confirm_list = None;
                vec![Cmd::LoadLists, Cmd::LoadLibrary]
            }
            Msg::ArticleAddedToList {
                list_id,
                title,
                result,
            } => {
                if let Err(err) = result {
                    self.toast(err.message, true);
                } else {
                    self.toast(format!("saved “{title}”"), false);
                    let _ = list_id;
                }
                self.pending_list_add = None;
                self.picker = None;
                vec![Cmd::LoadLists, Cmd::LoadLibrary]
            }
            Msg::ArticleRemovedFromList { result, .. } => {
                if let Err(err) = result {
                    self.toast(err.message, true);
                } else {
                    self.toast("removed from reading list", false);
                }
                if let Some(list_id) = self.active_list {
                    vec![Cmd::LoadListItems { list_id }, Cmd::LoadLists]
                } else {
                    vec![Cmd::LoadLists]
                }
            }
            Msg::BookmarkStateLoaded { title, bookmarked } => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.title == title) {
                    tab.bookmarked = bookmarked;
                }
                vec![]
            }
            Msg::PeekLoaded {
                req,
                title,
                summary,
                image,
                error,
            } => {
                self.finish_activity(req);
                // Route the result to whichever peek it belongs to.
                if self.picker_preview.req == Some(req) {
                    self.picker_preview.summary = Some(summary);
                    self.picker_preview.image = image;
                    self.picker_preview.error = error;
                    self.picker_preview.loading = false;
                    self.picker_preview.title = title;
                    if let Some(url) = self.picker_preview.image.clone() {
                        self.queue_media(&MediaSpec::new(url, MediaKind::Image));
                    }
                } else if self.peek.req == Some(req) {
                    self.peek.summary = Some(summary);
                    self.peek.image = image;
                    self.peek.error = error;
                    self.peek.loading = false;
                    self.peek.title = title;
                    if let Some(url) = self.peek.image.clone() {
                        self.queue_media(&MediaSpec::new(url, MediaKind::Image));
                    }
                }
                vec![]
            }
            Msg::BookmarkToggled {
                title,
                bookmarked,
                result,
            } => {
                if let Err(err) = result {
                    self.toast(err.message, true);
                } else {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.title == title) {
                        tab.bookmarked = bookmarked;
                    }
                    self.toast(
                        if bookmarked {
                            format!("saved · {title}")
                        } else {
                            format!("removed · {title}")
                        },
                        false,
                    );
                }
                vec![Cmd::LoadBookmarks, Cmd::LoadLibrary]
            }
            Msg::CachePurged(result) => {
                match result {
                    Ok(bytes) => {
                        self.toast(format!("cache purged · {}", bytes_label(bytes)), false)
                    }
                    Err(err) => self.toast(err.message, true),
                }
                vec![Cmd::LoadLibrary, Cmd::LoadBookmarks, Cmd::LoadHistory]
            }
            Msg::MediaLoaded { key, result } | Msg::MathRendered { key, result } => {
                if let Err(err) = &result {
                    self.toast(format!("media · {}", err.message), true);
                }
                if key.starts_with("math:") {
                    self.math_images.insert(key, loadable_from_result(result));
                } else {
                    self.images.insert(key, loadable_from_result(result));
                }
                vec![]
            }
            Msg::AudioLoaded { key, result } => {
                match result {
                    Ok(data) => {
                        let autoplay = self.config.media.autoplay || self.audio.autoplay_intent;
                        self.audio = AudioState::ready(key, data, autoplay);
                        self.audio.autoplay_intent = false;
                        if autoplay {
                            self.toast("audio · playing", false);
                        } else {
                            self.toast("audio · space to play", false);
                        }
                    }
                    Err(err) => {
                        self.audio.error = Some(err.message.clone());
                        self.toast(format!("audio · {}", err.message), true);
                    }
                }
                vec![]
            }
            Msg::VideoFrame {
                key,
                index,
                total,
                frame,
                position,
            } => {
                self.video.set_frame(key, index, total, frame, position);
                vec![]
            }
            Msg::VideoLoaded {
                key,
                total,
                duration,
            } => {
                self.video.ready(key, total, duration);
                self.toast("video · space to play", false);
                vec![]
            }
            Msg::VideoFailed { key, error } => {
                self.video.fail(key, error.message.clone());
                self.toast(format!("video · {}", error.message), true);
                vec![]
            }
            Msg::VideoStateChanged {
                key,
                playing,
                position,
                volume,
            } => {
                self.video.apply_state(&key, playing, position, volume);
                vec![]
            }
            Msg::Toast { text, error } => {
                self.toast(text, error);
                vec![]
            }
            Msg::OpenUrlResult { url, result } => {
                match result {
                    Ok(()) => self.toast(format!("opened {url}"), false),
                    Err(err) => self.toast(format!("open · {}", err.message), true),
                }
                vec![]
            }
        }
    }

    fn tick(&mut self) -> Vec<Cmd> {
        self.tick = self.tick.wrapping_add(1);
        self.prune_stale_activity();
        if let Some(toast) = &self.toast
            && toast.expired()
        {
            self.toast = None;
        }
        if self.screen == Screen::Splash && self.tick >= SPLASH_TICKS {
            self.screen = Screen::Home;
            if self.home.value().is_none() && self.home.is_loading() {
                // feed is already in flight
            }
        }
        if let Some(due) = self.search_due
            && Instant::now() >= due
        {
            self.search_due = None;
            let Some(picker) = self.picker.as_ref() else {
                return vec![];
            };
            if picker.mode == PickerMode::Search {
                let query = picker.input.text();
                if query.trim().len() >= 2 {
                    let req = RequestId::new();
                    if let Some(picker) = self.picker.as_mut() {
                        picker.online_req = Some(req);
                        picker.loading = true;
                    }
                    self.search_req = req;
                    self.start_activity(
                        req,
                        ActivityKind::Search,
                        query.clone(),
                        SurfaceId::Search,
                        true,
                    );
                    return vec![
                        Cmd::Search {
                            req,
                            query: query.clone(),
                        },
                        Cmd::SearchLocal { query },
                    ];
                }
            }
        }
        self.queue_feed_thumbnails();
        self.take_pending()
    }

    pub fn queue_feed_thumbnails(&mut self) {
        let Some(feed) = self.home.value_arc() else {
            return;
        };
        let mut specs = Vec::new();
        for section in &feed.sections {
            for item in &section.items {
                if specs.len() >= 16 {
                    break;
                }
                if let Some(url) = &item.thumbnail {
                    specs.push(MediaSpec {
                        url: url.clone(),
                        kind: MediaKind::Image,
                        title: item.title.clone(),
                        description: item.extract.clone(),
                        mime: None,
                        poster: None,
                        duration: None,
                    });
                }
            }
        }
        for spec in specs {
            self.queue_media(&spec);
        }
    }

    fn load_media_lab(&mut self) -> Vec<Cmd> {
        let Some(item) = self.media_lab.items.get(self.media_lab.selected).cloned() else {
            return vec![];
        };
        match item.kind {
            DemoKind::Image | DemoKind::Gif | DemoKind::Svg => {
                if let Some(url) = item.url {
                    let kind = match item.kind {
                        DemoKind::Gif => MediaKind::Gif,
                        DemoKind::Svg => MediaKind::Svg,
                        _ => MediaKind::Image,
                    };
                    let spec = MediaSpec::new(url, kind);
                    self.queue_media(&spec);
                }
            }
            DemoKind::Math => {
                if let Some(tex) = item.text {
                    self.queue_math(tex);
                }
            }
            DemoKind::Audio => {
                if let Some(url) = item.url {
                    let spec = MediaSpec::new(url, MediaKind::Audio);
                    self.queue_media(&spec);
                }
            }
            DemoKind::Video => {
                if let Some(url) = item.url {
                    let spec = MediaSpec::new(url, MediaKind::Video);
                    self.queue_media(&spec);
                }
            }
            DemoKind::Code => {}
        }
        self.take_pending()
    }

    pub fn open_media_lab_item(&mut self, idx: usize) -> Vec<Cmd> {
        if idx >= self.media_lab.items.len() {
            return vec![];
        }
        self.media_lab.selected = idx;
        self.load_media_lab()
    }

    pub fn media_lab_code(&self) -> &'static str {
        self.media_lab
            .items
            .get(self.media_lab.selected)
            .and_then(|i| i.text)
            .unwrap_or("")
    }

    pub fn media_lab_key(&self) -> Option<String> {
        let item = self.media_lab.items.get(self.media_lab.selected)?;
        if let Some(url) = item.url {
            Some(format!("{}:{}", media_kind_for_demo(item.kind), url))
        } else {
            item.text.map(|tex| math_key(tex, self.is_light()))
        }
    }

    pub fn toggle_media_lab_playback(&mut self) -> Vec<Cmd> {
        let Some(item) = self.media_lab.items.get(self.media_lab.selected) else {
            return vec![];
        };
        match item.kind {
            DemoKind::Audio => self.toggle_audio_playback(),
            DemoKind::Video if self.video.key.is_some() => {
                vec![Cmd::VideoControl(VideoControl::PlayPause)]
            }
            _ => vec![],
        }
    }

    /// Toggle decoded audio without touching the UI thread. The runtime drains
    /// the request on its next poll and talks to rodio from the event loop.
    pub fn toggle_audio_playback(&mut self) -> Vec<Cmd> {
        if self.audio.data.is_none() {
            return vec![];
        }
        if self.audio.status == crate::media::AudioStatus::Playing {
            self.audio.pause_requested = true;
            self.audio.status = crate::media::AudioStatus::Paused;
        } else {
            self.audio.play_requested = true;
            self.audio.status = crate::media::AudioStatus::Playing;
        }
        vec![]
    }

    pub fn handle_paste(&mut self, text: &str) -> Vec<Cmd> {
        if let Some(picker) = self.picker.as_mut() {
            picker.input.paste(text);
            return self.picker_input_changed();
        }
        if let Some(new_list) = self.new_list.as_mut() {
            new_list.input.paste(text);
        }
        vec![]
    }

    pub fn handle_picker_key(
        &mut self,
        code: crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Vec<Cmd> {
        use crossterm::event::KeyCode;
        let Some(picker) = self.picker.as_mut() else {
            return vec![];
        };
        match code {
            KeyCode::Esc => {
                self.close_picker();
                vec![]
            }
            KeyCode::Char(']') if picker.mode == PickerMode::ArticleElements => {
                picker.cycle_element_filter(true);
                vec![]
            }
            KeyCode::Char('[') if picker.mode == PickerMode::ArticleElements => {
                picker.cycle_element_filter(false);
                vec![]
            }
            KeyCode::Char(']') if picker.mode == PickerMode::Saved => {
                picker.cycle_saved_filter(true);
                vec![]
            }
            KeyCode::Char('[') if picker.mode == PickerMode::Saved => {
                picker.cycle_saved_filter(false);
                vec![]
            }
            KeyCode::Char('x') if picker.mode == PickerMode::Tabs => {
                let target = picker
                    .selected_entry()
                    .and_then(|entry| match &entry.action {
                        PickerAction::SwitchTab(id) => Some(*id),
                        _ => None,
                    });
                if let Some(id) = target {
                    self.close_tab(id);
                    if self.tabs.is_empty() {
                        self.close_picker();
                    } else {
                        let entries = self.tab_entries();
                        if let Some(picker) = self.picker.as_mut() {
                            picker.set_base(entries);
                        }
                    }
                }
                vec![]
            }
            KeyCode::Up => {
                picker.select_prev();
                self.preview_picker_theme();
                self.refresh_picker_preview()
            }
            KeyCode::Down | KeyCode::Tab => {
                picker.select_next();
                self.preview_picker_theme();
                self.refresh_picker_preview()
            }
            KeyCode::Char('p') if picker.mode == PickerMode::ArticleElements => {
                self.toggle_outline_audio()
            }
            KeyCode::Enter => {
                let selected = picker.selected_entry().cloned();
                let query = picker.input.text();
                let mode = picker.mode.clone();
                if let Some(entry) = selected {
                    return self.activate_picker_action(entry.action);
                }
                if mode == PickerMode::Search && !query.trim().is_empty() {
                    self.close_picker();
                    return self.open_article(query, false);
                }
                vec![]
            }
            KeyCode::Char('n') if modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                self.start_new_list(None);
                vec![]
            }
            KeyCode::Backspace => {
                picker.input.backspace();
                self.picker_input_changed()
            }
            KeyCode::Delete => {
                picker.input.delete();
                self.picker_input_changed()
            }
            KeyCode::Left => {
                picker.input.left();
                vec![]
            }
            KeyCode::Right => {
                picker.input.right();
                vec![]
            }
            KeyCode::Home => {
                picker.input.home();
                vec![]
            }
            KeyCode::End => {
                picker.input.end();
                vec![]
            }
            KeyCode::Char('u') if modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                picker.input.clear();
                self.picker_input_changed()
            }
            KeyCode::Char('w') if modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                picker.input.delete_word();
                self.picker_input_changed()
            }
            KeyCode::Char(c) if !modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                picker.input.insert(c);
                self.picker_input_changed()
            }
            _ => vec![],
        }
    }

    fn picker_input_changed(&mut self) -> Vec<Cmd> {
        let mode = self.picker.as_ref().map(|p| p.mode.clone());
        if let Some(picker) = self.picker.as_mut() {
            picker.filter();
        }
        if mode == Some(PickerMode::Search) {
            self.search_due = Some(Instant::now() + Duration::from_millis(220));
        } else {
            self.apply_picker_entries();
        }
        vec![]
    }

    fn activate_picker_action(&mut self, action: PickerAction) -> Vec<Cmd> {
        match action {
            PickerAction::OpenArticle(title) => {
                self.close_picker();
                self.open_article(title, false)
            }
            PickerAction::OpenList(id) => {
                self.close_picker();
                self.open_list(id)
            }
            PickerAction::OpenFeed(date) => {
                self.close_picker();
                self.screen = Screen::Home;
                self.load_feed(date, false)
            }
            PickerAction::CreateList => {
                let title = self.pending_list_add.clone();
                self.close_picker();
                self.start_new_list(title);
                vec![]
            }
            PickerAction::AddToList(id) => {
                let Some(title) = self.pending_list_add.take() else {
                    self.close_picker();
                    return vec![];
                };
                self.close_picker();
                vec![Cmd::AddToList { list_id: id, title }]
            }
            PickerAction::SetTheme(idx) => {
                // Commit: clear the revert snapshot so `close_picker` keeps the
                // chosen palette instead of restoring the old one.
                self.theme_revert = None;
                self.close_picker();
                self.set_theme(idx)
            }
            PickerAction::GoTo(screen) => {
                self.close_picker();
                self.set_screen(screen)
            }
            PickerAction::ScrollToLine(line) => {
                self.close_picker();
                self.scroll_to_article_line(line);
                vec![]
            }
            PickerAction::OpenLink(target) => {
                self.close_picker();
                match target.kind {
                    LinkKind::Article => self.open_article(target.target, false),
                    LinkKind::External | LinkKind::Media => {
                        vec![Cmd::OpenUrl { url: target.target }]
                    }
                    LinkKind::Citation => vec![],
                }
            }
            PickerAction::SwitchTab(id) => {
                self.close_picker();
                self.switch_tab(id);
                vec![]
            }
            PickerAction::RunCommand(command) => {
                self.close_picker();
                self.run_command(command)
            }
        }
    }

    /// Execute a command palette entry.
    pub fn run_command(&mut self, command: CommandKind) -> Vec<Cmd> {
        match command {
            CommandKind::Search => self.open_search_picker(),
            CommandKind::Saved => self.open_saved_picker(),
            CommandKind::Home => self.set_screen(Screen::Home),
            CommandKind::History => self.open_history_picker(),
            CommandKind::Lists => self.open_lists_picker(),
            CommandKind::MediaLab => self.set_screen(Screen::MediaLab),
            CommandKind::Themes => self.open_theme_picker(),
            CommandKind::Tabs => self.open_tabs_picker(),
            CommandKind::Calendar => {
                self.open_calendar();
                vec![]
            }
            CommandKind::ToggleBookmark => self.toggle_bookmark_current(),
            CommandKind::AddToList => self.add_current_to_list(),
            CommandKind::Refresh => match self.screen {
                Screen::Home | Screen::Splash => self.load_feed(self.feed_date, true),
                Screen::Article => self.refresh_article(),
                _ => self.set_screen(self.screen),
            },
            CommandKind::OpenBrowser => {
                if let Some(tab) = self.active_tab() {
                    let url = format!(
                        "https://en.wikipedia.org/wiki/{}",
                        percent_encode_title(&tab.title)
                    );
                    vec![Cmd::OpenUrl { url }]
                } else {
                    vec![]
                }
            }
            CommandKind::NextTheme => self.next_theme(),
            CommandKind::PrevTheme => self.prev_theme(),
            CommandKind::PurgeCache => vec![Cmd::PurgeCache],
            CommandKind::Help => {
                self.overlay = Overlay::Help;
                self.help_scroll = 0;
                vec![]
            }
            CommandKind::Activity => {
                self.overlay = Overlay::Activity;
                vec![]
            }
            CommandKind::Toc => {
                if self.active_tab().and_then(|t| t.doc.as_ref()).is_some() {
                    self.overlay = Overlay::Toc;
                    self.toc_index = 0;
                } else {
                    self.toast("table of contents is not ready yet", true);
                }
                vec![]
            }
            CommandKind::Outline => self.open_element_picker(),
            CommandKind::Quit => {
                self.should_quit = true;
                vec![]
            }
        }
    }

    pub fn start_new_list(&mut self, add_title: Option<String>) {
        self.overlay = Overlay::NewList;
        self.new_list = Some(NewListState {
            input: LineEditor::new(),
            add_title,
        });
        self.picker = None;
    }

    pub fn confirm_delete_list(&mut self, id: i64) {
        self.confirm_list = Some(id);
        self.overlay = Overlay::ConfirmDeleteList;
    }

    pub fn open_list(&mut self, id: i64) -> Vec<Cmd> {
        self.active_list = Some(id);
        self.screen = Screen::ListDetail;
        self.list_items = Loadable::Loading {
            stage: LoadStage::Connecting,
            progress: None,
            started_at: Instant::now(),
            previous: None,
            cancel: tokio_util::sync::CancellationToken::new(),
        };
        vec![Cmd::LoadListItems { list_id: id }]
    }

    pub fn active_list_name(&self) -> String {
        self.active_list
            .and_then(|id| {
                self.lists
                    .value()
                    .and_then(|lists| lists.iter().find(|l| l.id == id))
            })
            .map(|l| l.name.clone())
            .unwrap_or_else(|| "Reading list".to_string())
    }

    pub fn add_title_to_active_list(&mut self) -> Vec<Cmd> {
        let Some(list_id) = self.active_list else {
            return vec![];
        };
        let Some(items) = self.list_items.value() else {
            return vec![];
        };
        let Some(title) = items.get(self.list_item_index).cloned() else {
            return vec![];
        };
        vec![Cmd::RemoveFromList { list_id, title }]
    }

    /// Items for the active feed category tab (deduped within the section).
    pub fn feed_items_flat(&self) -> Vec<(usize, usize)> {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        self.home
            .value()
            .and_then(|feed| {
                let si = self.feed_section.min(feed.sections.len().saturating_sub(1));
                feed.sections.get(si).map(|section| {
                    let mut flat = Vec::new();
                    for (ii, item) in section.items.iter().enumerate() {
                        let key = item.title.trim().to_lowercase();
                        if key.is_empty() || seen.insert(key) {
                            flat.push((si, ii));
                        }
                    }
                    flat
                })
            })
            .unwrap_or_default()
    }

    pub fn cycle_feed_section(&mut self, delta: i32) {
        let Some(feed) = self.home.value() else {
            return;
        };
        let len = feed.sections.len() as i32;
        if len <= 0 {
            return;
        }
        let cur = self.feed_section as i32;
        let next = (cur + delta).rem_euclid(len) as usize;
        self.feed_section = next;
        self.feed_item = 0;
        self.feed_scroll = 0;
    }

    pub fn move_feed_selection(&mut self, delta: i32) {
        let flat = self.feed_items_flat();
        if flat.is_empty() {
            return;
        }
        let current = flat
            .iter()
            .position(|&(si, ii)| si == self.feed_section && ii == self.feed_item)
            .unwrap_or(0) as i32;
        let next = (current + delta).clamp(0, flat.len() as i32 - 1) as usize;
        self.feed_section = flat[next].0;
        self.feed_item = flat[next].1;
    }

    pub fn activate_feed_selection(&mut self) -> Vec<Cmd> {
        let Some(feed) = self.home.value() else {
            return vec![];
        };
        let Some(section) = feed.sections.get(self.feed_section) else {
            return vec![];
        };
        let Some(item) = section.items.get(self.feed_item) else {
            return vec![];
        };
        if let Some(title) = item
            .meta
            .as_ref()
            .filter(|m| m.starts_with("page:"))
            .map(|m| m.trim_start_matches("page:").to_string())
        {
            self.open_article(title, false)
        } else if let Some(url) = &item.url {
            vec![Cmd::OpenUrl { url: url.clone() }]
        } else {
            self.open_article(item.title.clone(), false)
        }
    }

    pub fn activate_current(&mut self) -> Vec<Cmd> {
        match self.screen {
            Screen::Home => self.activate_feed_selection(),
            Screen::Bookmarks => {
                let title = self
                    .bookmarks
                    .value()
                    .and_then(|b| b.get(self.bookmark_index))
                    .map(|b| b.title.clone());
                title
                    .map(|t| self.open_article(t, false))
                    .unwrap_or_default()
            }
            Screen::History => {
                let title = self
                    .history
                    .value()
                    .and_then(|h| h.get(self.history_index))
                    .map(|h| h.title.clone());
                title
                    .map(|t| self.open_article(t, false))
                    .unwrap_or_default()
            }
            Screen::Lists => {
                let id = self
                    .lists
                    .value()
                    .and_then(|l| l.get(self.list_index))
                    .map(|l| l.id);
                id.map(|id| self.open_list(id)).unwrap_or_default()
            }
            Screen::ListDetail => {
                let title = self
                    .list_items
                    .value()
                    .and_then(|items| items.get(self.list_item_index))
                    .cloned();
                title
                    .map(|t| self.open_article(t, false))
                    .unwrap_or_default()
            }
            Screen::MediaLab => self.open_media_lab_item(self.media_lab.selected),
            _ => vec![],
        }
    }

    pub fn handle_new_list_key(
        &mut self,
        code: crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Vec<Cmd> {
        use crossterm::event::KeyCode;
        let Some(state) = self.new_list.as_mut() else {
            return vec![];
        };
        match code {
            KeyCode::Esc => {
                self.new_list = None;
                self.overlay = Overlay::None;
                self.pending_list_add = None;
                vec![]
            }
            KeyCode::Enter => {
                let name = state.input.text().trim().to_string();
                if name.is_empty() {
                    self.toast("reading list needs a name", true);
                    return vec![];
                }
                self.pending_list_add = state.add_title.clone();
                self.new_list = None;
                self.overlay = Overlay::None;
                vec![Cmd::CreateList { name }]
            }
            KeyCode::Backspace => {
                state.input.backspace();
                vec![]
            }
            KeyCode::Delete => {
                state.input.delete();
                vec![]
            }
            KeyCode::Left => {
                state.input.left();
                vec![]
            }
            KeyCode::Right => {
                state.input.right();
                vec![]
            }
            KeyCode::Home => {
                state.input.home();
                vec![]
            }
            KeyCode::End => {
                state.input.end();
                vec![]
            }
            KeyCode::Char(c) if !modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                state.input.insert(c);
                vec![]
            }
            _ => vec![],
        }
    }

    pub fn handle_overlay_key(
        &mut self,
        code: crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Option<Vec<Cmd>> {
        use crossterm::event::KeyCode;
        match self.overlay {
            Overlay::NewList => Some(self.handle_new_list_key(code, modifiers)),
            Overlay::ConfirmDeleteList => Some(match code {
                KeyCode::Enter => {
                    if let Some(id) = self.confirm_list.take() {
                        vec![Cmd::DeleteList { list_id: id }]
                    } else {
                        vec![]
                    }
                }
                KeyCode::Esc | KeyCode::Char('n') => {
                    self.confirm_list = None;
                    self.overlay = Overlay::None;
                    vec![]
                }
                KeyCode::Char('y') => {
                    if let Some(id) = self.confirm_list.take() {
                        vec![Cmd::DeleteList { list_id: id }]
                    } else {
                        vec![]
                    }
                }
                _ => vec![],
            }),
            Overlay::Peek => Some(match code {
                KeyCode::Esc | KeyCode::Char('k') | KeyCode::Char('q') => {
                    self.overlay = Overlay::None;
                    self.peek = PeekState::default();
                    vec![]
                }
                _ => vec![],
            }),
            Overlay::Calendar => Some(self.handle_calendar_key(code)),
            Overlay::Help | Overlay::Activity | Overlay::Toc | Overlay::MediaViewer => {
                Some(match code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                        self.overlay = Overlay::None;
                        self.media_viewer = None;
                        vec![]
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        if self.overlay == Overlay::Help {
                            self.help_scroll = self.help_scroll.saturating_add(1);
                        } else if self.overlay == Overlay::Toc
                            && let Some(doc) = self.active_tab().and_then(|t| t.doc.as_ref())
                        {
                            self.toc_index =
                                (self.toc_index + 1).min(doc.toc.len().saturating_sub(1));
                        }
                        vec![]
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        if self.overlay == Overlay::Help {
                            self.help_scroll = self.help_scroll.saturating_sub(1);
                        } else if self.overlay == Overlay::Toc {
                            self.toc_index = self.toc_index.saturating_sub(1);
                        }
                        vec![]
                    }
                    KeyCode::Enter if self.overlay == Overlay::Toc => {
                        if let Some(doc) = self.active_tab().and_then(|t| t.doc.as_ref())
                            && let Some(entry) = doc.toc.get(self.toc_index)
                        {
                            let line = entry.line;
                            self.scroll_to_article_line(line);
                            self.overlay = Overlay::None;
                        }
                        vec![]
                    }
                    KeyCode::Char(' ') if self.overlay == Overlay::MediaViewer => {
                        match self.media_viewer.as_ref().map(|spec| spec.kind) {
                            Some(MediaKind::Audio) => {
                                self.toggle_audio_playback();
                                vec![]
                            }
                            _ => vec![Cmd::VideoControl(VideoControl::PlayPause)],
                        }
                    }
                    _ => vec![],
                })
            }
            _ => None,
        }
    }

    fn handle_calendar_key(&mut self, code: crossterm::event::KeyCode) -> Vec<Cmd> {
        use crossterm::event::KeyCode;
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.overlay = Overlay::None;
                vec![]
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.calendar_move(-1);
                vec![]
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.calendar_move(1);
                vec![]
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.calendar_move(-7);
                vec![]
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.calendar_move(7);
                vec![]
            }
            KeyCode::PageUp | KeyCode::Char('[') => {
                self.calendar_move_months(-1);
                vec![]
            }
            KeyCode::PageDown | KeyCode::Char(']') => {
                self.calendar_move_months(1);
                vec![]
            }
            KeyCode::Home => {
                let cursor = self.calendar.cursor;
                if let Some(first) = cursor.with_day(1) {
                    self.calendar.cursor = first;
                }
                vec![]
            }
            KeyCode::End => {
                let cursor = self.calendar.cursor;
                let last = days_in_month(cursor.year(), cursor.month());
                if let Some(day) = cursor.with_day(last) {
                    self.calendar.cursor = day;
                }
                vec![]
            }
            KeyCode::Char('t') => {
                self.calendar.cursor = self.calendar.today;
                vec![]
            }
            KeyCode::Enter => {
                let date = self.calendar.cursor;
                self.jump_to_date(date)
            }
            _ => vec![],
        }
    }

    pub fn handle_key(
        &mut self,
        code: crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Vec<Cmd> {
        use crossterm::event::KeyCode;
        if self.screen == Screen::Splash {
            self.screen = Screen::Home;
            if self.home.value().is_none() {
                return self.load_feed(self.feed_date, false);
            }
            return vec![];
        }
        if let Some(cmds) = self.handle_overlay_key(code, modifiers) {
            return cmds;
        }
        if self.picker.is_some() {
            return self.handle_picker_key(code, modifiers);
        }
        if self.finder.is_some() {
            return self.handle_finder_key(code, modifiers);
        }
        if modifiers.contains(crossterm::event::KeyModifiers::CONTROL) {
            match code {
                KeyCode::Char('c') => {
                    self.should_quit = true;
                    return vec![];
                }
                KeyCode::Char('p') => return self.open_search_picker(),
                KeyCode::Char('f') => {
                    self.open_finder();
                    return vec![];
                }
                KeyCode::Char('o') => return self.open_offline_picker(),
                KeyCode::Char('l') => return self.open_lists_picker(),
                KeyCode::Char('n') => {
                    self.start_new_list(None);
                    return vec![];
                }
                KeyCode::Char('k') => return self.peek_cursor_link(),
                KeyCode::Char('r') => return vec![Cmd::PurgeCache],
                _ => {}
            }
        }
        if modifiers.contains(crossterm::event::KeyModifiers::ALT) {
            match code {
                KeyCode::Char(' ') => return self.open_nav_picker(),
                KeyCode::Char('`') => return self.open_tabs_picker(),
                KeyCode::Char('e') => {
                    if self.screen == Screen::Article {
                        return self.open_element_picker();
                    }
                }
                KeyCode::Char('d') => {
                    self.open_calendar();
                    return vec![];
                }
                KeyCode::Char('h') => return self.open_history_picker(),
                KeyCode::Char('l') => return self.open_lists_picker(),
                KeyCode::Char('o') => return self.open_saved_picker(),
                KeyCode::Char('t') => return self.open_theme_picker(),
                KeyCode::Char('a') => {
                    self.overlay = Overlay::Activity;
                    return vec![];
                }
                KeyCode::Char('j') => {
                    if self.screen == Screen::Article {
                        return self.jump_link(true);
                    }
                }
                KeyCode::Char('k') => {
                    if self.screen == Screen::Article {
                        return self.jump_link(false);
                    }
                }
                KeyCode::Char('c') => {
                    if self.screen == Screen::Article
                        && self.active_tab().and_then(|t| t.doc.as_ref()).is_some()
                    {
                        self.overlay = Overlay::Toc;
                        self.toc_index = 0;
                    } else {
                        self.toast("table of contents is not ready yet", true);
                    }
                    return vec![];
                }
                _ => {}
            }
        }
        match code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                vec![]
            }
            KeyCode::Esc => {
                if self.overlay != Overlay::None {
                    self.overlay = Overlay::None;
                    self.media_viewer = None;
                    return vec![];
                }
                if self.picker.is_some() {
                    self.close_picker();
                    return vec![];
                }
                match self.screen {
                    Screen::Article => {
                        self.screen = Screen::Home;
                        vec![]
                    }
                    _ => vec![],
                }
            }
            KeyCode::Char('/') => self.open_search_picker(),
            KeyCode::Char(':') | KeyCode::Char(';') => self.open_command_palette(),
            KeyCode::Char('?') => {
                self.overlay = Overlay::Help;
                self.help_scroll = 0;
                vec![]
            }
            KeyCode::Char('t') => self.next_theme(),
            KeyCode::Char('T') => self.prev_theme(),
            KeyCode::Char('p') => self.open_theme_picker(),
            KeyCode::Char('h') => self.set_screen(Screen::Home),
            KeyCode::Char('b') => self.set_screen(Screen::Bookmarks),
            KeyCode::Char('y') => self.open_history_picker(),
            KeyCode::Char('l') => self.set_screen(Screen::Lists),
            KeyCode::Char('m') => self.set_screen(Screen::MediaLab),
            KeyCode::Char('B') => self.toggle_bookmark_current(),
            KeyCode::Char('a') => self.add_current_to_list(),
            KeyCode::Char('A') => {
                self.overlay = Overlay::Activity;
                vec![]
            }
            KeyCode::Char('n') => {
                self.start_new_list(None);
                vec![]
            }
            KeyCode::Char('r') => match self.screen {
                Screen::Home => self.load_feed(self.feed_date, true),
                Screen::Article => self.refresh_article(),
                _ => self.set_screen(self.screen),
            },
            KeyCode::Char('R') => {
                self.invalidate_layout();
                self.refresh_article()
            }
            KeyCode::Char('o') if self.screen == Screen::Article => {
                if let Some(tab) = self.active_tab() {
                    let url = format!(
                        "https://en.wikipedia.org/wiki/{}",
                        percent_encode_title(&tab.title)
                    );
                    vec![Cmd::OpenUrl { url }]
                } else {
                    vec![]
                }
            }
            KeyCode::Tab => {
                self.cycle_tab(1);
                vec![]
            }
            KeyCode::BackTab => {
                self.cycle_tab(-1);
                vec![]
            }
            KeyCode::Char(']') => {
                if self.screen == Screen::Home {
                    self.cycle_feed_section(1);
                } else {
                    self.cycle_tab(1);
                }
                vec![]
            }
            KeyCode::Char('[') => {
                if self.screen == Screen::Home {
                    self.cycle_feed_section(-1);
                } else {
                    self.cycle_tab(-1);
                }
                vec![]
            }
            KeyCode::Char('x') if self.screen == Screen::Article => {
                if let Some(id) = self.active_tab {
                    self.close_tab(id);
                }
                vec![]
            }
            KeyCode::Char('1'..='9') => {
                if let KeyCode::Char(c) = code {
                    let idx = c.to_digit(10).unwrap_or(1) as usize - 1;
                    if let Some(tab) = self.tabs.get(idx) {
                        self.switch_tab(tab.id);
                    }
                }
                vec![]
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.nav_down();
                vec![]
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.nav_up();
                vec![]
            }
            KeyCode::PageDown | KeyCode::Char(' ') if self.screen == Screen::Article => {
                self.scroll_article(self.article_view_height as i32 / 2);
                vec![]
            }
            KeyCode::PageUp if self.screen == Screen::Article => {
                self.scroll_article(-(self.article_view_height as i32 / 2));
                vec![]
            }
            KeyCode::Char('g') => {
                if self.screen == Screen::Article {
                    if let Some(tab) = self.active_tab_mut() {
                        tab.scroll = 0;
                    }
                } else {
                    self.nav_home();
                }
                vec![]
            }
            KeyCode::Char('G') => {
                if self.screen == Screen::Article {
                    let total = self.article_total_height();
                    if let Some(tab) = self.active_tab_mut() {
                        tab.scroll = total.saturating_sub(2);
                    }
                } else {
                    self.nav_end();
                }
                vec![]
            }
            KeyCode::Enter => self.activate_current(),
            KeyCode::Char(' ') if self.screen == Screen::MediaLab => {
                self.toggle_media_lab_playback()
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.audio.volume = (self.audio.volume + 0.05).min(1.0);
                vec![]
            }
            KeyCode::Char('-') => {
                self.audio.volume = (self.audio.volume - 0.05).max(0.0);
                vec![]
            }
            KeyCode::Char('D') if self.screen == Screen::Lists => {
                if let Some(id) = self
                    .lists
                    .value()
                    .and_then(|l| l.get(self.list_index))
                    .map(|l| l.id)
                {
                    self.confirm_delete_list(id);
                }
                vec![]
            }
            KeyCode::Char('d') if self.screen == Screen::ListDetail => {
                self.add_title_to_active_list()
            }
            KeyCode::Char('c') if self.screen == Screen::ListDetail => self.open_lists_picker(),
            _ => vec![],
        }
    }

    fn nav_down(&mut self) {
        match self.screen {
            Screen::Home => self.move_feed_selection(1),
            Screen::Article => self.scroll_article(1),
            Screen::Bookmarks => {
                self.bookmark_index = self.bookmark_index.saturating_add(1).min(
                    self.bookmarks
                        .value()
                        .map(|b| b.len().saturating_sub(1))
                        .unwrap_or(0),
                );
            }
            Screen::History => {
                self.history_index = self.history_index.saturating_add(1).min(
                    self.history
                        .value()
                        .map(|h| h.len().saturating_sub(1))
                        .unwrap_or(0),
                );
            }
            Screen::Lists => {
                self.list_index = self.list_index.saturating_add(1).min(
                    self.lists
                        .value()
                        .map(|l| l.len().saturating_sub(1))
                        .unwrap_or(0),
                );
            }
            Screen::ListDetail => {
                self.list_item_index = self.list_item_index.saturating_add(1).min(
                    self.list_items
                        .value()
                        .map(|l| l.len().saturating_sub(1))
                        .unwrap_or(0),
                );
            }

            Screen::MediaLab => {
                let next =
                    (self.media_lab.selected + 1).min(self.media_lab.items.len().saturating_sub(1));
                if next != self.media_lab.selected {
                    self.media_lab.selected = next;
                    let cmds = self.load_media_lab();
                    for cmd in cmds {
                        self.queue(cmd);
                    }
                }
            }
            _ => {}
        }
    }

    fn nav_up(&mut self) {
        match self.screen {
            Screen::Home => self.move_feed_selection(-1),
            Screen::Article => self.scroll_article(-1),
            Screen::Bookmarks => self.bookmark_index = self.bookmark_index.saturating_sub(1),
            Screen::History => self.history_index = self.history_index.saturating_sub(1),
            Screen::Lists => self.list_index = self.list_index.saturating_sub(1),
            Screen::ListDetail => self.list_item_index = self.list_item_index.saturating_sub(1),

            Screen::MediaLab => {
                let previous = self.media_lab.selected.saturating_sub(1);
                if previous != self.media_lab.selected {
                    self.media_lab.selected = previous;
                    let cmds = self.load_media_lab();
                    for cmd in cmds {
                        self.queue(cmd);
                    }
                }
            }
            _ => {}
        }
    }

    fn nav_home(&mut self) {
        match self.screen {
            Screen::Home => self.move_feed_selection(-999),
            Screen::Bookmarks => self.bookmark_index = 0,
            Screen::History => self.history_index = 0,
            Screen::Lists => self.list_index = 0,
            Screen::ListDetail => self.list_item_index = 0,

            Screen::MediaLab => self.media_lab.selected = 0,
            _ => {}
        }
    }

    fn nav_end(&mut self) {
        match self.screen {
            Screen::Home => self.move_feed_selection(999),
            Screen::Bookmarks => {
                self.bookmark_index = self
                    .bookmarks
                    .value()
                    .map(|b| b.len().saturating_sub(1))
                    .unwrap_or(0)
            }
            Screen::History => {
                self.history_index = self
                    .history
                    .value()
                    .map(|h| h.len().saturating_sub(1))
                    .unwrap_or(0)
            }
            Screen::Lists => {
                self.list_index = self
                    .lists
                    .value()
                    .map(|l| l.len().saturating_sub(1))
                    .unwrap_or(0)
            }
            Screen::ListDetail => {
                self.list_item_index = self
                    .list_items
                    .value()
                    .map(|l| l.len().saturating_sub(1))
                    .unwrap_or(0)
            }
            Screen::MediaLab => {
                self.media_lab.selected = self.media_lab.items.len().saturating_sub(1)
            }
            _ => {}
        }
    }

    pub fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) -> Vec<Cmd> {
        use crossterm::event::{MouseButton, MouseEventKind};
        let x = mouse.column;
        let y = mouse.row;
        match mouse.kind {
            MouseEventKind::ScrollDown => {
                if self.picker.is_some() {
                    if let Some(picker) = self.picker.as_mut() {
                        picker.select_next();
                    }
                    self.preview_picker_theme();
                    return self.refresh_picker_preview();
                }
                if self.screen == Screen::Article {
                    self.scroll_article(3);
                } else if self.screen == Screen::Home {
                    self.move_feed_selection(1);
                } else {
                    self.nav_down();
                }
                vec![]
            }
            MouseEventKind::ScrollUp => {
                if self.picker.is_some() {
                    if let Some(picker) = self.picker.as_mut() {
                        picker.select_prev();
                    }
                    self.preview_picker_theme();
                    return self.refresh_picker_preview();
                }
                if self.screen == Screen::Article {
                    self.scroll_article(-3);
                } else if self.screen == Screen::Home {
                    self.move_feed_selection(-1);
                } else {
                    self.nav_up();
                }
                vec![]
            }
            MouseEventKind::Moved => {
                self.update_hover(x, y);
                vec![]
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.screen == Screen::Splash {
                    self.screen = Screen::Home;
                    return vec![];
                }
                let region = self.hit_at(x, y).cloned();
                let now = Instant::now();
                let double = self
                    .last_click
                    .map(|(t, lx, ly)| {
                        now.duration_since(t) < Duration::from_millis(420)
                            && lx.abs_diff(x) <= 1
                            && ly.abs_diff(y) <= 1
                    })
                    .unwrap_or(false);
                self.last_click = Some((now, x, y));
                if let Some(region) = region {
                    return self.activate_hit(region.action, double);
                }
                vec![]
            }
            MouseEventKind::Down(MouseButton::Middle) => {
                if let Some(region) = self.hit_at(x, y).cloned() {
                    match region.action {
                        HitAction::ArticleLink(LinkTarget {
                            kind: LinkKind::Article,
                            target,
                            ..
                        }) => {
                            return self.open_article(target, true);
                        }
                        HitAction::FeedItem { section, item } => {
                            self.feed_section = section;
                            self.feed_item = item;
                            return self.activate_feed_selection();
                        }
                        _ => {}
                    }
                }
                vec![]
            }
            MouseEventKind::Down(MouseButton::Right) => {
                if let Some(region) = self.hit_at(x, y).cloned()
                    && let HitAction::ArticleLink(LinkTarget {
                        kind: LinkKind::Article,
                        target,
                        ..
                    }) = region.action
                {
                    return self.open_article(target, true);
                }
                vec![]
            }
            _ => vec![],
        }
    }

    fn hit_at(&self, x: u16, y: u16) -> Option<&HitRegion> {
        self.hitmap
            .iter()
            .filter(|r| r.contains(x, y))
            .max_by_key(|r| r.z)
    }

    fn update_hover(&mut self, x: u16, y: u16) {
        let region = self.hit_at(x, y).cloned();
        if let Some(region) = region {
            if let HitAction::ArticleLink(target) = &region.action {
                let same = self
                    .hover
                    .as_ref()
                    .map(|h| h.target == *target && h.x == x && h.y == y)
                    .unwrap_or(false);
                if !same {
                    self.hover = Some(Hover {
                        target: target.clone(),
                        x,
                        y,
                    });
                }
            } else {
                self.hover = None;
            }
        } else {
            self.hover = None;
        }
    }

    pub fn activate_hit(&mut self, action: HitAction, double: bool) -> Vec<Cmd> {
        match action {
            HitAction::Sidebar(screen) => self.set_screen(screen),
            HitAction::Tab(id) => {
                self.switch_tab(id);
                vec![]
            }
            HitAction::CloseTab(id) => {
                self.close_tab(id);
                vec![]
            }
            HitAction::FeedSection(section) => {
                self.feed_section = section;
                self.feed_item = 0;
                vec![]
            }
            HitAction::FeedItem { section, item } => {
                self.feed_section = section;
                self.feed_item = item;
                if double {
                    self.open_article(
                        self.home
                            .value()
                            .and_then(|f| f.sections.get(section))
                            .and_then(|s| s.items.get(item))
                            .map(|i| i.title.clone())
                            .unwrap_or_default(),
                        true,
                    )
                } else {
                    self.activate_feed_selection()
                }
            }
            HitAction::FeedPrev => self.feed_prev(),
            HitAction::FeedNext => self.feed_next(),
            HitAction::OpenCalendar => {
                self.open_calendar();
                vec![]
            }
            HitAction::CalendarDay(date) => {
                self.calendar.cursor = date;
                self.jump_to_date(date)
            }
            HitAction::CalendarMonth(delta) => {
                self.calendar_move_months(delta);
                vec![]
            }
            HitAction::ArticleLink(target) => match target.kind {
                LinkKind::Article => self.open_article(target.target, double),
                LinkKind::External | LinkKind::Media => vec![Cmd::OpenUrl { url: target.target }],
                LinkKind::Citation => vec![],
            },
            HitAction::ArticleMedia(key) => {
                let (kind, url) = parse_media_key(&key);
                self.overlay = Overlay::MediaViewer;
                self.media_viewer = Some(MediaSpec::new(url, kind));
                vec![]
            }
            HitAction::Toc(index) => {
                if let Some(doc) = self.active_tab().and_then(|t| t.doc.as_ref())
                    && let Some(entry) = doc.toc.get(index)
                {
                    let line = entry.line;
                    self.scroll_to_article_line(line);
                    self.overlay = Overlay::None;
                }
                vec![]
            }
            HitAction::PickerEntry(index) => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.selected = index;
                    let action = picker.selected_entry().map(|e| e.action.clone());
                    if let Some(action) = action {
                        return self.activate_picker_action(action);
                    }
                }
                vec![]
            }
            HitAction::PickerNewList => {
                let title = self.pending_list_add.clone();
                self.close_picker();
                self.start_new_list(title);
                vec![]
            }
            HitAction::PickerClose => {
                self.close_picker();
                vec![]
            }
            HitAction::PickerElementFilter(filter) => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.element_filter = filter;
                    picker.apply_element_filter();
                }
                vec![]
            }
            HitAction::BookmarkRow(index) => {
                self.bookmark_index = index;
                self.activate_current()
            }
            HitAction::HistoryRow(index) => {
                self.history_index = index;
                self.activate_current()
            }
            HitAction::ListRow(index) => {
                self.list_index = index;
                vec![]
            }
            HitAction::ListOpen(id) => self.open_list(id),
            HitAction::ListDelete(id) => {
                self.confirm_delete_list(id);
                vec![]
            }
            HitAction::ListItemRow(index) => {
                self.list_item_index = index;
                self.activate_current()
            }
            HitAction::MediaLabItem(index) => self.open_media_lab_item(index),
            HitAction::CloseOverlay | HitAction::HelpClose | HitAction::ActivityClose => {
                self.overlay = Overlay::None;
                self.media_viewer = None;
                vec![]
            }
            HitAction::PeekClose => {
                self.overlay = Overlay::None;
                self.peek = PeekState::default();
                vec![]
            }

            HitAction::MediaPlayPause => match self.media_viewer.as_ref().map(|spec| spec.kind) {
                Some(MediaKind::Audio) => {
                    self.toggle_audio_playback();
                    vec![]
                }
                _ => vec![Cmd::VideoControl(VideoControl::PlayPause)],
            },
            HitAction::MediaSeek(pct) => vec![Cmd::VideoControl(VideoControl::Seek(pct))],
            HitAction::NewListSubmit => self.handle_new_list_key(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ),
            HitAction::ConfirmDelete(yes) => {
                if yes {
                    if let Some(id) = self.confirm_list.take() {
                        vec![Cmd::DeleteList { list_id: id }]
                    } else {
                        vec![]
                    }
                } else {
                    self.confirm_list = None;
                    self.overlay = Overlay::None;
                    vec![]
                }
            }
        }
    }
}

fn loadable_from_result<T>(result: Result<T, LoadError>) -> Loadable<T> {
    match result {
        Ok(value) => Loadable::ready(value, Source::Network),
        Err(err) => Loadable::failed(err),
    }
}

fn media_kind_for_demo(kind: DemoKind) -> MediaKind {
    match kind {
        DemoKind::Image => MediaKind::Image,
        DemoKind::Gif => MediaKind::Gif,
        DemoKind::Svg => MediaKind::Svg,
        DemoKind::Audio => MediaKind::Audio,
        DemoKind::Video => MediaKind::Video,
        DemoKind::Code | DemoKind::Math => MediaKind::Image,
    }
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

fn parse_media_key(key: &str) -> (MediaKind, String) {
    if let Some(rest) = key.strip_prefix("math:") {
        return (MediaKind::Math, rest.to_string());
    }
    for (prefix, kind) in [
        ("image:", MediaKind::Image),
        ("animation:", MediaKind::Gif),
        ("vector:", MediaKind::Svg),
        ("audio:", MediaKind::Audio),
        ("video:", MediaKind::Video),
        ("document:", MediaKind::Document),
    ] {
        if let Some(rest) = key.strip_prefix(prefix) {
            return (kind, rest.to_string());
        }
    }
    (MediaKind::Image, key.to_string())
}

pub fn math_key(tex: &str, light: bool) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(tex.as_bytes());
    hasher.update([light as u8]);
    format!("math:{:x}", hasher.finalize())
}

/// Remove duplicate entries from the article-outline buckets. Two entries with
/// the same filter, label and action are the same destination, so only the
/// first (topmost in the document) is kept.
fn dedupe_elements(
    entries: Vec<(ElementFilter, PickerEntry)>,
) -> Vec<(ElementFilter, PickerEntry)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(entries.len());
    for (filter, entry) in entries {
        let key = format!(
            "{:?}|{}|{:?}",
            filter,
            entry.label.to_lowercase(),
            entry.action
        );
        if seen.insert(key) {
            out.push((filter, entry));
        }
    }
    out
}

fn strip_snippet(snippet: &str) -> String {
    let mut out = String::with_capacity(snippet.len());
    let mut in_tag = false;
    for c in snippet.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn percent_encode_title(title: &str) -> String {
    use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
    utf8_percent_encode(&title.replace(' ', "_"), NON_ALPHANUMERIC).to_string()
}

fn relative_time(ts: i64) -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let delta = (now - ts).max(0);
    if delta < 60 {
        "just now".to_string()
    } else if delta < 3600 {
        format!("{}m ago", delta / 60)
    } else if delta < 86_400 {
        format!("{}h ago", delta / 3600)
    } else {
        format!("{}d ago", delta / 86_400)
    }
}

fn bytes_label(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

impl crate::core::model::CachedArticle {
    fn size_bytes_label(&self) -> String {
        bytes_label(self.size_bytes)
    }
}

// Keep these imports alive for modules that use this file as the domain hub.
#[allow(unused_imports)]
use crate::core::{CachedArticle as _, FeedSection as _};

#[cfg(test)]
mod picker_tests {
    use super::*;
    use crate::core::{ReadingList, Source};

    #[test]
    fn add_to_list_picker_uses_add_actions() {
        let mut state = AppState::new(crate::core::Config::default(), false, false);
        state.lists = Loadable::ready(
            vec![ReadingList {
                id: 7,
                name: "Systems".to_string(),
                created_at: 0,
                item_count: 2,
            }],
            Source::Local,
        );
        state.open_add_to_list("Rust".to_string());
        let entries = &state.picker.as_ref().expect("picker").entries;
        assert!(entries.iter().any(|entry| {
            matches!(entry.action, PickerAction::AddToList(7)) && entry.label == "Systems"
        }));
    }

    #[test]
    fn nav_picker_lists_destinations() {
        let mut state = AppState::new(crate::core::Config::default(), false, false);
        state.open_nav_picker();
        let picker = state.picker.as_ref().expect("picker");
        assert_eq!(picker.mode, PickerMode::Navigation);
        assert!(
            picker
                .entries
                .iter()
                .any(|entry| matches!(entry.action, PickerAction::GoTo(Screen::Home)))
        );
        assert!(picker.entries.iter().any(|entry| {
            matches!(entry.action, PickerAction::RunCommand(CommandKind::Themes))
        }));
    }

    #[test]
    fn element_filter_cycles_buckets() {
        let mut picker = Picker::new(PickerMode::ArticleElements);
        picker.element_base = vec![
            (
                ElementFilter::RelatedLinks,
                PickerEntry::new("Rust", "link", PickerAction::ScrollToLine(0)),
            ),
            (
                ElementFilter::Headings,
                PickerEntry::new("History", "heading", PickerAction::ScrollToLine(1)),
            ),
            (
                ElementFilter::Code,
                PickerEntry::new("rust code", "code", PickerAction::ScrollToLine(2)),
            ),
        ];
        picker.apply_element_filter();
        assert_eq!(picker.entries.len(), 1);
        assert_eq!(picker.entries[0].label, "Rust");

        picker.cycle_element_filter(true);
        assert_eq!(picker.element_filter, ElementFilter::Headings);
        assert_eq!(picker.entries[0].label, "History");

        picker.cycle_element_filter(false);
        assert_eq!(picker.element_filter, ElementFilter::RelatedLinks);
        assert_eq!(picker.entries[0].label, "Rust");
    }
}
