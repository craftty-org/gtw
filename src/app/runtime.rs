use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures::StreamExt;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use crate::api::{ApiError, MediaBytes, WikiClient};
use crate::app::terminal::Tui;
use crate::app::ui;
use crate::cache::Cache;
use crate::core::{AppState, Cmd, Config, LoadError, LoadErrorKind, Msg, VideoControl};
use crate::media::audio::{AudioEngine, decode_audio};
use crate::media::image;
use crate::media::video::{VideoClip, decode_video};
use crate::render::EffectManager;
use crate::render::images::ImageRenderer;
use crate::render::math::render_math;

const LOOP_TICK: Duration = Duration::from_millis(40);

pub struct RunOptions {
    pub config: Config,
    pub offline: bool,
    pub demo_media: bool,
}

pub async fn run(options: RunOptions) -> Result<()> {
    let cache = Arc::new(Cache::open().context("opening the offline cache")?);
    let client = WikiClient::new(&options.config).context("creating the Wikipedia client")?;
    let mut tui = Tui::init().context("starting the terminal")?;
    let size = tui.size().unwrap_or(ratatui::layout::Size {
        width: 100,
        height: 30,
    });
    let mut state = AppState::new(options.config.clone(), options.offline, options.demo_media);
    let _ = state.handle_msg(Msg::Resize(size.width, size.height));
    let mut images = ImageRenderer::new();
    let mut effects = EffectManager::new();
    effects.set_enabled(options.config.effects.enabled);
    effects.add_startup();
    let (tx, mut rx) = mpsc::channel::<Msg>(256);
    let mut executor = Executor::new(tx.clone(), cache, client, options.config);
    let mut events = EventStream::new();
    let mut interval = tokio::time::interval(LOOP_TICK);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_frame = Instant::now();
    let mut last_screen = state.screen;
    let mut last_feed = state.feed_date;
    let mut redraw = true;

    executor.dispatch(state.bootstrap());
    executor.dispatch(state.take_pending());

    loop {
        tokio::select! {
            maybe_event = events.next() => {
                match maybe_event {
                    Some(Ok(event)) => {
                        let cmds = match event {
                            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                                state.handle_key(key.code, key.modifiers)
                            }
                            Event::Mouse(mouse) => state.handle_mouse(mouse),
                            Event::Resize(w, h) => {
                                images.clear();
                                state.handle_msg(Msg::Resize(w, h))
                            }
                            Event::Paste(text) => state.handle_paste(&text),
                            _ => Vec::new(),
                        };
                        executor.dispatch(cmds);
                        redraw = true;
                    }
                    Some(Err(error)) => {
                        tracing::warn!("terminal event stream: {error}");
                        state.toast("terminal input was interrupted", true);
                    }
                    None => {
                        state.should_quit = true;
                    }
                }
            }
            _ = interval.tick() => {
                let cmds = state.handle_msg(Msg::Tick);
                executor.dispatch(cmds);
                redraw = true;
            }
            maybe_msg = rx.recv() => {
                if let Some(msg) = maybe_msg {
                    let cmds = state.handle_msg(msg);
                    executor.dispatch(cmds);
                    redraw = true;
                }
            }
            _ = tokio::signal::ctrl_c() => {
                state.should_quit = true;
            }
        }

        while let Ok(msg) = rx.try_recv() {
            let cmds = state.handle_msg(msg);
            executor.dispatch(cmds);
            redraw = true;
        }
        executor.dispatch(state.take_pending());
        executor.config.general.offline = state.offline;
        executor.config.media.enabled = state.config.media.enabled;

        if executor.poll_media(&mut state) {
            redraw = true;
        }
        state.ensure_layout();

        if state.screen != last_screen {
            effects.page_in();
            last_screen = state.screen;
        }
        if state.feed_date != last_feed {
            effects.feed_in();
            last_feed = state.feed_date;
        }

        if redraw {
            let dt = last_frame.elapsed();
            last_frame = Instant::now();
            tui.draw(|frame| {
                let area = frame.area();
                ui::draw(frame, &mut state, &mut images);
                effects.process(dt, frame.buffer_mut(), area);
            })?;
            redraw = false;
        }

        if state.should_quit {
            break;
        }
    }

    executor.shutdown();
    tui.restore();
    Ok(())
}

