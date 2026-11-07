use scraper::{ElementRef, Html, Node, Selector};

use crate::core::{Block, Inline, InlineKind, LinkKind, LinkTarget, MediaKind, MediaSpec, Table};

#[derive(Debug, Clone, Default)]
pub struct Extracted {
    pub title: String,
    pub blocks: Vec<Block>,
    pub plain_text: String,
    pub media: Vec<MediaSpec>,
}

const SKIP_CLASSES: &[&str] = &[
    "mw-editsection",
    "navbox",
    "vertical-navbox",
    "metadata",
    "ambox",
    "sistersitebox",
    "mw-empty-elt",
    "printfooter",
    "catlinks",
    "noprint",
    "mw-jump-link",
    "shortdescription",
    "reflist",
    "references",
    "mw-references-wrap",
    "toc",
    "mw-hidden-catlinks",
    "navigation-not-searchable",
    "portal",
    "side-box",
    "mbox-small",
    "infobox-hidden",
];

const SKIP_TAGS: &[&str] = &[
    "script", "style", "noscript", "template", "sup", "link", "meta", "title", "base",
];

pub fn extract_article(html: &str, title_hint: &str) -> Extracted {
    let document = Html::parse_document(html);
    let content_sel = Selector::parse(
        "#mw-content-text .mw-parser-output, #mw-content-text, #bodyContent, main, article, body",
    )
    .expect("valid selector");
    let root = document.select(&content_sel).next();

    let mut out = Extracted {
        title: title_hint.to_string(),
        ..Default::default()
    };
    if let Some(root) = root {
        let mut ctx = WalkCtx::default();
        walk_blocks(root, &mut out.blocks, &mut ctx, 0);
        out.media = ctx.media;
        out.plain_text = plain_from_blocks(&out.blocks);
        if out.blocks.is_empty() {
            let text = root.text().collect::<Vec<_>>().join(" ");
            if !text.trim().is_empty() {
                out.blocks.push(Block::Paragraph(vec![Inline::plain(
                    text.split_whitespace().collect::<Vec<_>>().join(" "),
                )]));
                out.plain_text = text;
            }
        }
    }

    if out.title.is_empty()
        && let Some(t) = document
            .select(&Selector::parse("title").expect("title selector"))
            .next()
    {
        let raw = t.text().collect::<String>();
        out.title = raw
            .split(" - Wikipedia")
            .next()
            .unwrap_or(&raw)
            .trim()
            .to_string();
    }
    out
}

#[derive(Default)]
struct WalkCtx {
    media: Vec<MediaSpec>,
    media_urls: std::collections::HashSet<String>,
}

impl WalkCtx {
    fn push_media(&mut self, spec: MediaSpec) {
        if spec.url.is_empty() || !self.media_urls.insert(spec.url.clone()) {
            return;
        }
        self.media.push(spec);
    }
}

