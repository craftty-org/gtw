use std::time::Duration;

use chrono::{Datelike, NaiveDate};
use futures::StreamExt;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use serde_json::Value;

use crate::api::error::{ApiError, Result};
use crate::core::{
    Article, Config, Feed, FeedItem, FeedKind, FeedSection, MediaSpec, SearchHit, Source,
};
use crate::render::html::extract_article;

const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct MediaBytes {
    pub data: Vec<u8>,
}

#[derive(Clone)]
pub struct WikiClient {
    client: reqwest::Client,
    language: String,
    retries: u32,
}

impl WikiClient {
    pub fn new(config: &Config) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let ua = config.sanitized_user_agent();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&ua)
                .map_err(|_| ApiError::Network("invalid user agent".into()))?,
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/html;q=0.9, */*;q=0.5"),
        );
        let timeout = Duration::from_secs(config.network.timeout_secs.clamp(5, 120));
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(8))
            .build()
            .map_err(|e| ApiError::Network(e.to_string()))?;
        Ok(Self {
            client,
            language: config.general.language.clone(),
            retries: config.network.retries.min(4),
        })
    }

    fn api_base(&self) -> String {
        format!("https://{}.wikipedia.org/w/api.php", self.language)
    }

    fn feed_base(&self) -> String {
        format!(
            "https://api.wikimedia.org/feed/v1/wikipedia/{}/featured",
            self.language
        )
    }

    fn onthisday_base(&self) -> String {
        format!(
            "https://api.wikimedia.org/feed/v1/wikipedia/{}/onthisday/selected",
            self.language
        )
    }

    async fn get(&self, url: &str) -> Result<reqwest::Response> {
        let mut attempt = 0;
        loop {
            let result = self.client.get(url).send().await;
            match result {
                Ok(response) => {
                    let status = response.status();
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        return Err(ApiError::RateLimited);
                    }
                    if status == reqwest::StatusCode::NOT_FOUND {
                        return Err(ApiError::NotFound);
                    }
                    if status.is_server_error() && attempt < self.retries {
                        attempt += 1;
                        tokio::time::sleep(Duration::from_millis(200 * u64::from(attempt))).await;
                        continue;
                    }
                    if !status.is_success() {
                        return Err(ApiError::Http(status.as_u16()));
                    }
                    return Ok(response);
                }
                Err(error) => {
                    if (error.is_timeout() || error.is_connect()) && attempt < self.retries {
                        attempt += 1;
                        tokio::time::sleep(Duration::from_millis(200 * u64::from(attempt))).await;
                        continue;
                    }
                    return Err(ApiError::Network(error.to_string()));
                }
            }
        }
    }

    async fn get_json(&self, url: &str) -> Result<Value> {
        let response = self.get(url).await?;
        if let Some(length) = response.content_length()
            && length as usize > MAX_JSON_BYTES
        {
            return Err(ApiError::Network("response is too large".into()));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        if bytes.len() > MAX_JSON_BYTES {
            return Err(ApiError::Network("response is too large".into()));
        }
        serde_json::from_slice(&bytes).map_err(|e| ApiError::Network(format!("invalid JSON: {e}")))
    }

    fn query(&self, pairs: &[(&str, &str)]) -> String {
        let mut url = reqwest::Url::parse(&self.api_base()).expect("valid api url");
        {
            let mut query = url.query_pairs_mut();
            for (key, value) in pairs {
                query.append_pair(key, value);
            }
        }
        url.to_string()
    }

    pub async fn search(&self, query: &str) -> Result<Vec<SearchHit>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let url = self.query(&[
            ("action", "query"),
            ("list", "search"),
            ("srsearch", query),
            ("srlimit", "15"),
            ("srprop", "snippet"),
            ("format", "json"),
            ("formatversion", "2"),
            ("utf8", "1"),
        ]);
        let value = self.get_json(&url).await?;
        let hits = value
            .pointer("/query/search")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let title = item.get("title")?.as_str()?.to_string();
                        Some(SearchHit {
                            title,
                            snippet: item
                                .get("snippet")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            pageid: item.get("pageid").and_then(Value::as_u64),
                            local: false,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(hits)
    }

    pub async fn article(&self, title: &str) -> Result<Article> {
        let title = title.trim();
        if title.is_empty() {
            return Err(ApiError::NotFound);
        }
        let url = self.query(&[
            ("action", "parse"),
            ("page", title),
            ("prop", "text|displaytitle|revid|properties"),
            ("redirects", "1"),
            ("disableeditsection", "1"),
            ("disabletoc", "1"),
            ("format", "json"),
            ("formatversion", "2"),
            ("utf8", "1"),
        ]);
        let value = self.get_json(&url).await?;
        if let Some(error) = value.get("error") {
            let code = error
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or_default();
            return if code.contains("missing") || code.contains("notfound") {
                Err(ApiError::NotFound)
            } else {
                Err(ApiError::Network(
                    error
                        .get("info")
                        .and_then(Value::as_str)
                        .unwrap_or("Wikipedia could not parse this page")
                        .to_string(),
                ))
            };
        }
        let parse = value.get("parse").ok_or(ApiError::NotFound)?;
        let resolved_title = parse
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(title)
            .to_string();
        let display_title = parse
            .get("displaytitle")
            .and_then(Value::as_str)
            .map(strip_html)
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| resolved_title.clone());
        let html = parse
            .get("text")
            .and_then(|text| {
                text.as_str()
                    .or_else(|| text.get("*").and_then(Value::as_str))
            })
            .unwrap_or_default()
            .to_string();
        if html.trim().is_empty() {
            return Err(ApiError::Network("Wikipedia returned an empty page".into()));
        }
        let extracted = extract_article(&html, &resolved_title);
        let raw_summary = first_paragraph(&extracted.blocks)
            .or_else(|| {
                let text = extracted.plain_text.trim();
                (!text.is_empty()).then(|| truncate(text, 560))
            })
            .unwrap_or_default();
        // File pages expose the raw `File:Name.ext` namespace in their title and
        // often lead with it; keep the reader-facing caption instead.
        let is_file_page =
            resolved_title.starts_with("File:") || resolved_title.starts_with("file:");
        let display_title = if is_file_page {
            crate::core::clean_file_title(&display_title)
        } else {
            display_title
        };
        let summary = if is_file_page {
            crate::core::strip_file_prefix(&raw_summary)
        } else {
            raw_summary
        };
        let pageid = parse.get("pageid").and_then(Value::as_u64);
        let revision = parse.get("revid").and_then(Value::as_u64);
        let mut media = extracted.media;
        media.retain(|spec| !spec.url.trim().is_empty());
        Ok(Article {
            title: resolved_title,
            display_title,
            pageid,
            revision,
            language: self.language.clone(),
            html,
            summary,
            blocks: extracted.blocks,
            plain_text: extracted.plain_text,
            media,
            fetched_at: now(),
            source: Source::Network,
        })
    }

    pub async fn feed(&self, date: NaiveDate) -> Result<Feed> {
        let url = format!(
            "{}/{:04}/{:02}/{:02}",
            self.feed_base(),
            date.year(),
            date.month(),
            date.day()
        );
        let value = match self.get_json(&url).await {
            Ok(value) => value,
            Err(ApiError::NotFound) => {
                return self.onthisday_feed(date).await;
            }
            Err(error) => return Err(error),
        };
        let mut sections = Vec::new();

        if let Some(tfa) = value.get("tfa").and_then(Value::as_object) {
            let title = tfa
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if !title.is_empty() {
                sections.push(FeedSection {
                    kind: FeedKind::FeaturedArticle,
                    items: vec![FeedItem {
                        title: title.clone(),
                        subtitle: tfa
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("Featured article")
                            .to_string(),
                        extract: tfa
                            .get("extract")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        thumbnail: thumbnail_url(tfa),
                        url: tfa
                            .get("content_urls")
                            .and_then(|v| v.get("desktop"))
                            .and_then(|v| v.get("page"))
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        meta: Some(format!("page:{title}")),
                    }],
                });
            }
        }

        if let Some(image) = value.get("image").and_then(Value::as_object) {
            let raw_title = image
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Picture of the day")
                .to_string();
            let title = crate::core::clean_file_title(&raw_title);
            let description = image
                .get("description")
                .and_then(|v| v.get("text"))
                .and_then(Value::as_str)
                .map(strip_html)
                .or_else(|| {
                    image
                        .get("description")
                        .and_then(Value::as_str)
                        .map(strip_html)
                })
                .unwrap_or_default();
            let thumb = thumbnail_url(image).or_else(|| {
                image
                    .get("image")
                    .and_then(|v| v.get("source"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
            sections.push(FeedSection {
                kind: FeedKind::PictureOfTheDay,
                items: vec![FeedItem {
                    // Show the human caption, never the raw `File:…` title.
                    title,
                    subtitle: "Picture of the day".to_string(),
                    extract: description,
                    thumbnail: thumb,
                    url: image
                        .get("file_page")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    meta: Some(format!(
                        "page:{}",
                        crate::core::clean_file_title(&raw_title)
                    )),
                }],
            });
        }

        if let Some(items) = value.get("news").and_then(Value::as_array) {
            let items = items
                .iter()
                .filter_map(|item| {
                    let story = item.get("story").and_then(Value::as_str)?;
                    let links = item.get("links").and_then(Value::as_array);
                    let title = links
                        .and_then(|links| links.first())
                        .and_then(|link| link.get("title"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| truncate(&strip_html(story), 80));
                    Some(FeedItem {
                        title,
                        subtitle: "In the news".to_string(),
                        extract: strip_html(story),
                        thumbnail: None,
                        url: None,
                        meta: None,
                    })
                })
                .take(8)
                .collect::<Vec<_>>();
            if !items.is_empty() {
                sections.push(FeedSection {
                    kind: FeedKind::InTheNews,
                    items,
                });
            }
        }

        if let Some(items) = value.get("dyk").and_then(Value::as_array) {
            let items = items
                .iter()
                .filter_map(|item| {
                    let text = item.get("text").and_then(Value::as_str)?;
                    let first_link = item
                        .get("links")
                        .and_then(Value::as_array)
                        .and_then(|links| links.first());
                    Some(FeedItem {
                        title: first_link
                            .and_then(|link| link.get("title"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| "Did you know".to_string()),
                        subtitle: "Did you know".to_string(),
                        extract: strip_html(text),
                        thumbnail: None,
                        url: None,
                        meta: None,
                    })
                })
                .take(10)
                .collect::<Vec<_>>();
            if !items.is_empty() {
                sections.push(FeedSection {
                    kind: FeedKind::DidYouKnow,
                    items,
                });
            }
        }

        if let Some(items) = value.get("onthisday").and_then(Value::as_array) {
            let items = items
                .iter()
                .filter_map(|item| {
                    let text = item.get("text").and_then(Value::as_str)?;
                    let page = item
                        .get("pages")
                        .and_then(Value::as_array)
                        .and_then(|pages| pages.first());
                    let title = page
                        .and_then(|page| page.get("normalizedtitle").or_else(|| page.get("title")))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| truncate(&strip_html(text), 80));
                    Some(FeedItem {
                        title,
                        subtitle: item
                            .get("year")
                            .and_then(Value::as_i64)
                            .map(|year| year.to_string())
                            .unwrap_or_else(|| "On this day".to_string()),
                        extract: strip_html(text),
                        thumbnail: page.and_then(thumbnail_value),
                        url: None,
                        meta: None,
                    })
                })
                .take(12)
                .collect::<Vec<_>>();
            if !items.is_empty() {
                sections.push(FeedSection {
                    kind: FeedKind::OnThisDay,
                    items,
                });
            }
        }

        if let Some(items) = value
            .pointer("/mostread/articles")
            .and_then(Value::as_array)
        {
            let items = items
                .iter()
                .filter_map(|item| {
                    let title = item.get("title").and_then(Value::as_str)?.to_string();
                    let views = item.get("views").and_then(Value::as_u64);
                    Some(FeedItem {
                        title: title.clone(),
                        subtitle: views
                            .map(|v| format!("{} views today", format_count(v)))
                            .unwrap_or_else(|| "Most read".to_string()),
                        extract: item
                            .get("extract")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        thumbnail: thumbnail_value(item),
                        url: item
                            .pointer("/content_urls/desktop/page")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        meta: Some(format!("page:{title}")),
                    })
                })
                .take(10)
                .collect::<Vec<_>>();
            if !items.is_empty() {
                sections.push(FeedSection {
                    kind: FeedKind::MostRead,
                    items,
                });
            }
        }

        Ok(Feed {
            date,
            sections,
            fetched_at: now(),
            source: Source::Network,
        })
    }

    async fn onthisday_feed(&self, date: NaiveDate) -> Result<Feed> {
        let url = format!(
            "{}/{:02}/{:02}",
            self.onthisday_base(),
            date.month(),
            date.day()
        );
        let value = self.get_json(&url).await?;
        let items = value
            .get("selected")
            .and_then(Value::as_array)
            .or_else(|| value.get("events").and_then(Value::as_array))
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let text = item.get("text").and_then(Value::as_str)?;
                        let page = item
                            .get("pages")
                            .and_then(Value::as_array)
                            .and_then(|pages| pages.first());
                        Some(FeedItem {
                            title: page
                                .and_then(|p| p.get("normalizedtitle").or_else(|| p.get("title")))
                                .and_then(Value::as_str)
                                .map(str::to_string)
                                .unwrap_or_else(|| truncate(&strip_html(text), 80)),
                            subtitle: item
                                .get("year")
                                .and_then(Value::as_i64)
                                .map(|year| year.to_string())
                                .unwrap_or_else(|| "On this day".to_string()),
                            extract: strip_html(text),
                            thumbnail: page.and_then(thumbnail_value),
                            url: None,
                            meta: None,
                        })
                    })
                    .take(16)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if items.is_empty() {
            return Err(ApiError::NotFound);
        }
        Ok(Feed {
            date,
            sections: vec![FeedSection {
                kind: FeedKind::OnThisDay,
                items,
            }],
            fetched_at: now(),
            source: Source::Network,
        })
    }

    pub async fn download(&self, url: &str, max_bytes: u64) -> Result<MediaBytes> {
        let response = self.get(url).await?;
        if let Some(length) = response.content_length()
            && length > max_bytes
        {
            return Err(ApiError::Network(format!(
                "media file is too large ({:.1} MB)",
                length as f64 / 1_048_576.0
            )));
        }
        let mut stream = response.bytes_stream();
        let mut data = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| ApiError::Network(e.to_string()))?;
            if data.len() + chunk.len() > max_bytes as usize {
                return Err(ApiError::Network(format!(
                    "media exceeds the {} MB limit",
                    max_bytes / 1_048_576
                )));
            }
            data.extend_from_slice(&chunk);
        }
        if data.is_empty() {
            return Err(ApiError::Network("empty media response".into()));
        }
        Ok(MediaBytes { data })
    }
}

fn thumbnail_url(value: &serde_json::Map<String, Value>) -> Option<String> {
    ["thumbnail", "originalimage"].iter().find_map(|key| {
        value
            .get(*key)
            .and_then(|thumb| thumb.get("source"))
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

fn thumbnail_value(value: &Value) -> Option<String> {
    ["thumbnail", "originalimage"].iter().find_map(|key| {
        value
            .get(*key)
            .and_then(|thumb| thumb.get("source"))
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

fn first_paragraph(blocks: &[crate::core::Block]) -> Option<String> {
    for block in blocks {
        match block {
            crate::core::Block::Paragraph(inlines) => {
                let text = inlines
                    .iter()
                    .map(|inline| inline.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if text.len() > 40 {
                    return Some(truncate(&text, 560));
                }
            }
            crate::core::Block::Quote(inner) => {
                if let Some(text) = first_paragraph(inner) {
                    return Some(text);
                }
            }
            _ => {}
        }
    }
    None
}

fn strip_html(html: &str) -> String {
    crate::core::strip_html(html)
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}

fn format_count(value: u64) -> String {
    match value {
        0..=999 => value.to_string(),
        1_000..=999_999 => format!("{:.1}k", value as f64 / 1_000.0),
        _ => format!("{:.1}M", value as f64 / 1_000_000.0),
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[allow(dead_code)]
pub fn media_spec(url: &str, mime: Option<&str>, title: &str) -> MediaSpec {
    MediaSpec {
        url: url.to_string(),
        kind: mime
            .map(|mime| crate::core::MediaKind::from_mime(mime, url))
            .unwrap_or_else(|| crate::core::MediaKind::from_mime("", url)),
        title: title.to_string(),
        description: String::new(),
        mime: mime.map(str::to_string),
        poster: None,
        duration: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_markup_and_truncates() {
        assert_eq!(strip_html("<b>hello</b> &amp; world"), "hello & world");
        assert_eq!(truncate("abcdef", 4), "abc…");
    }

    #[test]
    fn parses_feed_shape() {
        let value: Value = serde_json::from_str(
            r##"{
                "tfa":{"title":"A","extract":"B","description":"C",
                  "thumbnail":{"source":"https://example.test/a.jpg"},
                  "content_urls":{"desktop":{"page":"https://example.test/A"}}},
                "image":{"title":"File:X.jpg","description":{"text":"Caption"},
                  "thumbnail":{"source":"https://example.test/x.jpg"}},
                "mostread":{"articles":[{"title":"A","views":1234,"extract":"E"}]}
            }"##,
        )
        .unwrap();
        assert_eq!(
            thumbnail_url(value.get("tfa").unwrap().as_object().unwrap()),
            Some("https://example.test/a.jpg".into())
        );
        assert_eq!(strip_html("<i>Caption</i>"), "Caption");
        assert_eq!(format_count(1_234), "1.2k");
    }
}
