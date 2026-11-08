use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::core::state::math_key;
use crate::core::{
    AppTheme, Article, Block, Inline, InlineKind, LinkKind, LinkTarget, MediaKind, MediaSpec, Table,
};

#[derive(Debug, Clone, Copy)]
pub struct LayoutOptions {
    pub light: bool,
    pub media_enabled: bool,
    pub syntax_enabled: bool,
    /// Cap image/media slot height (in rows) so tall images fit the viewport
    /// and can be rendered fully without mid-scroll resizing artifacts.
    pub max_media_height: u16,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            light: false,
            media_enabled: true,
            syntax_enabled: true,
            max_media_height: 18,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LinkSpan {
    pub line: usize,
    pub start: u16,
    pub end: u16,
    pub target: LinkTarget,
}

#[derive(Debug, Clone)]
pub struct MediaPlacement {
    pub key: String,
    pub kind: MediaKind,
    pub alt: String,
    pub line: usize,
    pub height: u16,
}

#[derive(Debug, Clone)]
pub struct TocEntry {
    pub level: u8,
    pub title: String,
    pub line: u16,
}

/// A fenced code block anchored to its first rendered line, exposed for the
/// in-article element picker.
#[derive(Debug, Clone)]
pub struct CodePlacement {
    pub label: String,
    pub line: u16,
    pub lines: usize,
    /// The raw source, kept so the Ctrl+e/Alt+e preview can show the block.
    pub language: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ArticleDoc {
    pub lines: Vec<Line<'static>>,
    pub links: Vec<LinkSpan>,
    pub media: Vec<MediaPlacement>,
    pub toc: Vec<TocEntry>,
    pub code: Vec<CodePlacement>,
}

impl ArticleDoc {
    pub fn total_height(&self) -> u16 {
        self.lines.len() as u16
    }
}

pub fn build_document(
    article: &Article,
    width: u16,
    theme: AppTheme,
    opts: LayoutOptions,
) -> ArticleDoc {
    let width = width.max(24);
    let mut builder = Builder {
        width: width as usize,
        theme,
        opts,
        lines: Vec::new(),
        links: Vec::new(),
        media: Vec::new(),
        toc: Vec::new(),
        code: Vec::new(),
    };

    let title_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    // Guard against cached file-page articles whose title still carries the
    // `File:` namespace; the reader should never see it.
    let display_title = if article.display_title.starts_with("File:")
        || article.display_title.starts_with("file:")
    {
        crate::core::clean_file_title(&article.display_title)
    } else {
        article.display_title.clone()
    };
    builder.push_wrapped_plain(&display_title, title_style, 0);
    builder.push_blank();

    let summary = if article.summary.trim().is_empty() {
        first_paragraph(article)
    } else {
        article.summary.clone()
    };
    let summary =
        if summary.trim_start().starts_with("File:") || summary.trim_start().starts_with("file:") {
            crate::core::strip_file_prefix(&summary)
        } else {
            summary
        };
    if !summary.trim().is_empty() {
        let style = Style::default()
            .fg(theme.fg_dim)
            .add_modifier(Modifier::ITALIC);
        for wrapped in wrap_plain(&strip_html(&summary), builder.width, style) {
            builder.lines.push(Line::from(wrapped.spans));
        }
        builder.push_blank();
    }

    let meta = format!(
        "{} · {} · {}",
        article.language.to_uppercase(),
        article.source.label(),
        if article.blocks.is_empty() {
            "empty article".to_string()
        } else {
            format!("{} blocks", article.blocks.len())
        }
    );
    builder.push_wrapped_plain(&meta, Style::default().fg(theme.fg_dim), 0);
    builder.push_rule();
    builder.push_blank();

    if article.blocks.is_empty() {
        builder.push_wrapped_plain(
            "This page has no readable paragraphs. Press o to open it in a browser.",
            Style::default().fg(theme.warning),
            0,
        );
    } else {
        for block in &article.blocks {
            builder.block(block);
            builder.push_blank();
        }
    }

    ArticleDoc {
        lines: builder.lines,
        links: builder.links,
        media: builder.media,
        toc: builder.toc,
        code: builder.code,
    }
}

struct Builder {
    width: usize,
    theme: AppTheme,
    opts: LayoutOptions,
    lines: Vec<Line<'static>>,
    links: Vec<LinkSpan>,
    media: Vec<MediaPlacement>,
    toc: Vec<TocEntry>,
    code: Vec<CodePlacement>,
}

impl Builder {
    fn current_line(&self) -> usize {
        self.lines.len()
    }

    fn push_blank(&mut self) {
        self.lines.push(Line::from(""));
    }

    fn push_rule(&mut self) {
        let style = Style::default().fg(self.theme.muted_border);
        let line = Span::styled("─".repeat(self.width), style);
        self.lines.push(Line::from(line));
    }