fn walk_blocks(el: ElementRef<'_>, out: &mut Vec<Block>, ctx: &mut WalkCtx, depth: usize) {
    if depth > 64 || should_skip(el) {
        return;
    }
    for child in el.children() {
        if let Some(child_el) = ElementRef::wrap(child) {
            if should_skip(child_el) {
                continue;
            }
            let name = child_el.value().name();
            match name {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    let level = name[1..].parse::<u8>().unwrap_or(2);
                    let id = child_el.value().attr("id").map(str::to_string);
                    let mut inlines = Vec::new();
                    parse_inlines(child_el, InlineKind::Normal, &mut inlines, ctx);
                    trim_inlines(&mut inlines);
                    if !inlines.is_empty() {
                        out.push(Block::Heading { level, id, inlines });
                    }
                }
                "p" => {
                    let mut inlines = Vec::new();
                    parse_inlines(child_el, InlineKind::Normal, &mut inlines, ctx);
                    trim_inlines(&mut inlines);
                    if !inlines.is_empty() {
                        out.push(Block::Paragraph(inlines));
                    }
                    // A paragraph that only wraps a picture is common on Wikipedia.
                    for img in descendants_named(child_el, "img").into_iter().take(2) {
                        if let Some(spec) = media_from_img(img, ctx) {
                            out.push(Block::Image(spec));
                        }
                    }
                }
                "ul" | "ol" => {
                    let ordered = name == "ol";
                    let items = parse_list(child_el, ordered, ctx, 0);
                    if !items.is_empty() {
                        out.push(Block::List { ordered, items });
                    }
                }
                "dl" => {
                    let items = parse_dl(child_el, ctx);
                    if !items.is_empty() {
                        out.push(Block::List {
                            ordered: false,
                            items,
                        });
                    }
                }
                "blockquote" | "q" => {
                    let mut inner = Vec::new();
                    walk_blocks(child_el, &mut inner, ctx, depth + 1);
                    if inner.is_empty() {
                        let mut inlines = Vec::new();
                        parse_inlines(child_el, InlineKind::Normal, &mut inlines, ctx);
                        trim_inlines(&mut inlines);
                        if !inlines.is_empty() {
                            inner.push(Block::Paragraph(inlines));
                        }
                    }
                    if !inner.is_empty() {
                        out.push(Block::Quote(inner));
                    }
                }
                "pre" => {
                    let text = clean_code_text(&child_el.text().collect::<String>());
                    if !text.trim().is_empty() {
                        let language = code_language(child_el);
                        out.push(Block::Code { language, text });
                    }
                }
                "syntaxhighlight" | "source" => {
                    // Wikipedia/GeSHi wrappers: <syntaxhighlight lang="rust">…</syntaxhighlight>
                    let text = clean_code_text(&child_el.text().collect::<String>());
                    if !text.trim().is_empty() {
                        let language = child_el
                            .value()
                            .attr("lang")
                            .or_else(|| child_el.value().attr("language"))
                            .map(normalize_language)
                            .or_else(|| {
                                child_el.value().attr("class").and_then(language_from_class)
                            });
                        out.push(Block::Code { language, text });
                    }
                }
                "code" => {
                    let text = child_el.text().collect::<String>();
                    if !text.trim().is_empty() {
                        out.push(Block::Code {
                            language: code_language(child_el),
                            text: clean_code_text(&text),
                        });
                    }
                }
                "table" => {
                    if let Some(block) = parse_table(child_el) {
                        out.push(block);
                    }
                }
                "figure" => {
                    parse_figure(child_el, out, ctx);
                }
                "img" => {
                    if let Some(spec) = media_from_img(child_el, ctx) {
                        out.push(Block::Image(spec));
                    }
                }
                "video" => {
                    if let Some(spec) = media_from_av(child_el, MediaKind::Video, ctx) {
                        out.push(Block::Video(spec));
                    }
                }
                "audio" => {
                    if let Some(spec) = media_from_av(child_el, MediaKind::Audio, ctx) {
                        out.push(Block::Audio(spec));
                    }
                }
                "hr" => out.push(Block::Rule),
                "math" => {
                    if let Some(tex) = extract_tex(child_el) {
                        let display = child_el.value().attr("display") == Some("block")
                            || class_contains(child_el, "display");
                        out.push(Block::Math { tex, display });
                    }
                }
                "span" if is_math_element(child_el) => {
                    if let Some(tex) = extract_tex(child_el) {
                        out.push(Block::Math {
                            tex,
                            display: class_contains(child_el, "display"),
                        });
                    }
                }
                "div" | "section" | "center" | "details" | "summary" | "main" => {
                    if class_contains(child_el, "gallery") {
                        parse_gallery(child_el, out, ctx);
                    } else if class_contains(child_el, "thumb")
                        || class_contains(child_el, "float")
                        || child_el.value().attr("typeof") == Some("mw:File/Thumb")
                    {
                        parse_figure(child_el, out, ctx);
                    } else {
                        walk_blocks(child_el, out, ctx, depth + 1);
                    }
                }
                "li" => {
                    // A stray list item outside a list.
                    let mut inlines = Vec::new();
                    parse_inlines(child_el, InlineKind::Normal, &mut inlines, ctx);
                    trim_inlines(&mut inlines);
                    if !inlines.is_empty() {
                        out.push(Block::Paragraph(inlines));
                    }
                }
                _ => {
                    if has_block_children(child_el) {
                        walk_blocks(child_el, out, ctx, depth + 1);
                    } else {
                        let mut inlines = Vec::new();
                        parse_inlines(child_el, InlineKind::Normal, &mut inlines, ctx);
                        trim_inlines(&mut inlines);
                        if !inlines.is_empty() {
                            out.push(Block::Paragraph(inlines));
                        }
                    }
                }
            }
        } else if let Node::Text(text) = child.value() {
            let normalized = normalize_text(text);
            if !normalized.trim().is_empty() {
                out.push(Block::Paragraph(vec![Inline::plain(normalized)]));
            }
        }
    }
}

