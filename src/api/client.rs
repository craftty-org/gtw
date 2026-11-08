use std::time::Duration;

use chrono::{Datelike, NaiveDate};
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
    http: tuikit::http::HttpClient,
    language: String,
}

impl WikiClient {
    pub fn new(config: &Config) -> Result<Self> {
        let ua = config.sanitized_user_agent();
        let timeout = Duration::from_secs(config.network.timeout_secs.clamp(5, 120));
        let http = tuikit::http::HttpClient::new(tuikit::http::HttpClientConfig {
            user_agent: ua,
            timeout,
            connect_timeout: Duration::from_secs(10),
            retries: config.network.retries.min(4),
            redirect_limit: 8,
            default_accept: Some("application/json, text/html;q=0.9, */*;q=0.5".to_string()),
            extra_headers: Vec::new(),
        })
        .map_err(ApiError::from)?;
        Ok(Self {
            http,
            language: config.general.language.clone(),
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

    async fn get_json(&self, url: &str) -> Result<Value> {
        self.http
            .get_json::<Value>(url, MAX_JSON_BYTES)
            .await
            .map_err(ApiError::from)
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
        let data = self
            .http
            .download_bytes(url, max_bytes)
            .await
            .map_err(ApiError::from)?;
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
    tuikit::text::truncate_chars(text, max_chars, "…")
}

fn format_count(value: u64) -> String {
    tuikit::text::format_compact_count(value)
}

fn now() -> i64 {
    tuikit::time::unix_now_secs()
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