    fn push_wrapped_plain(&mut self, text: &str, style: Style, indent: usize) {
        let width = self.width.saturating_sub(indent).max(8);
        let pad = " ".repeat(indent);
        for wrapped in wrap_plain(text, width, style) {
            let mut spans = Vec::new();
            if !pad.is_empty() {
                spans.push(Span::raw(pad.clone()));
            }
            spans.extend(wrapped.spans);
            self.lines.push(Line::from(spans));
        }
    }

    fn block(&mut self, block: &Block) {
        match block {
            Block::Heading {
                level,
                inlines,
                id: _,
            } => {
                let style = match level {
                    1 => Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                    2 => Style::default()
                        .fg(self.theme.secondary_accent)
                        .add_modifier(Modifier::BOLD),
                    _ => Style::default()
                        .fg(self.theme.fg_bright)
                        .add_modifier(Modifier::BOLD),
                };
                self.push_blank();
                let start = self.current_line();
                let prefix = format!("{} ", "#".repeat((*level as usize).clamp(1, 4)));
                let prefix_style = Style::default().fg(self.theme.fg_dim);
                let mut first = true;
                for mut wrapped in wrap_inlines(
                    inlines,
                    self.width.saturating_sub(prefix.len()),
                    style,
                    self.theme,
                ) {
                    if first {
                        wrapped
                            .spans
                            .insert(0, Span::styled(prefix.clone(), prefix_style));
                        for link in &mut wrapped.links {
                            link.start += prefix.width() as u16;
                            link.end += prefix.width() as u16;
                        }
                        first = false;
                    }
                    let line_no = self.current_line();
                    self.lines.push(Line::from(wrapped.spans));
                    for link in wrapped.links {
                        self.links.push(LinkSpan {
                            line: line_no,
                            start: link.start,
                            end: link.end,
                            target: link.target,
                        });
                    }
                }
                if *level <= 3 {
                    self.toc.push(TocEntry {
                        level: *level,
                        title: inline_text(inlines),
                        line: start as u16,
                    });
                }
            }
            Block::Paragraph(inlines) => {
                for wrapped in wrap_inlines(
                    inlines,
                    self.width,
                    Style::default().fg(self.theme.fg),
                    self.theme,
                ) {
                    let line_no = self.current_line();
                    self.lines.push(Line::from(wrapped.spans));
                    for link in wrapped.links {
                        self.links.push(LinkSpan {
                            line: line_no,
                            start: link.start,
                            end: link.end,
                            target: link.target,
                        });
                    }
                }
            }
            Block::List { ordered: _, items } => {
                for item in items {
                    let prefix_style = Style::default().fg(self.theme.accent);
                    let prefix = item
                        .first()
                        .map(|i| i.text.clone())
                        .filter(|t| {
                            t.starts_with('•')
                                || t.starts_with('◦')
                                || t.starts_with('▪')
                                || t.chars().next().is_some_and(|c| c.is_ascii_digit())
                        })
                        .unwrap_or_else(|| "• ".to_string());
                    let content = if prefix.len() < item.first().map(|i| i.text.len()).unwrap_or(0)
                        || item.first().is_some_and(|i| {
                            i.text.starts_with('•')
                                || i.text.starts_with('◦')
                                || i.text.starts_with('▪')
                        }) {
                        let mut rest = item.clone();
                        if let Some(first) = rest.first_mut() {
                            first.text = first.text.trim_start_matches(&prefix).to_string();
                        }
                        rest
                    } else {
                        item.clone()
                    };
                    let indent = prefix.width().max(2);
                    let width = self.width.saturating_sub(indent).max(8);
                    let mut first = true;
                    for wrapped in wrap_inlines(
                        &content,
                        width,
                        Style::default().fg(self.theme.fg),
                        self.theme,
                    ) {
                        let line_no = self.current_line();
                        let mut spans = Vec::new();
                        if first {
                            spans.push(Span::styled(prefix.clone(), prefix_style));
                            first = false;
                        } else {
                            spans.push(Span::raw(" ".repeat(indent)));
                        }
                        spans.extend(wrapped.spans);
                        self.lines.push(Line::from(spans));
                        for link in wrapped.links {
                            self.links.push(LinkSpan {
                                line: line_no,
                                start: link.start + indent as u16,
                                end: link.end + indent as u16,
                                target: link.target,
                            });
                        }
                    }
                }
            }
            Block::Quote(inner) => self.push_quote(inner),
            Block::Code { language, text } => {
                self.push_code(language.as_deref(), text);
            }
            Block::Table(table) => self.push_table(table),
            Block::Infobox { title, rows } => self.push_infobox(title.as_deref(), rows),
            Block::Gallery { images, caption } => {
                if let Some(caption) = caption {
                    self.push_caption(caption);
                }
                for image in images {
                    self.push_media(image, 9);
                }
            }
            Block::Image(spec) => self.push_media(spec, 16),
            Block::Audio(spec) | Block::Video(spec) => self.push_playable(spec),
            Block::Math { tex, display } => {
                if *display {
                    self.push_math(tex);
                } else {
                    let inline = vec![Inline::styled(
                        format!("⟨{tex}⟩"),
                        InlineKind::Math(tex.clone()),
                    )];
                    for wrapped in wrap_inlines(
                        &inline,
                        self.width,
                        Style::default().fg(self.theme.fg),
                        self.theme,
                    ) {
                        self.lines.push(Line::from(wrapped.spans));
                    }
                }
            }
            Block::Rule => self.push_rule(),
        }
    }