fn parse_list(
    el: ElementRef<'_>,
    ordered: bool,
    ctx: &mut WalkCtx,
    depth: usize,
) -> Vec<Vec<Inline>> {
    let mut out = Vec::new();
    if depth > 6 {
        return out;
    }
    let mut index = 1usize;
    for child in el.children() {
        let Some(li) = ElementRef::wrap(child) else {
            continue;
        };
        if li.value().name() != "li" || should_skip(li) {
            continue;
        }
        let mut inlines = Vec::new();
        let mut nested: Vec<ElementRef<'_>> = Vec::new();
        for node in li.children() {
            if let Some(child_el) = ElementRef::wrap(node) {
                let name = child_el.value().name();
                if name == "ul" || name == "ol" {
                    nested.push(child_el);
                    continue;
                }
                parse_one_inline(child_el, InlineKind::Normal, &mut inlines, ctx);
            } else if let Node::Text(text) = node.value() {
                push_text(&mut inlines, normalize_text(text), InlineKind::Normal);
            }
        }
        trim_inlines(&mut inlines);
        if inlines.is_empty() {
            continue;
        }
        let marker = if ordered {
            format!("{index}. ")
        } else if depth > 0 {
            "◦ ".to_string()
        } else {
            "• ".to_string()
        };
        index += 1;
        let mut marked = vec![Inline::plain(marker)];
        marked.extend(inlines);
        out.push(marked);
        for nested_el in nested {
            let nested_ordered = nested_el.value().name() == "ol";
            for mut item in parse_list(nested_el, nested_ordered, ctx, depth + 1) {
                item.insert(0, Inline::plain("  "));
                out.push(item);
            }
        }
    }
    out
}

fn parse_dl(el: ElementRef<'_>, ctx: &mut WalkCtx) -> Vec<Vec<Inline>> {
    let mut out = Vec::new();
    for child in el.children() {
        let Some(item) = ElementRef::wrap(child) else {
            continue;
        };
        let name = item.value().name();
        if name != "dt" && name != "dd" {
            continue;
        }
        let mut inlines = Vec::new();
        parse_inlines(item, InlineKind::Normal, &mut inlines, ctx);
        trim_inlines(&mut inlines);
        if inlines.is_empty() {
            continue;
        }
        let prefix = if name == "dt" { "▪ " } else { "  " };
        let mut marked = vec![Inline::plain(prefix)];
        marked.extend(inlines);
        out.push(marked);
    }
    out
}

fn parse_figure(el: ElementRef<'_>, out: &mut Vec<Block>, ctx: &mut WalkCtx) {
    let mut images = Vec::new();
    for img in el.select(&Selector::parse("img").expect("img selector")) {
        if let Some(spec) = media_from_img(img, ctx) {
            images.push(spec);
        }
    }
    let caption = el
        .select(&Selector::parse("figcaption, .thumbcaption, .gallerytext").expect("caption"))
        .next()
        .map(|c| flatten_text(c))
        .filter(|c| !c.trim().is_empty());
    for mut spec in images {
        if spec.description.is_empty() {
            spec.description = caption.clone().unwrap_or_default();
        }
        out.push(Block::Image(spec));
    }
}

fn parse_gallery(el: ElementRef<'_>, out: &mut Vec<Block>, ctx: &mut WalkCtx) {
    let mut images = Vec::new();
    for item in el.select(&Selector::parse("li.gallerybox, .gallerybox").expect("gallery box")) {
        let img = item.select(&Selector::parse("img").expect("img")).next();
        if let Some(img) = img
            && let Some(mut spec) = media_from_img(img, ctx)
        {
            spec.description = item
                .select(&Selector::parse(".gallerytext, figcaption").expect("gallery text"))
                .next()
                .map(flatten_text)
                .unwrap_or_default();
            images.push(spec);
        }
    }
    if !images.is_empty() {
        out.push(Block::Gallery {
            images,
            caption: None,
        });
    }
}