struct VideoPlayback {
    position: f64,
    last_tick: Instant,
    index: Option<u32>,
    playing: bool,
}

struct Executor {
    tx: mpsc::Sender<Msg>,
    cache: Arc<Cache>,
    client: WikiClient,
    config: Config,
    clips: Arc<Mutex<HashMap<String, Arc<VideoClip>>>>,
    audio: Option<AudioEngine>,
    audio_failed: bool,
    active_video: Option<String>,
    video: Option<VideoPlayback>,
    controls: Vec<VideoControl>,
}

impl Executor {
    fn new(tx: mpsc::Sender<Msg>, cache: Arc<Cache>, client: WikiClient, config: Config) -> Self {
        Self {
            tx,
            cache,
            client,
            config,
            clips: Arc::new(Mutex::new(HashMap::new())),
            audio: None,
            audio_failed: false,
            active_video: None,
            video: None,
            controls: Vec::new(),
        }
    }

    fn dispatch(&mut self, cmds: Vec<Cmd>) {
        for cmd in cmds {
            self.spawn(cmd);
        }
    }

    fn send_later(&self, msg: Msg) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(msg).await;
        });
    }

    fn spawn(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::LoadFeed { req, date, refresh } => self.load_feed(req, date, refresh),
            Cmd::LoadArticle {
                req,
                tab,
                title,
                refresh,
            } => self.load_article(req, tab, title, refresh),
            Cmd::Search { req, query } => self.search(req, query),
            Cmd::SearchLocal { query } => {
                let cache = self.cache.clone();
                let tx = self.tx.clone();
                let local_query = query.clone();
                tokio::spawn(async move {
                    let result =
                        tokio::task::spawn_blocking(move || cache.local_search(&local_query, 20))
                            .await
                            .map_err(|error| {
                                LoadError::new(LoadErrorKind::Unknown, error.to_string())
                            })
                            .and_then(|result| result.map_err(Into::into));
                    let _ = tx.send(Msg::LocalSearchResults { query, result }).await;
                });
            }
            Cmd::LoadBookmarks => {
                self.blocking_db(|cache| {
                    Msg::BookmarksLoaded(cache.bookmarks().map_err(Into::into))
                });
            }
            Cmd::LoadHistory => {
                self.blocking_db(|cache| Msg::HistoryLoaded(cache.history().map_err(Into::into)));
            }
            Cmd::LoadLists => {
                self.blocking_db(|cache| {
                    Msg::ListsLoaded(cache.reading_lists().map_err(Into::into))
                });
            }
            Cmd::LoadLibrary => {
                self.blocking_db(|cache| Msg::LibraryLoaded(cache.library().map_err(Into::into)));
            }
            Cmd::LoadListItems { list_id } => {
                self.blocking_db(move |cache| Msg::ListItemsLoaded {
                    list_id,
                    result: cache.list_items(list_id).map_err(Into::into),
                });
            }
            Cmd::LoadListPreview { list_id } => {
                self.blocking_db(move |cache| Msg::ListPreviewLoaded {
                    list_id,
                    result: cache.list_items_with_thumbnails(list_id).map_err(Into::into),
                });
            }
            Cmd::CreateList { name } => {
                self.blocking_db(move |cache| {
                    Msg::ListCreated(cache.create_reading_list(&name).map_err(Into::into))
                });
            }
            Cmd::DeleteList { list_id } => {
                self.blocking_db(move |cache| Msg::ListDeleted {
                    list_id,
                    result: cache.delete_reading_list(list_id).map_err(Into::into),
                });
            }
            Cmd::AddToList { list_id, title } => {
                self.blocking_db(move |cache| Msg::ArticleAddedToList {
                    list_id,
                    title: title.clone(),
                    result: cache.add_to_list(list_id, &title).map_err(Into::into),
                });
            }
            Cmd::RemoveFromList { list_id, title } => {
                self.blocking_db(move |cache| Msg::ArticleRemovedFromList {
                    list_id,
                    title: title.clone(),
                    result: cache.remove_from_list(list_id, &title).map_err(Into::into),
                });
            }
            Cmd::ToggleBookmark { title } => {
                let cache = self.cache.clone();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let toggle_title = title.clone();
                    let cache2 = cache.clone();
                    let result =
                        tokio::task::spawn_blocking(move || cache2.toggle_bookmark(&toggle_title))
                            .await;
                    let message = match result {
                        Ok(Ok(bookmarked)) => Msg::BookmarkToggled {
                            title,
                            bookmarked,
                            result: Ok(()),
                        },
                        Ok(Err(error)) => Msg::BookmarkToggled {
                            title,
                            bookmarked: false,
                            result: Err(error.into()),
                        },
                        Err(error) => Msg::BookmarkToggled {
                            title,
                            bookmarked: false,
                            result: Err(LoadError::new(LoadErrorKind::Unknown, error.to_string())),
                        },
                    };
                    let _ = tx.send(message).await;
                });
            }
            Cmd::LoadPeek { req, title } => {
                let tx = self.tx.clone();
                let cache = self.cache.clone();
                let client = self.client.clone();
                let offline = self.config.general.offline;
                tokio::spawn(async move {
                    // Prefer the cached article summary when offline or already
                    // present; otherwise hit the API for a fresh summary.
                    let peeker = req;
                    if let Ok(Some(article)) = tokio::task::spawn_blocking({
                        let cache = cache.clone();
                        let title = title.clone();
                        move || cache.get_article(&title)
                    })
                    .await
                    .unwrap_or(Ok(None))
                    {
                        let image = article
                            .media
                            .iter()
                            .find(|spec| spec.kind.is_visual())
                            .map(|spec| spec.url.clone());
                        let _ = tx
                            .send(Msg::PeekLoaded {
                                req: peeker,
                                title: article.display_title,
                                summary: article.summary,
                                image,
                                error: None,
                            })
                            .await;
                        return;
                    }
                    if offline {
                        let _ = tx
                            .send(Msg::PeekLoaded {
                                req: peeker,
                                title,
                                summary: String::new(),
                                image: None,
                                error: Some("offline · no cached summary".to_string()),
                            })
                            .await;
                        return;
                    }
                    match client.article(&title).await {
                        Ok(article) => {
                            let image = article
                                .media
                                .iter()
                                .find(|spec| spec.kind.is_visual())
                                .map(|spec| spec.url.clone());
                            let _ = tx
                                .send(Msg::PeekLoaded {
                                    req: peeker,
                                    title: article.display_title,
                                    summary: article.summary,
                                    image,
                                    error: None,
                                })
                                .await;
                        }
                        Err(error) => {
                            let _ = tx
                                .send(Msg::PeekLoaded {
                                    req: peeker,
                                    title,
                                    summary: String::new(),
                                    image: None,
                                    error: Some(api_error(error).message),
                                })
                                .await;
                        }
                    }
                });
            }
            Cmd::LoadBookmarkState { title } => {
                let cache = self.cache.clone();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let state_title = title.clone();
                    let bookmarked = tokio::task::spawn_blocking(move || {
                        cache.is_bookmarked(&state_title).unwrap_or(false)
                    })
                    .await
                    .unwrap_or(false);
                    let _ = tx
                        .send(Msg::BookmarkStateLoaded { title, bookmarked })
                        .await;
                });
            }
            Cmd::PurgeCache => {
                let ttl = self.config.ttl_secs();
                let max = self.config.cache.max_size_mb.saturating_mul(1024 * 1024);
                self.blocking_db(move |cache| {
                    Msg::CachePurged(cache.purge(ttl, max).map_err(Into::into))
                });
            }
            Cmd::LoadMedia { key, url, kind } => {
                let tx = self.tx.clone();
                let client = self.client.clone();
                let cache_dir = Config::media_dir();
                let max = self.config.media.max_megabytes.saturating_mul(1024 * 1024);
                let offline = self.config.general.offline;
                tokio::spawn(async move {
                    let result = async {
                        if offline {
                            return Err(LoadError::offline());
                        }
                        let bytes = fetch_media(&client, &cache_dir, &url, max).await?;
                        tokio::task::spawn_blocking(move || image::decode(&bytes, kind))
                            .await
                            .map_err(|error| {
                                LoadError::new(LoadErrorKind::Unknown, error.to_string())
                            })?
                    }
                    .await;
                    let _ = tx.send(Msg::MediaLoaded { key, result }).await;
                });
            }
            Cmd::RenderMath { key, tex, light } => {
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let result = tokio::task::spawn_blocking(move || {
                        render_math(&tex, light, 120)
                            .map(|image| {
                                crate::media::DecodedMedia::single(
                                    image,
                                    crate::core::MediaKind::Math,
                                )
                            })
                            .map_err(LoadError::parse)
                    })
                    .await
                    .map_err(|error| LoadError::new(LoadErrorKind::Unknown, error.to_string()))
                    .and_then(|result| result);
                    let _ = tx.send(Msg::MathRendered { key, result }).await;
                });
            }
            Cmd::LoadAudio { key, url, title } => {
                let tx = self.tx.clone();
                let client = self.client.clone();
                let cache_dir = Config::media_dir();
                let max = self.config.media.max_megabytes.saturating_mul(1024 * 1024);
                let offline = self.config.general.offline;
                tokio::spawn(async move {
                    let result = async {
                        if offline {
                            return Err(LoadError::offline());
                        }
                        let bytes = fetch_media(&client, &cache_dir, &url, max).await?;
                        let title2 = title.clone();
                        tokio::task::spawn_blocking(move || decode_audio(&bytes, title2))
                            .await
                            .map_err(|error| {
                                LoadError::new(LoadErrorKind::Unknown, error.to_string())
                            })?
                    }
                    .await;
                    let _ = tx.send(Msg::AudioLoaded { key, result }).await;
                });
            }
            Cmd::LoadVideo { key, url, title } => {
                let tx = self.tx.clone();
                let client = self.client.clone();
                let clips = self.clips.clone();
                let cache_dir = Config::media_dir();
                let max = self.config.media.max_megabytes.saturating_mul(1024 * 1024);
                let offline = self.config.general.offline;
                tokio::spawn(async move {
                    let result = async {
                        if offline {
                            return Err(LoadError::offline());
                        }
                        let bytes = fetch_media(&client, &cache_dir, &url, max).await?;
                        tokio::task::spawn_blocking(move || decode_video(&bytes))
                            .await
                            .map_err(|error| {
                                LoadError::new(LoadErrorKind::Unknown, error.to_string())
                            })?
                    }
                    .await;
                    match result {
                        Ok(clip) => {
                            let clip = Arc::new(clip);
                            if let Ok(mut map) = clips.lock() {
                                map.insert(key.clone(), clip.clone());
                            }
                            let _ = tx
                                .send(Msg::VideoLoaded {
                                    key: key.clone(),
                                    total: clip.total(),
                                    duration: clip.duration,
                                })
                                .await;
                            let _ = title;
                        }
                        Err(error) => {
                            let _ = tx.send(Msg::VideoFailed { key, error }).await;
                        }
                    }
                });
            }
            Cmd::VideoControl(control) => self.controls.push(control),
            Cmd::OpenUrl { url } => {
                self.send_later(Msg::Toast {
                    text: format!("opening {url}"),
                    error: false,
                });
                let tx = self.tx.clone();
                tokio::task::spawn_blocking(move || {
                    let result = open_url(&url);
                    let _ = tx.blocking_send(Msg::OpenUrlResult { url, result });
                });
            }
        }
    }

    fn blocking_db<F>(&self, f: F)
    where
        F: FnOnce(&Cache) -> Msg + Send + 'static,
    {
        let cache = self.cache.clone();
        let tx = self.tx.clone();
        tokio::task::spawn_blocking(move || {
            let msg = f(cache.as_ref());
            let _ = tx.blocking_send(msg);
        });
    }

    fn load_feed(&self, req: crate::core::RequestId, date: chrono::NaiveDate, refresh: bool) {
        let tx = self.tx.clone();
        let cache = self.cache.clone();
        let client = self.client.clone();
        let offline = self.config.general.offline;
        tokio::spawn(async move {
            if !refresh {
                let cache2 = cache.clone();
                let cached = tokio::task::spawn_blocking(move || cache2.get_feed(date))
                    .await
                    .ok()
                    .and_then(|result| result.ok())
                    .flatten();
                if let Some(feed) = cached {
                    let cache3 = cache.clone();
                    let _ = tokio::task::spawn_blocking(move || cache3.touch_feed(date)).await;
                    let _ = tx
                        .send(Msg::FeedLoaded {
                            req,
                            date,
                            result: Ok(feed),
                        })
                        .await;
                    return;
                }
            }
            if offline {
                let _ = tx
                    .send(Msg::FeedLoaded {
                        req,
                        date,
                        result: Err(LoadError::offline()),
                    })
                    .await;
                return;
            }
            let result = client.feed(date).await.map_err(api_error);
            if let Ok(feed) = &result {
                let cache2 = cache.clone();
                let feed = feed.clone();
                let _ = tokio::task::spawn_blocking(move || cache2.put_feed(&feed)).await;
            }
            let _ = tx.send(Msg::FeedLoaded { req, date, result }).await;
        });
    }

    fn load_article(
        &self,
        req: crate::core::RequestId,
        tab: crate::core::TabId,
        title: String,
        refresh: bool,
    ) {
        let tx = self.tx.clone();
        let cache = self.cache.clone();
        let client = self.client.clone();
        let offline = self.config.general.offline;
        tokio::spawn(async move {
            if !refresh {
                let cache2 = cache.clone();
                let title2 = title.clone();
                let cached = tokio::task::spawn_blocking(move || cache2.get_article(&title2))
                    .await
                    .ok()
                    .and_then(|result| result.ok())
                    .flatten();
                if let Some(article) = cached {
                    let cache3 = cache.clone();
                    let title3 = title.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let _ = cache3.touch_article(&title3);
                        let _ = cache3.record_history(&title3);
                    })
                    .await;
                    let _ = tx
                        .send(Msg::ArticleLoaded {
                            req,
                            tab,
                            result: Ok(article),
                        })
                        .await;
                    return;
                }
            }
            if offline {
                let _ = tx
                    .send(Msg::ArticleLoaded {
                        req,
                        tab,
                        result: Err(LoadError::offline()),
                    })
                    .await;
                return;
            }
            let result = client.article(&title).await.map_err(api_error);
            if let Ok(article) = &result {
                let cache2 = cache.clone();
                let article = article.clone();
                let title = article.title.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let _ = cache2.put_article(&article);
                    let _ = cache2.record_history(&title);
                })
                .await;
            }
            let _ = tx.send(Msg::ArticleLoaded { req, tab, result }).await;
        });
    }

    fn search(&self, req: crate::core::RequestId, query: String) {
        let tx = self.tx.clone();
        let client = self.client.clone();
        let offline = self.config.general.offline;
        tokio::spawn(async move {
            let result = if offline {
                Ok(Vec::new())
            } else {
                client.search(&query).await.map_err(api_error)
            };
            let _ = tx.send(Msg::SearchResults { req, query, result }).await;
        });
    }

    fn poll_media(&mut self, state: &mut AppState) -> bool {
        let mut changed = false;
        changed |= self.poll_audio(state);
        changed |= self.poll_video(state);
        changed
    }

    fn poll_audio(&mut self, state: &mut AppState) -> bool {
        let Some(key) = state.audio.key.clone() else {
            return false;
        };
        let Some(data) = state.audio.data.clone() else {
            return false;
        };
        if self.audio.is_none() && !self.audio_failed {
            match AudioEngine::new() {
                Ok(engine) => self.audio = Some(engine),
                Err(error) => {
                    self.audio_failed = true;
                    state.audio.status = crate::media::AudioStatus::Failed;
                    state.audio.error = Some(error.clone());
                    state.toast(error, true);
                }
            }
        }
        let Some(engine) = self.audio.as_mut() else {
            return false;
        };
        let mut changed = false;
        if state.audio.take_play_request() {
            match engine.play(&key, &data) {
                Ok(()) => {
                    state.audio.status = crate::media::AudioStatus::Playing;
                    changed = true;
                }
                Err(error) => {
                    state.audio.error = Some(error.clone());
                    state.toast(format!("audio · {error}"), true);
                }
            }
        }
        if state.audio.take_pause_request() {
            engine.pause();
            state.audio.status = crate::media::AudioStatus::Paused;
            changed = true;
        }
        if state.audio.take_stop_request() {
            engine.stop();
            state.audio.status = crate::media::AudioStatus::Idle;
            state.audio.position = 0.0;
            changed = true;
        }
        if let Some(position) = state.audio.take_seek() {
            engine.seek(position);
            changed = true;
        }
        if (state.audio.volume - engine.volume()).abs() > 0.001 {
            engine.set_volume(state.audio.volume);
            changed = true;
        }
        let (position, playing) = engine.status();
        if (state.audio.position - position).abs() > 0.02 {
            state.audio.position = position;
            changed = true;
        }
        let status = if playing {
            crate::media::AudioStatus::Playing
        } else if state.audio.status == crate::media::AudioStatus::Playing {
            crate::media::AudioStatus::Paused
        } else {
            state.audio.status
        };
        if state.audio.status != status {
            state.audio.status = status;
            changed = true;
        }
        changed
    }

    fn poll_video(&mut self, state: &mut AppState) -> bool {
        let Some(key) = state.video.key.clone() else {
            return false;
        };
        let clip = self
            .clips
            .lock()
            .ok()
            .and_then(|map| map.get(&key).cloned());
        let Some(clip) = clip else {
            return !self.controls.is_empty();
        };
        let mut changed = false;
        if self.active_video.as_deref() != Some(key.as_str()) {
            self.active_video = Some(key.clone());
            self.video = Some(VideoPlayback {
                position: 0.0,
                last_tick: Instant::now(),
                index: None,
                playing: false,
            });
            state.video.key = Some(key.clone());
            state.video.total = clip.total();
            state.video.duration = clip.duration;
            state.video.playing = false;
            state.video.frame = None;
            state.video.status = crate::media::VideoStatus::Ready;
            changed = true;
        }
        if let Some(playback) = self.video.as_mut() {
            for control in self.controls.drain(..) {
                match control {
                    VideoControl::PlayPause => {
                        playback.playing = !playback.playing;
                        state.video.playing = playback.playing;
                        playback.last_tick = Instant::now();
                        if playback.playing {
                            state.video.status = crate::media::VideoStatus::Playing;
                        } else {
                            state.video.status = crate::media::VideoStatus::Paused;
                        }
                        changed = true;
                    }
                    VideoControl::Seek(fraction) => {
                        playback.position = fraction.clamp(0.0, 1.0) * clip.duration.max(0.0);
                        playback.index = None;
                        playback.last_tick = Instant::now();
                        changed = true;
                    }
                    VideoControl::Stop => {
                        playback.playing = false;
                        playback.position = 0.0;
                        playback.index = None;
                        state.video.playing = false;
                        state.video.status = crate::media::VideoStatus::Ready;
                        changed = true;
                    }
                    VideoControl::Volume(volume) => {
                        state.video.volume = volume.clamp(0.0, 1.0);
                        changed = true;
                    }
                }
            }
            if playback.playing {
                let now = Instant::now();
                let dt = now.duration_since(playback.last_tick).as_secs_f64();
                playback.last_tick = now;
                playback.position += dt;
                if playback.position >= clip.duration {
                    playback.position = 0.0;
                    playback.index = None;
                }
            }
            if playback.playing {
                let index = clip.index_at(playback.position);
                if playback.index != Some(index)
                    && let Some(frame) = clip.frame(index)
                {
                    state.video.set_frame(
                        key.clone(),
                        index,
                        clip.total(),
                        frame,
                        playback.position,
                    );
                    state.video.playing = true;
                    state.video.status = crate::media::VideoStatus::Playing;
                    playback.index = Some(index);
                    changed = true;
                }
            } else if playback.index.is_none()
                && let Some(frame) = clip.frame(0)
            {
                state
                    .video
                    .set_frame(key.clone(), 0, clip.total(), frame, playback.position);
                state.video.playing = false;
                playback.index = Some(0);
                changed = true;
            }
            if (state.video.position - playback.position).abs() > 0.03 {
                state.video.position = playback.position;
                changed = true;
            }
        }
        changed
    }

    fn shutdown(&mut self) {
        if let Some(audio) = self.audio.as_mut() {
            audio.stop();
        }
        if let Ok(mut map) = self.clips.lock() {
            map.clear();
        }
    }
}