    fn push_caption(&mut self, caption: &str) {
        let style = Style::default()
            .fg(self.theme.fg_dim)
            .add_modifier(Modifier::ITALIC);
        let width = self.width.max(8);
        for wrapped in wrap_plain(caption, width.saturating_sub(4).max(8), style) {
            let line_w: usize = wrapped
                .spans
                .iter()
                .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                .sum();
            let pad = width.saturating_sub(line_w) / 2;
            let mut spans = Vec::new();
            if pad > 0 {
                spans.push(Span::raw(" ".repeat(pad)));
            }
            spans.extend(wrapped.spans);
            self.lines.push(Line::from(spans));
        }
    }

    /// Blockquotes get a themed left rule and a full-width tinted wash so they
    /// read as a distinct block instead of a plain indented paragraph.
    fn push_quote(&mut self, inner: &[Block]) {
        let bg = self.theme.elevated_bg;
        let rule = Style::default().fg(self.theme.accent).bg(bg);
        let text_style = Style::default()
            .fg(self.theme.fg)
            .bg(bg)
            .add_modifier(Modifier::ITALIC);
        let pad = self.width.saturating_sub(2).max(1);

        // Top rule: ▌ + rounded corner + tinted fill.
        self.lines.push(Line::from(vec![
            Span::styled("▌", rule),
            Span::styled("╭", Style::default().fg(self.theme.muted_border).bg(bg)),
            Span::styled(
                "─".repeat(pad.saturating_sub(1)),
                Style::default().fg(self.theme.muted_border).bg(bg),
            ),
        ]));

        for inner_block in inner {
            match inner_block {
                Block::Paragraph(inlines) => {
                    let width = self.width.saturating_sub(3).max(8);
                    for wrapped in wrap_inlines(inlines, width, text_style, self.theme) {
                        let line_no = self.current_line();
                        let mut spans = vec![Span::styled("▌ ", rule)];
                        let mut used = 2usize;
                        for span in wrapped.spans {
                            used += span.content.as_ref().width();
                            spans.push(Span::styled(span.content.into_owned(), text_style.bg(bg)));
                        }
                        // Pad to the block width so the wash reads as a panel.
                        let fill = self.width.saturating_sub(used);
                        spans.push(Span::styled(" ".repeat(fill), text_style));
                        self.lines.push(Line::from(spans));
                        for link in wrapped.links {
                            self.links.push(LinkSpan {
                                line: line_no,
                                start: link.start + 2,
                                end: link.end + 2,
                                target: link.target,
                            });
                        }
                    }
                }
                other => self.block(other),
            }
        }

        self.lines.push(Line::from(vec![
            Span::styled("▌", rule),
            Span::styled("╰", Style::default().fg(self.theme.muted_border).bg(bg)),
            Span::styled(
                "─".repeat(pad.saturating_sub(1)),
                Style::default().fg(self.theme.muted_border).bg(bg),
            ),
        ]));
    }

    fn push_media(&mut self, spec: &MediaSpec, requested_height: u16) {
        let alt = if spec.title.trim().is_empty() {
            spec.description.trim().to_string()
        } else {
            spec.title.trim().to_string()
        };
        let label = if alt.is_empty() {
            format!("▸ {}", spec.kind)
        } else {
            format!("▸ {} · {}", spec.kind, alt)
        };
        let label_style = Style::default()
            .fg(self.theme.tertiary_accent)
            .add_modifier(Modifier::BOLD);
        self.push_wrapped_plain(&label, label_style, 0);

        if !self.opts.media_enabled || !spec.kind.is_visual() {
            if !spec.description.trim().is_empty() {
                self.push_wrapped_plain(
                    spec.description.trim(),
                    Style::default().fg(self.theme.fg_dim),
                    2,
                );
            }
            return;
        }

        let natural = (self.width as f32 * 0.42) as u16;
        let cap = self.opts.max_media_height.max(6);
        let height = natural.clamp(6, requested_height.max(6)).min(cap);
        let start = self.current_line();
        for _ in 0..height {
            self.push_blank();
        }
        self.media.push(MediaPlacement {
            key: spec.key(),
            kind: spec.kind,
            alt: alt.clone(),
            line: start,
            height,
        });
        if !spec.description.trim().is_empty() {
            self.push_caption(&spec.description);
        }
    }