fn parse_table(el: ElementRef<'_>) -> Option<Block> {
    let is_infobox = class_contains(el, "infobox");
    let caption = el
        .select(&Selector::parse("caption").expect("caption"))
        .next()
        .map(flatten_text)
        .filter(|s| !s.trim().is_empty());
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut headers: Vec<String> = Vec::new();
    let row_sel = Selector::parse("tr").expect("tr");
    let cell_sel = Selector::parse("th, td").expect("cells");
    for (ri, tr) in el.select(&row_sel).take(141).enumerate() {
        let mut row = Vec::new();
        for cell in tr.select(&cell_sel).take(12) {
            let text = flatten_text(cell);
            if text.trim().is_empty() {
                row.push(String::new());
            } else {
                row.push(text.split_whitespace().collect::<Vec<_>>().join(" "));
            }
        }
        if row.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        if ri == 0
            && tr
                .children()
                .filter_map(ElementRef::wrap)
                .any(|c| c.value().name() == "th")
        {
            headers = row;
        } else {
            rows.push(row);
        }
        if rows.len() >= 120 {
            break;
        }
    }

    if is_infobox {
        let title = caption.clone();
        let mut pairs = Vec::new();
        for row in &rows {
            if row.len() >= 2 {
                let key = row[0].trim().to_string();
                let value = row[1..].join(" · ");
                if !key.is_empty() && !value.trim().is_empty() {
                    pairs.push((key, value));
                }
            }
        }
        if !pairs.is_empty() {
            return Some(Block::Infobox { title, rows: pairs });
        }
    }

    if rows.is_empty() && headers.is_empty() {
        return None;
    }
    Some(Block::Table(Table {
        headers,
        rows,
        caption,
    }))
}

fn parse_inlines(el: ElementRef<'_>, base: InlineKind, out: &mut Vec<Inline>, ctx: &mut WalkCtx) {
    for child in el.children() {
        match child.value() {
            Node::Text(text) => push_text(out, normalize_text(text), base.clone()),
            Node::Element(_) => {
                if let Some(child_el) = ElementRef::wrap(child) {
                    if should_skip(child_el) {
                        continue;
                    }
                    parse_one_inline(child_el, base.clone(), out, ctx);
                }
            }
            _ => {}
        }
    }
}

fn parse_one_inline(
    el: ElementRef<'_>,
    base: InlineKind,
    out: &mut Vec<Inline>,
    ctx: &mut WalkCtx,
) {
    let name = el.value().name();
    match name {
        "b" | "strong" => parse_inlines(el, combine(base, InlineKind::Bold), out, ctx),
        "i" | "em" | "cite" => parse_inlines(el, combine(base, InlineKind::Italic), out, ctx),
        "code" | "kbd" | "samp" | "var" => {
            let text = el.text().collect::<String>();
            if !text.trim().is_empty() {
                push_text(out, text, InlineKind::Code);
            }
        }
        "sup" => {
            let mut inner = Vec::new();
            parse_inlines(el, InlineKind::Superscript, &mut inner, ctx);
            if !inner.is_empty() {
                out.extend(inner);
            }
        }
        "sub" => {
            let mut inner = Vec::new();
            parse_inlines(el, InlineKind::Subscript, &mut inner, ctx);
            if !inner.is_empty() {
                out.extend(inner);
            }
        }
        "a" => {
            let href = el.value().attr("href").unwrap_or("");
            let mut inner = Vec::new();
            parse_inlines(el, base.clone(), &mut inner, ctx);
            trim_inlines(&mut inner);
            if inner.is_empty() {
                return;
            }
            let label = inline_text(&inner);
            let target = link_from_href(href, &label);
            if let Some(target) = target {
                for inline in &mut inner {
                    if matches!(inline.kind, InlineKind::Normal) {
                        inline.kind = InlineKind::Link(target.clone());
                    }
                }
                out.extend(inner);
            } else {
                out.extend(inner);
            }
        }
        "br" => push_text(out, "\n".to_string(), base),
        "img" => {
            if let Some(spec) = media_from_img(el, ctx) {
                ctx.push_media(spec);
            }
        }
        "span" if is_math_element(el) => {
            if let Some(tex) = extract_tex(el) {
                push_text(out, format!("⟨{tex}⟩"), InlineKind::Math(tex));
            }
        }
        "math" => {
            if let Some(tex) = extract_tex(el) {
                push_text(out, format!("⟨{tex}⟩"), InlineKind::Math(tex));
            }
        }
        "video" | "audio" | "source" => {}
        _ => parse_inlines(el, base, out, ctx),
    }
}

fn combine(base: InlineKind, add: InlineKind) -> InlineKind {
    match (base, add) {
        (InlineKind::Italic, InlineKind::Bold) | (InlineKind::Bold, InlineKind::Italic) => {
            InlineKind::BoldItalic
        }
        (InlineKind::BoldItalic, InlineKind::Bold)
        | (InlineKind::BoldItalic, InlineKind::Italic) => InlineKind::BoldItalic,
        (_, add) => add,
    }
}