/// Videos are decoded in blocking tasks, so the cache directory and client
/// travel with the command rather than borrowing the executor.
async fn fetch_media(
    client: &WikiClient,
    cache_dir: &Path,
    url: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, LoadError> {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    let extension = url
        .split('?')
        .next()
        .and_then(|path| path.rsplit('.').next())
        .filter(|ext| ext.len() <= 8 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("bin");
    let path = cache_dir.join(format!("{digest}.{extension}"));
    if let Ok(bytes) = tokio::fs::read(&path).await
        && !bytes.is_empty()
    {
        return Ok(bytes);
    }
    let MediaBytes { data, .. } = client.download(url, max_bytes).await.map_err(api_error)?;
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Err(error) = tokio::fs::write(&path, &data).await {
        tracing::debug!("media cache write failed: {error}");
    }
    Ok(data)
}

fn api_error(error: ApiError) -> LoadError {
    match error {
        ApiError::NotFound => LoadError::new(LoadErrorKind::NotFound, "page not found"),
        ApiError::RateLimited => LoadError::new(
            LoadErrorKind::RateLimited,
            "Wikipedia rate limit reached; retry shortly",
        ),
        ApiError::Network(message) => LoadError::network(message),
        ApiError::Http(status) => LoadError::network(format!("Wikipedia returned HTTP {status}")),
        ApiError::Cancelled => LoadError::cancelled(),
    }
}

fn open_url(url: &str) -> Result<(), LoadError> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(LoadError::new(
            LoadErrorKind::Unknown,
            "refusing to open a non-web URL",
        ));
    }
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(url);
        command
    } else if cfg!(target_os = "windows") {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    command.spawn().map(|_| ()).map_err(|error| {
        LoadError::new(
            LoadErrorKind::Unknown,
            format!("cannot open browser: {error}"),
        )
    })
}