    fn push_playable(&mut self, spec: &MediaSpec) {
        let label = format!(
            "▶ {} · {}",
            match spec.kind {
                MediaKind::Video => "video",
                MediaKind::Audio => "audio",
                _ => "media",
            },
            if spec.title.is_empty() {
                spec.url.clone()
            } else {
                spec.title.clone()
            }
        );
        let target = LinkTarget {
            kind: LinkKind::Media,
            target: spec.url.clone(),
            label: label.clone(),
        };
        let style = Style::default()
            .fg(self.theme.success)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
        let line_no = self.current_line();
        self.lines.push(Line::from(Span::styled(label, style)));
        self.links.push(LinkSpan {
            line: line_no,
            start: 0,
            end: self.width.min(80) as u16,
            target,
        });
        // Expose the placement to the article-outline picker and preview.
        self.media.push(MediaPlacement {
            key: spec.key(),
            kind: spec.kind,
            alt: if spec.title.is_empty() {
                format!("{}", spec.kind)
            } else {
                spec.title.clone()
            },
            line: line_no,
            height: 1,
        });
        if !spec.description.is_empty() {
            self.push_caption(&spec.description);
        }
    }

    fn push_math(&mut self, tex: &str) {
        let label = format!("∑ {tex}");
        let style = Style::default()
            .fg(self.theme.tertiary_accent)
            .add_modifier(Modifier::ITALIC);
        self.push_wrapped_plain(&label, style, 0);
        if !self.opts.media_enabled {
            return;
        }
        let height = 7;
        let start = self.current_line();
        for _ in 0..height {
            self.push_blank();
        }
        self.media.push(MediaPlacement {
            key: math_key(tex, self.opts.light),
            kind: MediaKind::Math,
            alt: tex.to_string(),
            line: start,
            height,
        });
    }

    /// Typeset a math/LaTeX fenced block. Strips common `$`/`$$`/`\[…\]`
    /// delimiters so both bare and wrapped TeX render correctly.
    fn push_math_block(&mut self, text: &str) {
        let cleaned = clean_math_source(text);
        if cleaned.is_empty() {
            return;
        }
        let label = format!("∑ {cleaned}");
        let style = Style::default()
            .fg(self.theme.tertiary_accent)
            .add_modifier(Modifier::ITALIC);
        self.push_wrapped_plain(&label, style, 0);
        if !self.opts.media_enabled {
            return;
        }
        let height = 7;
        let start = self.current_line();
        for _ in 0..height {
            self.push_blank();
        }
        self.media.push(MediaPlacement {
            key: math_key(&cleaned, self.opts.light),
            kind: MediaKind::Math,
            alt: cleaned.clone(),
            line: start,
            height,
        });
        // Expose the block to the article-outline picker.
        self.code.push(CodePlacement {
            label: "math block".to_string(),
            line: start as u16,
            lines: height as usize,
            language: Some("math".to_string()),
            text: cleaned,
        });
    }