fn push_text(out: &mut Vec<Inline>, text: String, kind: InlineKind) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut()
        && last.kind == kind
        && !text.starts_with('\n')
    {
        last.text.push_str(&text);
        return;
    }
    out.push(Inline { text, kind });
}

fn trim_inlines(inlines: &mut Vec<Inline>) {
    while let Some(first) = inlines.first_mut() {
        let trimmed = first.text.trim_start().to_string();
        if trimmed.is_empty() {
            inlines.remove(0);
        } else {
            first.text = trimmed;
            break;
        }
    }
    while let Some(last) = inlines.last_mut() {
        let trimmed = last.text.trim_end().to_string();
        if trimmed.is_empty() {
            inlines.pop();
        } else {
            last.text = trimmed;
            break;
        }
    }
    // Collapse whitespace created by adjacent inline elements.
    let mut previous_space = false;
    for inline in inlines.iter_mut() {
        if inline.kind == InlineKind::Code || matches!(inline.kind, InlineKind::Math(_)) {
            previous_space = false;
            continue;
        }
        let mut cleaned = String::with_capacity(inline.text.len());
        for c in inline.text.chars() {
            if c.is_whitespace() {
                if !previous_space {
                    cleaned.push(if c == '\n' { '\n' } else { ' ' });
                    previous_space = true;
                }
            } else {
                cleaned.push(c);
                previous_space = false;
            }
        }
        inline.text = cleaned;
    }
    if let Some(first) = inlines.first_mut() {
        first.text = first.text.trim_start().to_string();
    }
    if let Some(last) = inlines.last_mut() {
        last.text = last.text.trim_end().to_string();
    }
    inlines.retain(|i| !i.text.is_empty());
}

fn inline_text(inlines: &[Inline]) -> String {
    inlines.iter().map(|i| i.text.as_str()).collect()
}

fn media_from_img(el: ElementRef<'_>, ctx: &mut WalkCtx) -> Option<MediaSpec> {
    let src = best_img_url(el)?;
    let alt = el
        .value()
        .attr("alt")
        .or_else(|| el.value().attr("title"))
        .unwrap_or("")
        .to_string();
    let kind = MediaKind::from_mime("", &src);
    let mut spec = MediaSpec::new(src, kind);
    spec.title = alt.clone();
    spec.description = el.value().attr("data-file-title").unwrap_or("").to_string();
    ctx.push_media(spec.clone());
    Some(spec)
}

fn media_from_av(el: ElementRef<'_>, kind: MediaKind, ctx: &mut WalkCtx) -> Option<MediaSpec> {
    let mut src = el.value().attr("src").map(str::to_string);
    let mut mime = None;
    let mut poster = el.value().attr("poster").map(normalize_url);
    if src.is_none() {
        for source in el.select(&Selector::parse("source").expect("source")) {
            if let Some(candidate) = source.value().attr("src") {
                let ty = source.value().attr("type").unwrap_or("");
                src = Some(normalize_url(candidate));
                mime = if ty.is_empty() {
                    None
                } else {
                    Some(ty.to_string())
                };
                if kind == MediaKind::Video && (ty.contains("vp9") || candidate.ends_with(".webm"))
                {
                    break;
                }
            }
        }
    }
    if kind == MediaKind::Video && poster.is_none() {
        poster = el.value().attr("data-poster").map(normalize_url);
    }
    let src = src?;
    let src = normalize_url(&src);
    let mut spec = MediaSpec::new(src.clone(), kind);
    spec.title = el.value().attr("title").unwrap_or("").to_string();
    spec.mime = mime;
    spec.poster = poster;
    ctx.push_media(spec.clone());
    Some(spec)
}

fn best_img_url(el: ElementRef<'_>) -> Option<String> {
    if let Some(srcset) = el.value().attr("srcset") {
        let mut best: Option<(f32, String)> = None;
        for part in srcset.split(',') {
            let mut words = part.split_whitespace();
            let Some(url) = words.next() else { continue };
            let width = words
                .next()
                .and_then(|w| {
                    w.trim_end_matches('w')
                        .trim_end_matches('x')
                        .parse::<f32>()
                        .ok()
                })
                .unwrap_or(1.0);
            if best.as_ref().map(|(w, _)| width > *w).unwrap_or(true) {
                best = Some((width, normalize_url(url)));
            }
        }
        if let Some((_, url)) = best {
            return Some(upgrade_thumbnail(url));
        }
    }
    el.value()
        .attr("src")
        .or_else(|| el.value().attr("data-src"))
        .map(normalize_url)
        .map(upgrade_thumbnail)
}

fn upgrade_thumbnail(url: String) -> String {
    // Wikipedia thumbnails end in /220px-Foo.jpg; 960px is a good terminal size.
    if let Some(pos) = url.rfind("/thumb/") {
        let tail = &url[pos + 7..];
        if let Some(slash) = tail.rfind('/') {
            let file = &tail[..slash];
            let last = file.rsplit('/').next().unwrap_or(file);
            if let Some(px) = last.find("px-") {
                let rest = &last[px + 3..];
                return format!("{}/thumb/{}/960px-{}", &url[..pos], file, rest);
            }
        }
    }
    url
}

fn normalize_url(raw: &str) -> String {
    let raw = raw.trim();
    if raw.starts_with("//") {
        format!("https:{raw}")
    } else if raw.starts_with('/') {
        format!("https://en.wikipedia.org{raw}")
    } else {
        raw.to_string()
    }
}

fn link_from_href(href: &str, label: &str) -> Option<LinkTarget> {
    if href.is_empty() {
        return None;
    }
    if href.starts_with("#") {
        return Some(LinkTarget {
            kind: LinkKind::Citation,
            target: href.to_string(),
            label: label.to_string(),
        });
    }
    if let Some(rest) = href.strip_prefix("/wiki/") {
        let title = rest.split('#').next().unwrap_or(rest);
        let title = percent_encoding::percent_decode_str(title)
            .decode_utf8_lossy()
            .replace('_', " ");
        return Some(LinkTarget {
            kind: LinkKind::Article,
            target: title,
            label: label.to_string(),
        });
    }
    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(LinkTarget {
            kind: LinkKind::External,
            target: href.to_string(),
            label: label.to_string(),
        });
    }
    if href.starts_with("/") {
        return Some(LinkTarget {
            kind: LinkKind::External,
            target: format!("https://en.wikipedia.org{href}"),
            label: label.to_string(),
        });
    }
    None
}

fn is_math_element(el: ElementRef<'_>) -> bool {
    class_contains(el, "mwe-math-element") || el.value().name() == "math"
}

fn extract_tex(el: ElementRef<'_>) -> Option<String> {
    if let Some(data_mw) = el.value().attr("data-mw")
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(data_mw)
    {
        if let Some(tex) = v
            .get("body")
            .and_then(|b| b.get("extsrc"))
            .and_then(|v| v.as_str())
            && !tex.trim().is_empty()
        {
            return Some(tex.trim().to_string());
        }
        if let Some(tex) = v
            .get("attrs")
            .and_then(|a| a.get("math"))
            .and_then(|v| v.as_str())
            && !tex.trim().is_empty()
        {
            return Some(tex.trim().to_string());
        }
    }
    let annotation_sel = Selector::parse("annotation").ok()?;
    for annotation in el.select(&annotation_sel) {
        let encoding = annotation.value().attr("encoding").unwrap_or("");
        if encoding.contains("tex") {
            let text = annotation.text().collect::<String>();
            if !text.trim().is_empty() {
                return Some(text.trim().to_string());
            }
        }
    }
    if let Some(alt) = el
        .select(&Selector::parse("img").ok()?)
        .next()
        .and_then(|img| img.value().attr("alt"))
        && !alt.trim().is_empty()
    {
        return Some(alt.trim().to_string());
    }
    None
}

fn language_from_class(class: &str) -> Option<String> {
    for token in class.split_whitespace() {
        if let Some(lang) = token.strip_prefix("language-") {
            return Some(normalize_language(lang));
        }
        if let Some(lang) = token.strip_prefix("lang-") {
            return Some(normalize_language(lang));
        }
        if let Some(lang) = token.strip_prefix("highlight-") {
            return Some(normalize_language(lang));
        }
    }
    None
}