    fn push_code(&mut self, language: Option<&str>, text: &str) {
        // Math/LaTeX fenced blocks are typeset through the RaTeX pipeline
        // rather than syntax-highlighted as source.
        if let Some(language) = language {
            let normalized = language.trim().to_ascii_lowercase();
            if matches!(
                normalized.as_str(),
                "math" | "latex" | "tex" | "katex" | "mathml" | "amsmath"
            ) {
                self.push_math_block(text);
                return;
            }
        }
        // Reserve room for the `123 │ ` gutter so highlighted lines stay inside
        // the article column at every terminal width.
        let inner_width = self.width.saturating_sub(6).max(4);
        let language_label = language.unwrap_or("code");
        let language_label = if language_label.chars().count() > 18 {
            format!("{}…", language_label.chars().take(17).collect::<String>())
        } else {
            language_label.to_string()
        };
        let start = self.current_line();
        let lines = crate::render::syntax::highlight(
            text,
            language,
            self.theme,
            self.opts.syntax_enabled,
            inner_width,
        );
        let bg = self.theme.elevated_bg;
        let border = Style::default().fg(self.theme.muted_border).bg(bg);
        self.lines.push(Line::from(vec![
            Span::styled("  ┌─ ", border),
            Span::styled(
                language_label.clone(),
                Style::default()
                    .fg(self.theme.tertiary_accent)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", border),
        ]));
        for (i, mut line) in lines.into_iter().enumerate() {
            let mut spans = vec![Span::styled(
                format!("{:>3} │ ", i + 1),
                Style::default().fg(self.theme.fg_dim).bg(bg),
            )];
            for span in line.spans.drain(..) {
                let mut style = span.style.bg(bg);
                if style.fg.is_none() {
                    style = style.fg(self.theme.fg);
                }
                spans.push(Span::styled(span.content.into_owned(), style));
            }
            self.lines.push(Line::from(spans));
        }
        self.lines.push(Line::from(Span::styled("  └─", border)));
        self.code.push(CodePlacement {
            label: format!("{language_label} code"),
            line: start as u16,
            lines: self.lines.len().saturating_sub(start),
            language: language.map(str::to_string),
            text: text.to_string(),
        });
    }

    fn push_table(&mut self, table: &Table) {
        let ncols = table
            .rows
            .iter()
            .map(|r| r.len())
            .chain(std::iter::once(table.headers.len()))
            .max()
            .unwrap_or(0)
            .min(6);
        if ncols == 0 {
            return;
        }
        if let Some(caption) = &table.caption {
            self.push_caption(caption);
        }

        // Normalise every row to `ncols` cells.
        let mut header_cells = table.headers.clone();
        header_cells.resize(ncols, String::new());
        let rows: Vec<Vec<String>> = table
            .rows
            .iter()
            .take(90)
            .map(|row| {
                let mut cells = row.clone();
                cells.resize(ncols, String::new());
                cells
            })
            .collect();

        // Content-aware widths: start from the widest natural cell per column,
        // then scale to fit the available span.
        let separators = ncols.saturating_sub(1);
        let available = self
            .width
            .saturating_sub(separators + ncols + 1)
            .max(ncols * 4) as usize;
        let mut natural = vec![4usize; ncols];
        for (col, slot) in natural.iter_mut().enumerate() {
            let mut widest = header_cells.get(col).map(|c| c.width()).unwrap_or(0);
            for row in &rows {
                widest = widest.max(row.get(col).map(|c| c.width()).unwrap_or(0));
            }
            *slot = widest.clamp(3, 42);
        }
        let total: usize = natural.iter().sum();
        let mut col_widths = if total <= available {
            natural.clone()
        } else {
            natural
                .iter()
                .map(|w| ((*w as f64) * (available as f64) / (total as f64)).floor() as usize)
                .map(|w| w.clamp(3, 42))
                .collect()
        };
        // Distribute any rounding slack to the widest column.
        let mut sum: usize = col_widths.iter().sum();
        while sum < available {
            if let Some((idx, _)) = col_widths.iter().enumerate().max_by_key(|(_, w)| **w) {
                col_widths[idx] += 1;
                sum += 1;
            } else {
                break;
            }
        }
        while sum > available {
            if let Some(idx) = col_widths
                .iter()
                .enumerate()
                .filter(|(_, w)| **w > 3)
                .max_by_key(|(_, w)| **w)
                .map(|(i, _)| i)
            {
                col_widths[idx] -= 1;
                sum -= 1;
            } else {
                break;
            }
        }

        let border = Style::default().fg(self.theme.muted_border);
        let edge = Style::default().fg(self.theme.accent);

        // ┌──┬──┐
        let top = format!(
            "┌{}┐",
            col_widths
                .iter()
                .map(|w| "─".repeat(w + 2))
                .collect::<Vec<_>>()
                .join("┬")
        );
        self.lines.push(Line::from(Span::styled(top, edge)));

        let header_style = Style::default()
            .fg(self.theme.fg_bright)
            .bg(self.theme.elevated_bg)
            .add_modifier(Modifier::BOLD);
        self.push_themed_row(&header_cells, &col_widths, header_style, border);

        // ├──┼──┤
        let mid = format!(
            "├{}┤",
            col_widths
                .iter()
                .map(|w| "─".repeat(w + 2))
                .collect::<Vec<_>>()
                .join("┼")
        );
        self.lines.push(Line::from(Span::styled(mid, border)));

        for (index, row) in rows.iter().enumerate() {
            // Zebra striping keeps long rows readable.
            let row_style = if index % 2 == 1 {
                Style::default()
                    .fg(self.theme.fg)
                    .bg(self.theme.elevated_bg)
            } else {
                Style::default().fg(self.theme.fg).bg(self.theme.bg)
            };
            self.push_themed_row(row, &col_widths, row_style, border);
        }

        if table.rows.len() > 90 {
            self.push_wrapped_plain(
                &format!("… {} more rows", table.rows.len() - 90),
                Style::default().fg(self.theme.fg_dim),
                2,
            );
        }

        // └──┴──┘
        let bottom = format!(
            "└{}┘",
            col_widths
                .iter()
                .map(|w| "─".repeat(w + 2))
                .collect::<Vec<_>>()
                .join("┴")
        );
        self.lines.push(Line::from(Span::styled(bottom, edge)));
    }

    fn push_themed_row(
        &mut self,
        cells: &[String],
        col_widths: &[usize],
        cell_style: Style,
        border: Style,
    ) {
        let bg = cell_style.bg.unwrap_or(self.theme.bg);
        let border = border.bg(bg);
        let wrapped: Vec<Vec<String>> = cells
            .iter()
            .enumerate()
            .map(|(i, cell)| wrap_to_strings(cell, col_widths.get(i).copied().unwrap_or(3)))
            .collect();
        let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
        for line in 0..height {
            let mut spans = vec![Span::styled("│", border)];
            for (i, cell_lines) in wrapped.iter().enumerate() {
                let width = col_widths.get(i).copied().unwrap_or(3);
                spans.push(Span::styled(" ", border));
                let text = cell_lines.get(line).cloned().unwrap_or_default();
                spans.push(Span::styled(pad_right(&text, width), cell_style.bg(bg)));
                spans.push(Span::styled(" │", border));
            }
            self.lines.push(Line::from(spans));
        }
    }

    fn push_infobox(&mut self, title: Option<&str>, rows: &[(String, String)]) {
        let border = Style::default().fg(self.theme.muted_border);
        let title = title.filter(|t| !t.trim().is_empty());
        let width = self.width.saturating_sub(2).clamp(12, 72);
        if let Some(title) = title {
            self.lines.push(Line::from(vec![
                Span::styled("╭─ ", border),
                Span::styled(
                    title.to_string(),
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {}", "─".repeat(width.saturating_sub(title.width() + 4))),
                    border,
                ),
            ]));
        }
        for (key, value) in rows.iter().take(80) {
            let key_style = Style::default()
                .fg(self.theme.tertiary_accent)
                .add_modifier(Modifier::BOLD);
            let value_style = Style::default().fg(self.theme.fg);
            let key_width = key.width().min(18);
            let value_width = width.saturating_sub(key_width + 5).max(8);
            let value_lines = wrap_to_strings(value, value_width);
            for (i, text) in value_lines.iter().enumerate() {
                let mut spans = vec![Span::styled("│ ", border)];
                if i == 0 {
                    spans.push(Span::styled(
                        format!("{:<width$}", truncate(key, key_width), width = key_width),
                        key_style,
                    ));
                    spans.push(Span::styled(": ", border));
                } else {
                    spans.push(Span::raw(" ".repeat(key_width + 2)));
                }
                spans.push(Span::styled(text.clone(), value_style));
                self.lines.push(Line::from(spans));
            }
        }
        self.lines.push(Line::from(Span::styled("╰─", border)));
    }
}

#[derive(Debug, Clone)]
struct Wrapped {
    spans: Vec<Span<'static>>,
    links: Vec<WrappedLink>,
}

#[derive(Debug, Clone)]
struct WrappedLink {
    start: u16,
    end: u16,
    target: LinkTarget,
}

#[derive(Debug, Clone)]
struct Token {
    text: String,
    style: Style,
    link: Option<LinkTarget>,
    space_before: bool,
    force_break: bool,
    width: usize,
}

fn wrap_inlines(
    inlines: &[Inline],
    width: usize,
    base_style: Style,
    theme: AppTheme,
) -> Vec<Wrapped> {
    let width = width.max(4);
    let tokens = tokenize(inlines, base_style, theme);
    wrap_tokens(&tokens, width)
}

fn wrap_plain(text: &str, width: usize, style: Style) -> Vec<Wrapped> {
    let tokens = tokenize_plain(text, style);
    wrap_tokens(&tokens, width.max(4))
}

fn tokenize_plain(text: &str, style: Style) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut force_break = false;
    let mut first_line = true;
    for segment in text.split('\n') {
        if !first_line {
            force_break = true;
        }
        first_line = false;
        let leading = segment.chars().next().is_some_and(char::is_whitespace);
        for (i, word) in segment.split_whitespace().enumerate() {
            tokens.push(Token {
                text: word.to_string(),
                style,
                link: None,
                space_before: if i == 0 {
                    leading || !tokens.is_empty()
                } else {
                    true
                },
                force_break,
                width: word.width(),
            });
            force_break = false;
        }
    }
    tokens
}

fn tokenize(inlines: &[Inline], base_style: Style, theme: AppTheme) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut pending_newline = false;
    for inline in inlines {
        let (style, link) = inline_style(inline, base_style, theme);
        let preserve = matches!(inline.kind, InlineKind::Code | InlineKind::Math(_));
        if preserve {
            let text = inline.text.trim().to_string();
            if text.is_empty() {
                continue;
            }
            let token = Token {
                width: text.width(),
                text,
                style,
                link,
                space_before: true,
                force_break: pending_newline,
            };
            pending_newline = false;
            tokens.push(token);
            continue;
        }

        let mut first = true;
        for segment in inline.text.split('\n') {
            if !first {
                pending_newline = true;
            }
            first = false;
            let leading_space = segment.chars().next().is_some_and(char::is_whitespace);
            let words: Vec<&str> = segment.split_whitespace().collect();
            if words.is_empty() {
                continue;
            }
            for (i, word) in words.iter().enumerate() {
                let token = Token {
                    text: (*word).to_string(),
                    style,
                    link: link.clone(),
                    space_before: if i == 0 {
                        leading_space || !tokens.is_empty()
                    } else {
                        true
                    },
                    force_break: pending_newline,
                    width: word.width(),
                };
                pending_newline = false;
                tokens.push(token);
            }
        }
    }
    tokens
}

fn inline_style(
    inline: &Inline,
    base_style: Style,
    theme: AppTheme,
) -> (Style, Option<LinkTarget>) {
    let mut style = base_style;
    let mut link = None;
    match &inline.kind {
        InlineKind::Normal => {}
        InlineKind::Bold => style = style.add_modifier(Modifier::BOLD),
        InlineKind::Italic => style = style.add_modifier(Modifier::ITALIC),
        InlineKind::BoldItalic => {
            style = style.add_modifier(Modifier::BOLD | Modifier::ITALIC);
        }
        InlineKind::Code => {
            style = style.fg(theme.tertiary_accent).bg(theme.elevated_bg);
        }
        InlineKind::Link(target) => {
            link = Some(target.clone());
            style = style.fg(theme.accent).add_modifier(Modifier::UNDERLINED);
        }
        InlineKind::Superscript | InlineKind::Subscript => {
            style = style.fg(theme.fg_dim);
        }
        InlineKind::Math(_) => {
            style = style
                .fg(theme.secondary_accent)
                .add_modifier(Modifier::ITALIC);
        }
    }
    (style, link)
}

#[allow(unused_assignments)]
fn wrap_tokens(tokens: &[Token], width: usize) -> Vec<Wrapped> {
    let mut out = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut links: Vec<WrappedLink> = Vec::new();
    let mut x = 0usize;

    macro_rules! flush {
        () => {
            if !spans.is_empty() {
                out.push(Wrapped {
                    spans: std::mem::take(&mut spans),
                    links: std::mem::take(&mut links),
                });
            }
            x = 0;
        };
    }

    for token in tokens {
        if token.force_break && x > 0 {
            flush!();
        }
        if token.text.is_empty() {
            continue;
        }
        if x > 0 && token.space_before && x + 1 + token.width > width {
            flush!();
        } else if x > 0 && token.space_before {
            spans.push(Span::raw(" "));
            x += 1;
        }
        if x + token.width <= width {
            push_span(
                &mut spans,
                &mut links,
                &token.text,
                token.style,
                token.link.as_ref(),
                x,
            );
            x += token.width;
        } else {
            let mut chunk = String::new();
            let mut chunk_w = 0;
            for c in token.text.chars() {
                let cw = c.width().unwrap_or(0);
                if x + chunk_w + cw > width && (x + chunk_w) > 0 {
                    if !chunk.is_empty() {
                        push_span(
                            &mut spans,
                            &mut links,
                            &chunk,
                            token.style,
                            token.link.as_ref(),
                            x,
                        );
                        x += chunk_w;
                        chunk.clear();
                        chunk_w = 0;
                    }
                    flush!();
                }
                chunk.push(c);
                chunk_w += cw;
            }
            if !chunk.is_empty() {
                push_span(
                    &mut spans,
                    &mut links,
                    &chunk,
                    token.style,
                    token.link.as_ref(),
                    x,
                );
                x += chunk_w;
            }
        }
        if x >= width {
            flush!();
        }
    }
    flush!();
    if out.is_empty() {
        out.push(Wrapped {
            spans: vec![Span::raw("")],
            links: Vec::new(),
        });
    }
    out
}

fn push_span(
    spans: &mut Vec<Span<'static>>,
    links: &mut Vec<WrappedLink>,
    text: &str,
    style: Style,
    link: Option<&LinkTarget>,
    x: usize,
) {
    let w = text.width();
    if let Some(target) = link {
        if let Some(last) = links.last_mut()
            && last.target == *target
            && last.end as usize == x
        {
            last.end = (x + w) as u16;
        } else {
            links.push(WrappedLink {
                start: x as u16,
                end: (x + w) as u16,
                target: target.clone(),
            });
        }
    }
    spans.push(Span::styled(text.to_string(), style));
}

pub fn wrap_to_strings(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            out.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut used = 0;
        for word in line.split_whitespace() {
            let ww = word.width();
            if used > 0 && used + 1 + ww > width {
                out.push(current);
                current = String::new();
                used = 0;
            }
            if used > 0 {
                current.push(' ');
                used += 1;
            }
            if ww > width && used == 0 {
                for c in word.chars() {
                    let cw = c.width().unwrap_or(0);
                    if used + cw > width && used > 0 {
                        out.push(current);
                        current = String::new();
                        used = 0;
                    }
                    current.push(c);
                    used += cw;
                }
            } else {
                current.push_str(word);
                used += ww;
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn pad_right(text: &str, width: usize) -> String {
    let mut out = text.to_string();
    let used = text.width();
    if used < width {
        out.push_str(&" ".repeat(width - used));
    }
    out
}

fn truncate(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let cw = c.width().unwrap_or(0);
        if used + cw > width {
            break;
        }
        out.push(c);
        used += cw;
    }
    out
}

fn inline_text(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(|i| i.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn first_paragraph(article: &Article) -> String {
    for block in &article.blocks {
        if let Block::Paragraph(inlines) = block {
            let text = inline_text(inlines);
            if !text.is_empty() {
                return text;
            }
        }
    }
    article
        .plain_text
        .lines()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for c in input.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    html_escape::decode_html_entities(&out).to_string()
}

/// Normalise a math/LaTeX fenced block into a single TeX string: drop `$$`,
/// `$`, `\[ \]`, `\( \)` delimiters and collapse whitespace.
fn clean_math_source(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut line = line.trim();
        for (open, close) in [("\\[", "\\]"), ("\\(", "\\)")] {
            if let Some(rest) = line.strip_prefix(open) {
                line = rest;
            }
            if let Some(rest) = line.strip_suffix(close) {
                line = rest;
            }
        }
        let line = line.trim();
        let line = line
            .strip_prefix("$$")
            .and_then(|rest| rest.strip_suffix("$$"))
            .or_else(|| {
                line.strip_prefix('$')
                    .and_then(|rest| rest.strip_suffix('$'))
            })
            .unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(line.trim());
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> AppTheme {
        crate::render::theme::merged_themes()
            .into_iter()
            .next()
            .expect("theme")
            .theme
    }

    fn article(blocks: Vec<Block>) -> Article {
        Article {
            title: "Test".to_string(),
            display_title: "Test".to_string(),
            pageid: None,
            revision: None,
            language: "en".to_string(),
            html: String::new(),
            summary: "A test article.".to_string(),
            blocks,
            plain_text: "A test article.".to_string(),
            media: Vec::new(),
            fetched_at: 0,
            source: crate::core::Source::Local,
        }
    }

    #[test]
    fn wraps_long_paragraphs() {
        let blocks = vec![Block::Paragraph(vec![Inline::plain("word ".repeat(200))])];
        let doc = build_document(&article(blocks), 40, theme(), LayoutOptions::default());
        for line in &doc.lines {
            assert!(line.width() <= 40);
        }
    }

    #[test]
    fn emits_toc_entries() {
        let blocks = vec![Block::Heading {
            level: 2,
            id: Some("History".to_string()),
            inlines: vec![Inline::plain("History")],
        }];
        let doc = build_document(&article(blocks), 60, theme(), LayoutOptions::default());
        assert_eq!(doc.toc.len(), 1);
    }

    #[test]
    fn emits_math_placement() {
        let blocks = vec![Block::Math {
            tex: "x^2".to_string(),
            display: true,
        }];
        let doc = build_document(&article(blocks), 60, theme(), LayoutOptions::default());
        assert_eq!(doc.media.len(), 1);
        assert!(doc.media[0].key.starts_with("math:"));
    }

    #[test]
    fn lays_out_500kb_article_within_budget() {
        let mut html = String::with_capacity(640 * 1024);
        html.push_str("<div id=\"mw-content-text\"><div class=\"mw-parser-output\">");
        let sentence = "Terminal readers keep the interface responsive while a large article is parsed and wrapped off the UI thread. ";
        while html.len() < 600 * 1024 {
            html.push_str("<p>");
            html.push_str(&sentence.repeat(2));
            html.push_str("</p>");
        }
        html.push_str("</div></div>");
        assert!(html.len() >= 512 * 1024);

        let started = std::time::Instant::now();
        let extracted = crate::render::html::extract_article(&html, "Large article");
        assert!(!extracted.blocks.is_empty());
        let mut large = article(extracted.blocks);
        large.summary = extracted.plain_text.chars().take(240).collect();
        large.plain_text = extracted.plain_text;
        let doc = build_document(&large, 100, theme(), LayoutOptions::default());
        let elapsed = started.elapsed();

        assert!(doc.lines.len() > 1_000);
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "500 KB article took {elapsed:?}"
        );
        for line in &doc.lines {
            assert!(line.width() <= 100);
        }
    }

    #[test]
    fn keeps_code_inside_width() {
        let blocks = vec![Block::Code {
            language: Some("rust".to_string()),
            text: "fn main() { println!(\"hello world\"); }".to_string(),
        }];
        let doc = build_document(&article(blocks), 40, theme(), LayoutOptions::default());
        for line in &doc.lines {
            assert!(line.width() <= 40);
        }
    }
}