/// Normalise a Wikipedia/GeSHi language token into a syntect token.
fn normalize_language(lang: &str) -> String {
    let normalized = lang.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "r" | "rs" | "rust" => "rust".into(),
        "py" | "py3" | "python" | "python3" => "python".into(),
        "js" | "jsx" | "javascript" => "javascript".into(),
        "ts" | "tsx" | "typescript" => "typescript".into(),
        "c" => "c".into(),
        "cpp" | "cxx" | "cc" | "c++" | "cplusplus" => "cpp".into(),
        "csharp" | "cs" | "c#" => "csharp".into(),
        "sh" | "bash" | "zsh" | "shell" | "shellscript" | "console" | "terminal" => "bash".into(),
        "powershell" | "pwsh" => "powershell".into(),
        "yml" | "yaml" => "yaml".into(),
        "toml" => "toml".into(),
        "md" | "markdown" => "markdown".into(),
        "json" => "json".into(),
        "html" | "xhtml" | "svg" => "html".into(),
        "xml" | "xhtml-xml" => "xml".into(),
        "css" => "css".into(),
        "sql" => "sql".into(),
        "vb" | "visualbasic" => "vbnet".into(),
        _ => normalized,
    }
}

/// Recover the language for a code element: the element's own class/lang attrs
/// or its `<syntaxhighlight>`/`<source>` ancestor's `lang`.
fn code_language(el: ElementRef<'_>) -> Option<String> {
    el.value()
        .attr("class")
        .and_then(language_from_class)
        .or_else(|| {
            el.value()
                .attr("lang")
                .or_else(|| el.value().attr("language"))
                .map(normalize_language)
        })
        .or_else(|| {
            el.select(&Selector::parse("code").ok()?)
                .next()
                .and_then(|c| c.value().attr("class").and_then(language_from_class))
        })
        .or_else(|| ancestor_language(el))
}

/// Walk up the DOM for a `syntaxhighlight`/`source` wrapper carrying `lang`.
fn ancestor_language(el: ElementRef<'_>) -> Option<String> {
    let mut parent = el.parent();
    while let Some(node) = parent {
        if let Some(ancestor) = ElementRef::wrap(node) {
            let name = ancestor.value().name();
            if name == "syntaxhighlight" || name == "source" {
                let raw = ancestor
                    .value()
                    .attr("lang")
                    .map(str::to_string)
                    .or_else(|| ancestor.value().attr("language").map(str::to_string))
                    .or_else(|| ancestor.value().attr("class").and_then(language_from_class));
                return raw.map(|lang| normalize_language(&lang));
            }
        }
        parent = node.parent();
    }
    None
}

fn should_skip(el: ElementRef<'_>) -> bool {
    if SKIP_TAGS.contains(&el.value().name()) {
        return true;
    }
    let class = el.value().attr("class").unwrap_or("");
    SKIP_CLASSES
        .iter()
        .any(|skip| class.split_whitespace().any(|c| c == *skip))
        || el.value().attr("aria-hidden") == Some("true") && el.value().name() != "span"
}

fn class_contains(el: ElementRef<'_>, needle: &str) -> bool {
    el.value()
        .attr("class")
        .map(|c| c.split_whitespace().any(|token| token == needle))
        .unwrap_or(false)
}

fn has_block_children(el: ElementRef<'_>) -> bool {
    const BLOCKS: &[&str] = &[
        "p",
        "div",
        "section",
        "table",
        "ul",
        "ol",
        "dl",
        "blockquote",
        "pre",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "figure",
        "hr",
    ];
    el.children()
        .filter_map(ElementRef::wrap)
        .any(|c| BLOCKS.contains(&c.value().name()))
}

fn descendants_named<'a>(el: ElementRef<'a>, name: &str) -> Vec<ElementRef<'a>> {
    let selector = Selector::parse(name).expect("valid tag selector");
    el.select(&selector).collect()
}

fn flatten_text(el: ElementRef<'_>) -> String {
    let mut parts = Vec::new();
    collect_text(el, &mut parts);
    parts
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn collect_text(el: ElementRef<'_>, parts: &mut Vec<String>) {
    for child in el.children() {
        match child.value() {
            Node::Text(t) => {
                let t = t.trim();
                if !t.is_empty() {
                    parts.push(t.to_string());
                }
            }
            Node::Element(_) => {
                if let Some(child_el) = ElementRef::wrap(child)
                    && !should_skip(child_el)
                {
                    collect_text(child_el, parts);
                }
            }
            _ => {}
        }
    }
}

fn normalize_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !space {
                out.push(if c == '\n' { '\n' } else { ' ' });
                space = true;
            }
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

fn clean_code_text(text: &str) -> String {
    text.replace('\u{a0}', " ").replace("\r\n", "\n")
}

fn plain_from_blocks(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Heading { inlines, .. } | Block::Paragraph(inlines) => {
                out.push_str(&inline_text(inlines));
                out.push('\n');
            }
            Block::List { items, .. } => {
                for item in items {
                    out.push_str(&inline_text(item));
                    out.push('\n');
                }
            }
            Block::Quote(inner) => {
                out.push_str(&plain_from_blocks(inner));
            }
            Block::Code { text, .. } => {
                out.push_str(text);
                out.push('\n');
            }
            Block::Table(table) => {
                if let Some(caption) = &table.caption {
                    out.push_str(caption);
                    out.push('\n');
                }
                for row in &table.rows {
                    out.push_str(&row.join(" "));
                    out.push('\n');
                }
            }
            Block::Infobox { title, rows } => {
                if let Some(title) = title {
                    out.push_str(title);
                    out.push('\n');
                }
                for (key, value) in rows {
                    out.push_str(key);
                    out.push(' ');
                    out.push_str(value);
                    out.push('\n');
                }
            }
            Block::Math { tex, .. } => {
                out.push_str(tex);
                out.push('\n');
            }
            Block::Image(media) | Block::Audio(media) | Block::Video(media) => {
                out.push_str(&media.title);
                out.push('\n');
            }
            Block::Gallery { images, .. } => {
                for media in images {
                    out.push_str(&media.title);
                    out.push('\n');
                }
            }
            Block::Rule => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_basic_blocks() {
        let html = r#"<html><body><div class="mw-parser-output">
            <h2 id="History">History</h2>
            <p>Hello <b>world</b> and <a href="/wiki/Rust_(programming_language)">Rust</a>.</p>
            <ul><li>One</li><li>Two</li></ul>
            <pre class="language-rust"><code>fn main() {}</code></pre>
        </div></body></html>"#;
        let out = extract_article(html, "Test");
        assert!(
            out.blocks
                .iter()
                .any(|b| matches!(b, Block::Heading { .. }))
        );
        assert!(out.blocks.iter().any(|b| matches!(b, Block::List { .. })));
        assert!(out.blocks.iter().any(|b| matches!(b, Block::Code { .. })));
        assert!(out.plain_text.contains("Hello world"));
    }

    #[test]
    fn extracts_math_tex() {
        let html = r#"<span class="mwe-math-element"><math><semantics><annotation encoding="application/x-tex">x^2</annotation></semantics></math></span>"#;
        let out = extract_article(html, "");
        assert!(
            out.blocks
                .iter()
                .any(|b| matches!(b, Block::Math { tex, .. } if tex == "x^2"))
        );
    }

    #[test]
    fn extracts_video_sources() {
        let html = r#"<video poster="//example.org/p.jpg"><source src="//example.org/v.webm" type="video/webm; codecs=vp9"></video>"#;
        let out = extract_article(html, "");
        assert!(out.media.iter().any(|m| m.kind == MediaKind::Video));
    }

    #[test]
    fn extracts_syntaxhighlight_language() {
        let html = r#"<div class="mw-parser-output">
            <syntaxhighlight lang="rust" line="1">fn main() {}</syntaxhighlight>
            <div class="mw-highlight"><pre><span class="kd">let</span> x = 1;</pre></div>
        </div>"#;
        let out = extract_article(html, "");
        let codes: Vec<_> = out
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Code { language, text } => Some((language.clone(), text.clone())),
                _ => None,
            })
            .collect();
        assert!(
            codes
                .iter()
                .any(|(lang, _)| lang.as_deref() == Some("rust")),
            "expected a rust code block, got {codes:?}"
        );
        assert!(
            codes.iter().any(|(_, text)| text.contains("fn main")),
            "expected the syntaxhighlight body to survive"
        );
    }

    #[test]
    fn recovers_language_from_ancestor_wrapper() {
        let html = r#"<syntaxhighlight lang="python"><pre>print(1)</pre></syntaxhighlight>"#;
        let out = extract_article(html, "");
        assert!(
            out.blocks.iter().any(|b| matches!(
                b,
                Block::Code { language, .. } if language.as_deref() == Some("python")
            )),
            "expected python language recovered for pre: {:?}",
            out.blocks
        );
    }

    #[test]
    fn normalizes_language_aliases() {
        assert_eq!(normalize_language("rs"), "rust");
        assert_eq!(normalize_language("py"), "python");
        assert_eq!(normalize_language("JS"), "javascript");
        assert_eq!(normalize_language("sh"), "bash");
        assert_eq!(normalize_language("cpp"), "cpp");
    }
}
