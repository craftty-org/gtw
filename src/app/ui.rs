use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::core::state::SavedFilter;
use crate::core::{
    AppState, AppTheme, ElementFilter, Feed, HitAction, HitRegion, LinkKind, LoadError, LoadStage,
    Loadable, MediaKind, MediaSpec, PickerMode, Screen, Source, days_in_month,
};
use crate::render::images::ImageRenderer;
use crate::render::wordmark::{render_globe_mark, render_wordmark};

const SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

pub fn draw(frame: &mut Frame<'_>, state: &mut AppState, images: &mut ImageRenderer) {
    let area = frame.area();
    let theme = state.theme();
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.bg).fg(theme.fg)),
        area,
    );
    state.hitmap.clear();
    if area.width < 24 || area.height < 7 {
        frame.render_widget(
            Paragraph::new("gtw — terminal is too small")
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.error)),
            area,
        );
        return;
    }
    if state.screen == Screen::Splash {
        draw_splash(frame, state, area, theme);
        return;
    }

    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);
    let mut regions = Vec::new();
    draw_header(frame, state, chunks[0], theme, &mut regions);
    let body = chunks[2];

    state.article_view_height = body.height.saturating_sub(7).max(4);
    state.list_view_height = body.height.saturating_sub(4).max(1);

    match state.screen {
        Screen::Splash => {}
        Screen::Home => draw_home(frame, state, body, theme, images, &mut regions),
        Screen::Article => draw_article(frame, state, body, theme, images, &mut regions),
        Screen::Bookmarks => draw_bookmarks(frame, state, body, theme, &mut regions),
        Screen::History => draw_history(frame, state, body, theme, &mut regions),
        Screen::Lists => draw_lists(frame, state, body, theme, &mut regions),
        Screen::ListDetail => draw_list_detail(frame, state, body, theme, &mut regions),
        Screen::MediaLab => draw_media_lab(frame, state, body, theme, images, &mut regions),
    }

    draw_footer(frame, state, chunks[3], theme);
    if state.picker.is_some() {
        draw_picker_overlay(frame, state, area, theme, images, &mut regions);
    }
    if state.overlay != crate::core::Overlay::None {
        draw_overlay(frame, state, area, theme, images, &mut regions);
    }
    draw_toast(frame, state, area, theme);
    state.hitmap.extend(regions);
}

fn hit(regions: &mut Vec<HitRegion>, area: Rect, action: HitAction, z: u8) {
    if area.width > 0 && area.height > 0 {
        regions.push(HitRegion { area, action, z });
    }
}

fn row(area: Rect, y: u16) -> Rect {
    Rect::new(area.x, y, area.width, 1)
}

fn spinner(state: &AppState) -> &'static str {
    SPINNER[(state.tick as usize / 2) % SPINNER.len()]
}

fn truncate(text: &str, max: usize) -> String {
    let width = UnicodeWidthStr::width(text);
    if width <= max {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

fn section_style(theme: AppTheme) -> Style {
    Style::default()
        .fg(theme.fg)
        .bg(theme.bg)
        .add_modifier(Modifier::BOLD)
}

fn pane(theme: AppTheme) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.muted_border))
        .style(Style::default().bg(theme.pane_bg).fg(theme.fg))
}

fn draw_splash(frame: &mut Frame<'_>, state: &mut AppState, area: Rect, theme: AppTheme) {
    // 9 logo rows + tagline + activity + prompt, centred on a fixed block.
    let chunks = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(13),
        Constraint::Fill(1),
    ])
    .split(area);
    let center = chunks[1];
    let mark_area = Rect::new(
        center.x + center.width.saturating_sub(19) / 2,
        center.y,
        19.min(center.width),
        9.min(center.height),
    );
    render_wordmark(mark_area, frame.buffer_mut(), theme.accent);
    frame.render_widget(
        Paragraph::new("WIKIPEDIA · The Free Encyclopedia")
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.fg_bright)),
        Rect::new(center.x, center.y.saturating_add(10), center.width, 1),
    );
    let activity = if let Some(activity) = state.activity.highest_priority() {
        format!("{} {}", spinner(state), activity.label)
    } else {
        "warming the cache…".to_string()
    };
    frame.render_widget(
        Paragraph::new(activity)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.secondary_accent).bg(theme.bg)),
        Rect::new(center.x, center.y.saturating_add(11), center.width, 1),
    );
    frame.render_widget(
        Paragraph::new("click or press any key")
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.fg_dim).bg(theme.bg)),
        Rect::new(center.x, center.y.saturating_add(12), center.width, 1),
    );
}

fn draw_header(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    theme: AppTheme,
    regions: &mut Vec<HitRegion>,
) {
    frame.render_widget(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(theme.muted_border))
            .style(Style::default().bg(theme.elevated_bg).fg(theme.fg)),
        area,
    );
    let mark = Rect::new(area.x + 1, area.y, 9.min(area.width), 3);
    render_globe_mark(mark, frame.buffer_mut(), theme.accent);
    hit(regions, mark, HitAction::Sidebar(Screen::Home), 4);
    let wordmark = Rect::new(area.x + 11, area.y + 1, 12.min(area.width), 1);
    frame.render_widget(
        Paragraph::new("WIKIPEDIA").style(
            Style::default()
                .fg(theme.fg_bright)
                .bg(theme.elevated_bg)
                .add_modifier(Modifier::BOLD),
        ),
        wordmark,
    );

    let mut x = area.x.saturating_add(24);
    if area.width > 96 {
        // Only show live activity — no "N cached · 3-day cache" banner.
        if let Some(activity) = state.activity.highest_priority() {
            let label = format!("{} {}  ", spinner(state), activity.label);
            let width = label.width() as u16;
            if width < area.width.saturating_sub(20) {
                frame.render_widget(
                    Paragraph::new(label)
                        .alignment(Alignment::Right)
                        .style(Style::default().fg(theme.fg_dim).bg(theme.elevated_bg)),
                    Rect::new(
                        area.x + area.width.saturating_sub(width),
                        area.y,
                        width,
                        area.height,
                    ),
                );
            }
        }
    }

    if state.tabs.is_empty() || area.width < 68 {
        frame.render_widget(
            Paragraph::new(format!(
                "  {}  ·  {}",
                state.screen.title(),
                state.feed_date
            ))
            .style(Style::default().fg(theme.fg_bright).bg(theme.elevated_bg)),
            row(area, area.y + 2),
        );
        return;
    }

    for (index, tab) in state.tabs.iter().take(6).enumerate() {
        let active = state.active_tab == Some(tab.id);
        let title = truncate(&tab.title, 14);
        let label = format!(" {} {title} × ", index + 1);
        let width = label.width() as u16;
        if x + width > area.x + area.width.saturating_sub(2) {
            break;
        }
        let style = if active {
            Style::default()
                .fg(theme.selection_fg)
                .bg(theme.selection_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.fg_dim).bg(theme.elevated_bg)
        };
        let tab_area = Rect::new(x, area.y + 2, width.saturating_sub(2), 1);
        frame.render_widget(
            Paragraph::new(label).style(style),
            row(area, area.y + 2).intersection(Rect::new(x, area.y + 2, width, 1)),
        );
        hit(regions, tab_area, HitAction::Tab(tab.id), 4);
        hit(
            regions,
            Rect::new(x + width.saturating_sub(2), area.y + 2, 2, 1),
            HitAction::CloseTab(tab.id),
            5,
        );
        x = x.saturating_add(width);
    }
}

fn draw_footer(frame: &mut Frame<'_>, state: &AppState, area: Rect, theme: AppTheme) {
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.elevated_bg).fg(theme.fg_dim)),
        area,
    );
    let hint = if let Some(hover) = &state.hover {
        format!(
            "{} · {}",
            link_kind_label(hover.target.kind),
            truncate(&hover.target.label, 48)
        )
    } else {
        match state.screen {
            Screen::Splash => "click or press any key".to_string(),
            Screen::Home => {
                "[j/k] select · [enter] read · [←/→] date · [alt+d] calendar · [/] search · [?] help"
                    .to_string()
            }
            Screen::Article => {
                "[j/k] scroll · [alt+j/k] links · [ctrl+k] peek · [alt+e] outline · [ctrl+f] find · [B] save · [esc] back"
                    .to_string()
            }
            Screen::Bookmarks | Screen::History => {
                "[j/k] select · [enter] read · [esc] back".to_string()
            }
            Screen::Lists => "[j/k] list · [enter] open · [n] new · [D] delete".to_string(),
            Screen::ListDetail => {
                "[j/k] select · [enter] read · [d] remove · [c] lists".to_string()
            }
            Screen::MediaLab => {
                "[j/k] preview · [enter] load · [space] play/pause · [b] save".to_string()
            }
        }
    };
    let mut line = vec![Span::styled(
        format!(
            " {} ",
            truncate(&hint, area.width.saturating_sub(18) as usize)
        ),
        Style::default().fg(theme.fg_dim),
    )];
    if let Some(toast) = &state.toast
        && !toast.expired()
    {
        line.push(Span::styled(
            format!(" · {}", truncate(&toast.text, 28)),
            Style::default().fg(if toast.error {
                theme.error
            } else {
                theme.success
            }),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(line)), area);
    if let Some(activity) = state.activity.highest_priority() {
        let text = format!("{} {} ", spinner(state), truncate(&activity.label, 18));
        let width = text.width() as u16;
        frame.render_widget(
            Paragraph::new(text).style(Style::default().fg(theme.secondary_accent)),
            Rect::new(area.x + area.width.saturating_sub(width), area.y, width, 1),
        );
    }
}

fn draw_home(
    frame: &mut Frame<'_>,
    state: &mut AppState,
    area: Rect,
    theme: AppTheme,
    images: &mut ImageRenderer,
    regions: &mut Vec<HitRegion>,
) {
    let layout = Layout::vertical([Constraint::Length(3), Constraint::Min(4)]).split(area);
    draw_feed_nav(frame, state, layout[0], theme, regions);
    if draw_load_state(frame, state, layout[1], &state.home, "today’s Wikipedia") {
        return;
    }
    let Some(feed) = state.home.value_arc() else {
        return;
    };
    if area.width >= 118 {
        // Full-width: list on the left, sticky preview pinned to the right.
        let split =
            Layout::horizontal([Constraint::Min(50), Constraint::Length(41)]).split(layout[1]);
        draw_feed_list(frame, state, split[0], &feed, theme, regions);
        draw_feed_preview(frame, state, split[1], &feed, theme, images);
    } else {
        // Narrow: one row per story with a full-width preview pinned to the bottom.
        let preview_height = (layout[1].height / 3).clamp(6, 10);
        let split = Layout::vertical([Constraint::Min(3), Constraint::Length(preview_height)])
            .split(layout[1]);
        draw_feed_list(frame, state, split[0], &feed, theme, regions);
        draw_feed_preview(frame, state, split[1], &feed, theme, images);
    }
}

fn draw_feed_nav(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    theme: AppTheme,
    regions: &mut Vec<HitRegion>,
) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(theme.muted_border))
        .style(Style::default().bg(theme.bg));
    frame.render_widget(block, area);
    if let Some(feed) = state.home.value() {
        let mut x = area.x.saturating_add(1);
        for (index, section) in feed.sections.iter().enumerate() {
            let label = format!(" {} ", section.kind.title());
            let width = label.width() as u16;
            if x.saturating_add(width) > area.right().saturating_sub(1) {
                break;
            }
            let style = if index == state.feed_section {
                Style::default()
                    .fg(theme.selection_fg)
                    .bg(theme.selection_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg_dim).bg(theme.bg)
            };
            frame.render_widget(
                Paragraph::new(label).style(style),
                Rect::new(x, area.y, width, 1),
            );
            hit(
                regions,
                Rect::new(x, area.y, width, 1),
                HitAction::FeedSection(index),
                3,
            );
            x = x.saturating_add(width);
        }
    }
    let date = state.feed_date;
    let label = date.format("%A, %B %-d, %Y").to_string();
    let prev_label = date
        .pred_opt()
        .map(|d| d.format("%-d %b").to_string())
        .unwrap_or_default();
    let next_label = date
        .succ_opt()
        .map(|d| d.format("%-d %b").to_string())
        .unwrap_or_default();
    let left = format!("‹ {prev_label}");
    let right = format!("{next_label} ›");
    let left_width = left.width() as u16;
    let right_width = right.width() as u16;
    let label_width = label.width() as u16;

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                left.clone(),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("   ", Style::default()),
            Span::styled(
                label,
                Style::default()
                    .fg(theme.fg_bright)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("   ", Style::default()),
            Span::styled(
                right.clone(),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "        alt+d calendar   r refresh",
                Style::default().fg(theme.fg_dim),
            ),
        ]))
        .style(Style::default().bg(theme.bg)),
        row(area, area.y + 1),
    );

    // Exact hit regions: the `‹`/`›` cells and the centred date label.
    hit(
        regions,
        Rect::new(area.x, area.y + 1, left_width, 1),
        HitAction::FeedPrev,
        4,
    );
    let date_x = area.x + left_width + 3;
    hit(
        regions,
        Rect::new(date_x, area.y + 1, label_width, 1),
        HitAction::OpenCalendar,
        4,
    );
    let right_x = date_x + label_width + 3;
    hit(
        regions,
        Rect::new(right_x, area.y + 1, right_width, 1),
        HitAction::FeedNext,
        4,
    );
}

fn draw_feed_list(
    frame: &mut Frame<'_>,
    state: &mut AppState,
    area: Rect,
    feed: &Feed,
    theme: AppTheme,
    regions: &mut Vec<HitRegion>,
) {
    let flat = state.feed_items_flat();
    if flat.is_empty() {
        frame.render_widget(
            Paragraph::new("No stories for this date. Try another day or refresh.")
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.fg_dim).bg(theme.bg))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let item_height: u16 = 1;
    let per_page = (area.height / item_height).max(1) as usize;
    let selected = flat
        .iter()
        .position(|&(section, item)| section == state.feed_section && item == state.feed_item)
        .unwrap_or(0);
    let mut scroll = state.feed_scroll as usize;
    if selected < scroll {
        scroll = selected;
    } else if selected >= scroll + per_page {
        scroll = selected.saturating_sub(per_page - 1);
    }
    state.feed_scroll = scroll as u16;

    frame.render_widget(Block::default().style(Style::default().bg(theme.bg)), area);
    let mut y = area.y;
    for (flat_index, &(section_index, item_index)) in flat.iter().enumerate().skip(scroll) {
        if y >= area.bottom() {
            break;
        }
        let Some(section) = feed.sections.get(section_index) else {
            continue;
        };
        let Some(item) = section.items.get(item_index) else {
            continue;
        };
        let selected_item = flat_index == selected;
        let row_area = Rect::new(area.x, y, area.width, item_height);
        let bg = if selected_item {
            theme.selection_bg
        } else {
            theme.bg
        };
        let fg = if selected_item {
            theme.selection_fg
        } else {
            theme.fg
        };
        let title = truncate(&item.title, area.width.saturating_sub(5) as usize);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    if selected_item { " ▸ " } else { "   " },
                    Style::default()
                        .fg(theme.accent)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    title,
                    Style::default()
                        .fg(fg)
                        .bg(bg)
                        .add_modifier(if selected_item {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ]))
            .style(Style::default().bg(bg).fg(fg)),
            row_area,
        );
        hit(
            regions,
            row_area,
            HitAction::FeedItem {
                section: section_index,
                item: item_index,
            },
            2,
        );
        y += item_height;
    }

    if flat.len() > per_page {
        let track = area.height as usize;
        let thumb = (track * per_page / flat.len()).max(1);
        let start = track * scroll / flat.len();
        for index in 0..track {
            let ch = if index >= start && index < start + thumb {
                "▐"
            } else {
                " "
            };
            frame.render_widget(
                Paragraph::new(ch).style(Style::default().fg(theme.muted_border).bg(theme.bg)),
                Rect::new(area.right().saturating_sub(1), area.y + index as u16, 1, 1),
            );
        }
    }
}

fn draw_feed_preview(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    feed: &Feed,
    theme: AppTheme,
    images: &mut ImageRenderer,
) {
    frame.render_widget(pane(theme), area);
    let Some(section) = feed.sections.get(state.feed_section) else {
        return;
    };
    let Some(item) = section.items.get(state.feed_item) else {
        return;
    };
    frame.render_widget(pane(theme).title(" preview "), area);
    let inner = Rect::new(
        area.x + 2,
        area.y + 1,
        area.width.saturating_sub(4),
        area.height.saturating_sub(2),
    );
    if inner.height < 2 {
        return;
    }

    // Thumbnail column on the right when there is room and the image is ready.
    let mut text_area = inner;
    if let Some(url) = &item.thumbnail
        && inner.width >= 46
        && inner.height >= 4
    {
        let key = MediaSpec::new(url.clone(), MediaKind::Image).key();
        if let Some(Loadable::Ready { value, .. }) = state.images.get(&key) {
            let image_width = 18u16.min(inner.width / 3).max(10);
            let image_area = Rect::new(
                inner.right().saturating_sub(image_width),
                inner.y,
                image_width,
                inner.height,
            );
            let frame_index = value.frame_at(Duration::from_millis(state.tick * 40)).0;
            let (iw, ih) = value
                .frames
                .first()
                .map(|f| (f.width(), f.height()))
                .unwrap_or((100, 75));
            let centered = crate::render::images::ImageRenderer::fit_centered(image_area, iw, ih);
            images.render_media(frame, &key, value, centered, frame_index);
            text_area = Rect::new(
                inner.x,
                inner.y,
                inner.width.saturating_sub(image_width + 2),
                inner.height,
            );
        }
    }

    let mut y = text_area.y;
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                section.kind.title().to_uppercase(),
                Style::default()
                    .fg(theme.secondary_accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if item.subtitle.is_empty() {
                    String::new()
                } else {
                    format!("   {}", item.subtitle)
                },
                Style::default().fg(theme.fg_dim),
            ),
        ])),
        Rect::new(text_area.x, y, text_area.width, 1),
    );
    y += 1;
    frame.render_widget(
        Paragraph::new(item.title.clone())
            .style(section_style(theme))
            .wrap(Wrap { trim: true }),
        Rect::new(
            text_area.x,
            y,
            text_area.width,
            2.min(text_area.bottom().saturating_sub(y)),
        ),
    );
    y += 2;
    if y < text_area.bottom() {
        frame.render_widget(
            Paragraph::new(item.extract.clone())
                .style(Style::default().fg(theme.fg_dim))
                .wrap(Wrap { trim: true }),
            Rect::new(
                text_area.x,
                y,
                text_area.width,
                text_area.bottom().saturating_sub(y),
            ),
        );
    }
}

fn draw_article(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    theme: AppTheme,
    images: &mut ImageRenderer,
    regions: &mut Vec<HitRegion>,
) {
    let Some(tab) = state.active_tab() else {
        empty_state(
            frame,
            area,
            theme,
            "No article open",
            "Search or pick a story from Home.",
        );
        return;
    };
    let layout = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Min(3),
    ])
    .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                if tab.bookmarked { "★ " } else { "  " },
                Style::default().fg(theme.warning),
            ),
            Span::styled(
                tab.title.clone(),
                Style::default()
                    .fg(theme.fg_bright)
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .wrap(Wrap { trim: true })
        .style(Style::default().bg(theme.bg)),
        layout[0],
    );
    let source = tab
        .article
        .source()
        .map(|source| match source {
            Source::Network => "online",
            Source::Cache => "cached",
            Source::Prefetch => "prefetched",
            Source::Local => "local",
        })
        .unwrap_or("…");
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                truncate(&tab.summary, layout[1].width.saturating_sub(18) as usize),
                Style::default().fg(theme.fg_dim),
            ),
            Span::styled(
                format!("  · {source}"),
                Style::default().fg(theme.secondary_accent),
            ),
        ]))
        .wrap(Wrap { trim: true })
        .style(Style::default().bg(theme.bg)),
        layout[1],
    );

    if draw_load_state(frame, state, layout[2], &tab.article, &tab.title) {
        return;
    }
    let Some(doc) = tab.doc.as_ref() else {
        empty_state(
            frame,
            layout[2],
            theme,
            "Laying out the article…",
            "Large pages are wrapped off the UI thread on the next frame.",
        );
        return;
    };
    let body = layout[2];
    let scroll = tab.scroll as usize;
    let visible = doc
        .lines
        .iter()
        .skip(scroll)
        .take(body.height as usize)
        .cloned()
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Text::from(visible)).style(Style::default().bg(theme.bg).fg(theme.fg)),
        body,
    );
    let x_offset = body.x.saturating_add(2);
    for link in doc.links.iter() {
        if link.line < scroll || link.line >= scroll + body.height as usize {
            continue;
        }
        let y = body.y + (link.line - scroll) as u16;
        let start = x_offset.saturating_add(link.start);
        let end = x_offset.saturating_add(link.end);
        if start >= body.right() {
            continue;
        }
        let width = end.saturating_sub(start).max(1);
        let underlined = state
            .hover
            .as_ref()
            .map(|hover| hover.target == link.target)
            .unwrap_or(false);
        if underlined {
            for x in start..(start + width).min(body.right()) {
                if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                    cell.set_style(
                        Style::default()
                            .fg(theme.selection_fg)
                            .bg(theme.selection_bg)
                            .add_modifier(Modifier::BOLD),
                    );
                }
            }
        }
        hit(
            regions,
            Rect::new(start, y, width.min(body.right().saturating_sub(start)), 1),
            HitAction::ArticleLink(link.target.clone()),
            2,
        );
    }
    let view_top = scroll;
    let view_bottom = scroll.saturating_add(body.height as usize);
    let images_on = state.images_enabled();
    for placement in doc.media.iter() {
        let p_top = placement.line;
        let p_bottom = placement.line.saturating_add(placement.height as usize);
        // Reveal/grow: render once the image top enters the viewport; visible
        // height increases as you scroll (responsive). Skip when top is above.
        if p_top >= view_bottom || p_bottom <= view_top || p_top < view_top {
            continue;
        }
        let full_w = body.width.saturating_sub(4);
        let full_area = Rect::new(
            body.x + 2,
            body.y + (p_top - view_top) as u16,
            full_w,
            placement.height,
        );
        let visible_h = p_bottom
            .min(view_bottom)
            .saturating_sub(p_top.max(view_top)) as u16;
        let visible_h = visible_h.min(body.bottom().saturating_sub(full_area.y));
        if visible_h < 2 || full_area.y >= body.bottom() {
            continue;
        }
        let draw_area = Rect::new(full_area.x, full_area.y, full_area.width, visible_h);
        hit(
            regions,
            draw_area,
            HitAction::ArticleMedia(placement.key.clone()),
            2,
        );

        if !images_on {
            continue;
        }

        let loaded = if placement.kind == MediaKind::Math {
            state.math_images.get(&placement.key)
        } else {
            state.images.get(&placement.key)
        };
        match loaded {
            Some(Loadable::Ready { value, .. }) => {
                let frame_index = value.frame_at(Duration::from_millis(state.tick * 40)).0;
                let (iw, ih) = value
                    .frames
                    .first()
                    .map(|f| (f.width(), f.height()))
                    .unwrap_or((100, 75));
                // Center within the stable full slot (current terminal width),
                // encode at a quantized stable size to avoid resize thrash, and
                // draw the visible/growing intersection (reveal behavior).
                let centered =
                    crate::render::images::ImageRenderer::fit_centered(full_area, iw, ih);
                let vis_draw = centered.intersection(body);
                if vis_draw.width >= 2 && vis_draw.height >= 2 {
                    // Quantize encode height (4-row steps) to limit cache churn
                    // while the image grows on reveal; stable once fully visible.
                    let step = 4u16;
                    let full_h = centered
                        .height
                        .min(body.bottom().saturating_sub(vis_draw.y));
                    let enc_h = if vis_draw.height >= full_h.saturating_sub(1) {
                        full_h.max(2)
                    } else {
                        vis_draw
                            .height
                            .div_ceil(step)
                            .saturating_mul(step)
                            .clamp(2, full_h.max(2))
                    };
                    let encode = Rect::new(vis_draw.x, vis_draw.y, vis_draw.width, enc_h);
                    let draw = Rect::new(
                        vis_draw.x,
                        vis_draw.y,
                        vis_draw.width,
                        vis_draw.height.min(enc_h),
                    );
                    images.render_media_reveal(
                        frame,
                        &placement.key,
                        value,
                        draw,
                        encode,
                        frame_index,
                    );
                }
            }
            Some(Loadable::Failed { error, .. }) => {
                frame.render_widget(
                    Paragraph::new(format!("⚠ {} · {}", placement.alt, error.message))
                        .style(Style::default().fg(theme.error).bg(theme.elevated_bg))
                        .wrap(Wrap { trim: true }),
                    draw_area,
                );
            }
            Some(Loadable::Loading { .. }) | Some(Loadable::Queued { .. }) => {
                frame.render_widget(
                    Paragraph::new(format!("{} rendering {}", spinner(state), placement.alt))
                        .alignment(Alignment::Center)
                        .style(Style::default().fg(theme.fg_dim).bg(theme.elevated_bg)),
                    draw_area,
                );
            }
            _ => {
                frame.render_widget(
                    Paragraph::new(format!("▧ {}", placement.alt))
                        .alignment(Alignment::Center)
                        .style(Style::default().fg(theme.fg_dim).bg(theme.elevated_bg)),
                    draw_area,
                );
            }
        }
    }
    // In-page search highlights: paint matches onto the rendered buffer after
    // the document has been drawn.
    if let Some(finder) = &state.finder {
        let top = scroll;
        let bottom = top + body.height as usize;
        let x_offset = body.x.saturating_add(2);
        let query = finder.input.text();
        for (index, line) in doc.lines.iter().enumerate() {
            if index < top || index >= bottom {
                continue;
            }
            if !finder.matches.contains(&index) {
                continue;
            }
            let y = body.y + (index - top) as u16;
            let active = finder.matches.get(finder.current) == Some(&index);
            let line_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            highlight_matches(
                frame.buffer_mut(),
                y,
                x_offset,
                &line_text,
                &query,
                active,
                theme,
                body,
            );
        }
        draw_finder_bar(frame, state, body, theme);
    }
    draw_scrollbar(frame, body, theme, tab.scroll, doc.total_height());
}

/// Paint every occurrence of the query on a line with the finder highlight.
fn highlight_matches(
    buffer: &mut ratatui::buffer::Buffer,
    line_y: u16,
    x_offset: u16,
    line_text: &str,
    query: &str,
    active: bool,
    theme: AppTheme,
    body: Rect,
) {
    if query.is_empty() || line_y < body.y || line_y >= body.bottom() {
        return;
    }
    let chars: Vec<char> = line_text.chars().collect();
    let query_chars: Vec<char> = query.to_lowercase().chars().collect();
    if chars.len() < query_chars.len() {
        return;
    }
    let style = if active {
        Style::default()
            .fg(theme.selection_fg)
            .bg(theme.selection_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.bg).bg(theme.warning)
    };
    let mut i = 0usize;
    while i + query_chars.len() <= chars.len() {
        let matched = chars[i..i + query_chars.len()]
            .iter()
            .enumerate()
            .all(|(j, ch)| ch.to_lowercase().next() == Some(query_chars[j]));
        if matched {
            // Compute the cell columns that cover chars[i..i+len).
            let mut used = 0u16;
            for (index, ch) in chars.iter().enumerate() {
                let width = unicode_width::UnicodeWidthChar::width(*ch).unwrap_or(0) as u16;
                if index >= i && index < i + query_chars.len() {
                    for dx in 0..width {
                        let x = x_offset + used + dx;
                        if x < body.right() {
                            if let Some(cell) = buffer.cell_mut((x, line_y)) {
                                cell.set_style(style);
                            }
                        }
                    }
                }
                used = used.saturating_add(width);
            }
            i += query_chars.len();
        } else {
            i += 1;
        }
    }
}

/// Floating in-page search bar at the top of the article body.
fn draw_finder_bar(frame: &mut Frame<'_>, state: &AppState, body: Rect, theme: AppTheme) {
    let Some(finder) = &state.finder else {
        return;
    };
    let width = 52u16
        .clamp(34, body.width.saturating_sub(2))
        .min(body.width);
    if width < 34 {
        return;
    }
    let height = 3;
    let bar = Rect::new(
        body.x + body.width.saturating_sub(width) / 2,
        body.y + 1,
        width,
        height,
    );
    frame.render_widget(Clear, bar);
    let query = truncate(
        &finder.input.display_with_cursor(),
        bar.width.saturating_sub(4) as usize,
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_active))
            .title(Span::styled(
                format!(
                    " find · {}/{} ",
                    if finder.matches.is_empty() {
                        0
                    } else {
                        finder.current + 1
                    },
                    finder.matches.len()
                ),
                Style::default()
                    .fg(theme.fg_bright)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Span::styled(
                " ↑↓/enter jump · esc close ",
                Style::default().fg(theme.fg_dim),
            ))
            .style(Style::default().bg(theme.picker_bg).fg(theme.fg)),
        bar,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {query}"),
            Style::default().fg(theme.fg_bright).bg(theme.elevated_bg),
        )))
        .style(Style::default().bg(theme.elevated_bg).fg(theme.fg)),
        Rect::new(bar.x + 2, bar.y + 1, bar.width.saturating_sub(4), 1),
    );
}

fn draw_scrollbar(frame: &mut Frame<'_>, area: Rect, theme: AppTheme, offset: u16, total: u16) {
    if total <= area.height || area.height < 3 {
        return;
    }
    let x = area.right().saturating_sub(1);
    let thumb = ((area.height as u32 * area.height as u32) / total as u32).max(1) as u16;
    let start = (area.height as u32 * offset as u32 / total as u32) as u16;
    for index in 0..area.height {
        let ch = if index >= start && index < start + thumb {
            "▐"
        } else {
            " "
        };
        frame.render_widget(
            Paragraph::new(ch).style(Style::default().fg(theme.muted_border).bg(theme.bg)),
            Rect::new(x, area.y + index, 1, 1),
        );
    }
}

fn draw_bookmarks(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    theme: AppTheme,
    regions: &mut Vec<HitRegion>,
) {
    if draw_load_state(frame, state, area, &state.bookmarks, "saved articles") {
        return;
    }
    let Some(items) = state.bookmarks.value() else {
        return;
    };
    if items.is_empty() {
        empty_state(
            frame,
            area,
            theme,
            "Nothing saved yet",
            "Open an article and press B (or click save) to keep it offline.",
        );
        return;
    }
    frame.render_widget(pane(theme).title(" Saved · offline forever "), area);
    let mut y = area.y + 1;
    for (index, item) in items.iter().enumerate() {
        if y >= area.bottom().saturating_sub(1) {
            break;
        }
        let selected = index == state.bookmark_index;
        let style = if selected {
            Style::default()
                .fg(theme.selection_fg)
                .bg(theme.selection_bg)
        } else {
            Style::default().fg(theme.fg).bg(theme.pane_bg)
        };
        let line = Line::from(vec![
            Span::styled(if selected { " ▸ " } else { "   " }, style.fg(theme.accent)),
            Span::styled(
                truncate(&item.title, area.width.saturating_sub(18) as usize),
                style.add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", relative_time(item.added_at)),
                Style::default().fg(theme.fg_dim).bg(if selected {
                    theme.selection_bg
                } else {
                    theme.pane_bg
                }),
            ),
        ]);
        frame.render_widget(Paragraph::new(line), Rect::new(area.x, y, area.width, 1));
        hit(
            regions,
            Rect::new(area.x, y, area.width, 1),
            HitAction::BookmarkRow(index),
            2,
        );
        y += 2;
    }
}

fn draw_history(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    theme: AppTheme,
    regions: &mut Vec<HitRegion>,
) {
    if draw_load_state(frame, state, area, &state.history, "reading history") {
        return;
    }
    let Some(items) = state.history.value() else {
        return;
    };
    if items.is_empty() {
        empty_state(
            frame,
            area,
            theme,
            "No reading history yet",
            "Articles you open stay in the local history for quick return.",
        );
        return;
    }
    frame.render_widget(pane(theme).title(" History · most recent first "), area);
    let mut y = area.y + 1;
    for (index, item) in items.iter().enumerate() {
        if y >= area.bottom().saturating_sub(1) {
            break;
        }
        let selected = index == state.history_index;
        let style = if selected {
            Style::default()
                .fg(theme.selection_fg)
                .bg(theme.selection_bg)
        } else {
            Style::default().fg(theme.fg).bg(theme.pane_bg)
        };
        let line = Line::from(vec![
            Span::styled(if selected { " ▸ " } else { "   " }, style.fg(theme.accent)),
            Span::styled(
                truncate(&item.title, area.width.saturating_sub(24) as usize),
                style,
            ),
            Span::styled(
                format!("  {}× {}", item.visits, relative_time(item.visited_at)),
                Style::default().fg(theme.fg_dim).bg(if selected {
                    theme.selection_bg
                } else {
                    theme.pane_bg
                }),
            ),
        ]);
        frame.render_widget(Paragraph::new(line), Rect::new(area.x, y, area.width, 1));
        hit(
            regions,
            Rect::new(area.x, y, area.width, 1),
            HitAction::HistoryRow(index),
            2,
        );
        y += 2;
    }
}

fn draw_lists(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    theme: AppTheme,
    regions: &mut Vec<HitRegion>,
) {
    if draw_load_state(frame, state, area, &state.lists, "reading lists") {
        return;
    }
    let Some(lists) = state.lists.value() else {
        return;
    };
    frame.render_widget(
        pane(theme).title(" Reading lists · n new · D delete "),
        area,
    );
    if lists.is_empty() {
        empty_state(
            frame,
            area,
            theme,
            "Build your first reading list",
            "Press n or click the new-list button in search (ctrl+n).",
        );
        return;
    }
    let mut y = area.y + 1;
    for (index, list) in lists.iter().enumerate() {
        if y >= area.bottom().saturating_sub(1) {
            break;
        }
        let selected = index == state.list_index;
        let style = if selected {
            Style::default()
                .fg(theme.selection_fg)
                .bg(theme.selection_bg)
        } else {
            Style::default().fg(theme.fg).bg(theme.pane_bg)
        };
        let name_width = area.width.saturating_sub(18) as usize;
        let line = Line::from(vec![
            Span::styled(if selected { " ▸ " } else { "   " }, style.fg(theme.accent)),
            Span::styled(
                truncate(&list.name, name_width),
                style.add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {} items", list.item_count),
                Style::default().fg(theme.fg_dim).bg(if selected {
                    theme.selection_bg
                } else {
                    theme.pane_bg
                }),
            ),
            Span::styled(
                "   [delete]",
                Style::default().fg(theme.error).bg(if selected {
                    theme.selection_bg
                } else {
                    theme.pane_bg
                }),
            ),
        ]);
        frame.render_widget(Paragraph::new(line), Rect::new(area.x, y, area.width, 1));
        hit(
            regions,
            Rect::new(area.x, y, area.width.saturating_sub(10), 1),
            HitAction::ListOpen(list.id),
            2,
        );
        hit(
            regions,
            Rect::new(area.x, y, area.width, 1),
            HitAction::ListRow(index),
            1,
        );
        hit(
            regions,
            Rect::new(area.right().saturating_sub(10), y, 10, 1),
            HitAction::ListDelete(list.id),
            3,
        );
        y += 2;
    }
}

fn draw_list_detail(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    theme: AppTheme,
    regions: &mut Vec<HitRegion>,
) {
    frame.render_widget(
        pane(theme).title(format!(
            " {} · d remove · c lists ",
            state.active_list_name()
        )),
        area,
    );
    if draw_load_state(frame, state, area, &state.list_items, "reading list") {
        return;
    }
    let Some(items) = state.list_items.value() else {
        return;
    };
    if items.is_empty() {
        empty_state(
            frame,
            area,
            theme,
            "This list is empty",
            "Open an article and use a (add to list) to put it here.",
        );
        return;
    }
    let mut y = area.y + 1;
    for (index, title) in items.iter().enumerate() {
        if y >= area.bottom().saturating_sub(1) {
            break;
        }
        let selected = index == state.list_item_index;
        let style = if selected {
            Style::default()
                .fg(theme.selection_fg)
                .bg(theme.selection_bg)
        } else {
            Style::default().fg(theme.fg).bg(theme.pane_bg)
        };
        let line = Line::from(vec![
            Span::styled(if selected { " ▸ " } else { "   " }, style.fg(theme.accent)),
            Span::styled(
                truncate(title, area.width.saturating_sub(10) as usize),
                style,
            ),
            Span::styled(
                "  [remove]",
                Style::default().fg(theme.error).bg(if selected {
                    theme.selection_bg
                } else {
                    theme.pane_bg
                }),
            ),
        ]);
        frame.render_widget(Paragraph::new(line), Rect::new(area.x, y, area.width, 1));
        hit(
            regions,
            Rect::new(area.x, y, area.width.saturating_sub(10), 1),
            HitAction::ListItemRow(index),
            2,
        );
        y += 2;
    }
}

fn draw_media_lab(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    theme: AppTheme,
    images: &mut ImageRenderer,
    regions: &mut Vec<HitRegion>,
) {
    let split = Layout::horizontal([Constraint::Length(31), Constraint::Min(20)]).split(area);
    frame.render_widget(pane(theme).title(" Media lab "), split[0]);
    let mut y = split[0].y + 1;
    for (index, item) in state.media_lab.items.iter().enumerate() {
        if y >= split[0].bottom().saturating_sub(1) {
            break;
        }
        let selected = index == state.media_lab.selected;
        let style = if selected {
            Style::default()
                .fg(theme.selection_fg)
                .bg(theme.selection_bg)
        } else {
            Style::default().fg(theme.fg).bg(theme.pane_bg)
        };
        let icon = match item.kind {
            crate::core::DemoKind::Image => "▧",
            crate::core::DemoKind::Gif => "◌",
            crate::core::DemoKind::Svg => "◇",
            crate::core::DemoKind::Code => "⌘",
            crate::core::DemoKind::Math => "∑",
            crate::core::DemoKind::Audio => "♪",
            crate::core::DemoKind::Video => "▶",
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(if selected { " ▸ " } else { "   " }, style.fg(theme.accent)),
                Span::styled(format!("{icon} {}", item.label), style),
            ])),
            Rect::new(split[0].x, y, split[0].width, 1),
        );
        hit(
            regions,
            Rect::new(split[0].x, y, split[0].width, 1),
            HitAction::MediaLabItem(index),
            2,
        );
        y += 2;
    }
    let Some(item) = state.media_lab.items.get(state.media_lab.selected) else {
        return;
    };
    frame.render_widget(pane(theme).title(format!(" {} ", item.label)), split[1]);
    let inner = Rect::new(
        split[1].x + 2,
        split[1].y + 1,
        split[1].width.saturating_sub(4),
        split[1].height.saturating_sub(2),
    );
    match item.kind {
        crate::core::DemoKind::Code => {
            let lines = crate::render::syntax::highlight(
                state.media_lab_code(),
                Some("rust"),
                theme,
                state.config.render.syntax_highlight,
                inner.width as usize,
            );
            frame.render_widget(Paragraph::new(lines), inner);
        }
        crate::core::DemoKind::Math => {
            if let Some(tex) = item.text {
                let key = crate::core::state::math_key(tex, state.is_light());
                draw_loaded_media(frame, state, images, inner, theme, &key, true);
            }
        }
        _ => {
            if let Some(key) = state.media_lab_key() {
                if item.kind == crate::core::DemoKind::Video {
                    if state.video.key.as_deref() == Some(key.as_str()) {
                        draw_video(frame, state, images, inner, theme);
                    } else {
                        draw_loaded_media(frame, state, images, inner, theme, &key, false);
                    }
                } else if item.kind == crate::core::DemoKind::Audio {
                    draw_audio(frame, state, inner, theme);
                } else {
                    draw_loaded_media(frame, state, images, inner, theme, &key, false);
                }
            }
        }
    }
}

fn draw_loaded_media(
    frame: &mut Frame<'_>,
    state: &AppState,
    images: &mut ImageRenderer,
    area: Rect,
    theme: AppTheme,
    key: &str,
    math: bool,
) {
    let loaded = if math {
        state.math_images.get(key)
    } else {
        state.images.get(key)
    };
    match loaded {
        Some(Loadable::Ready { value, .. }) => {
            let index = value.frame_at(Duration::from_millis(state.tick * 40)).0;
            let (iw, ih) = value
                .frames
                .first()
                .map(|f| (f.width(), f.height()))
                .unwrap_or((100, 75));
            let centered = crate::render::images::ImageRenderer::fit_centered(area, iw, ih);
            images.render_media(frame, key, value, centered, index);
        }
        Some(Loadable::Failed { error, .. }) => {
            frame.render_widget(
                Paragraph::new(format!(
                    "⚠ {}\n\n{}",
                    error.message,
                    error.hint().unwrap_or("")
                ))
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(theme.error).bg(theme.pane_bg)),
                area,
            );
        }
        Some(Loadable::Loading { .. }) | Some(Loadable::Queued { .. }) => {
            frame.render_widget(
                Paragraph::new(format!("{} loading media…", spinner(state)))
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(theme.fg_dim).bg(theme.elevated_bg)),
                area,
            );
        }
        _ => {
            frame.render_widget(
                Paragraph::new("Press enter to load this preview.")
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(theme.fg_dim).bg(theme.pane_bg)),
                area,
            );
        }
    }
}

fn draw_video(
    frame: &mut Frame<'_>,
    state: &AppState,
    images: &mut ImageRenderer,
    area: Rect,
    theme: AppTheme,
) {
    if let Some(frame_image) = &state.video.frame {
        images.render_rgba(frame, "video-preview", frame_image, area);
    } else {
        frame.render_widget(
            Paragraph::new(format!(
                "{} decoding {}…",
                spinner(state),
                if state.video.title.is_empty() {
                    "video frames"
                } else {
                    &state.video.title
                }
            ))
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.fg_dim).bg(theme.elevated_bg)),
            area,
        );
    }
    if area.height > 2 {
        let footer = format!(
            "{}  {}  ·  space play/pause",
            if state.video.playing { "▶" } else { "❚❚" },
            state.video.position_label()
        );
        frame.render_widget(
            Paragraph::new(footer).style(Style::default().fg(theme.secondary_accent).bg(theme.bg)),
            Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
        );
    }
}

fn draw_audio(frame: &mut Frame<'_>, state: &AppState, area: Rect, theme: AppTheme) {
    if let Some(error) = &state.audio.error {
        frame.render_widget(
            Paragraph::new(format!("⚠ audio\n\n{error}"))
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(theme.error).bg(theme.pane_bg)),
            area,
        );
        return;
    }
    let Some(data) = &state.audio.data else {
        let label = if state.audio.status == crate::media::AudioStatus::Loading {
            format!(
                "{} decoding {}…",
                spinner(state),
                if state.audio.title.is_empty() {
                    "audio"
                } else {
                    &state.audio.title
                }
            )
        } else {
            "Press enter to load audio.".to_string()
        };
        frame.render_widget(
            Paragraph::new(label)
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.fg_dim)),
            area,
        );
        return;
    };
    let wave_height = area.height.saturating_sub(4).clamp(2, 12);
    draw_waveform(frame, area, theme, data, state.audio.position, wave_height);
    frame.render_widget(
        Paragraph::new(format!(
            " {}  {}  ·  space play/pause   +/− volume {:.0}%",
            if state.audio.status == crate::media::AudioStatus::Playing {
                "▶"
            } else {
                "❚❚"
            },
            state.audio.position_label(),
            state.audio.volume * 100.0
        ))
        .style(Style::default().fg(theme.secondary_accent).bg(theme.bg)),
        Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
    );
}

fn draw_waveform(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: AppTheme,
    data: &crate::media::AudioData,
    position: f64,
    height: u16,
) {
    let bars = area.width.min(72) as usize;
    if bars == 0 || data.samples.is_empty() {
        return;
    }
    let channels = data.channels.max(1) as usize;
    let start = (position * data.sample_rate as f64) as usize * channels;
    let window = (data.samples.len() / bars).max(channels);
    let levels = (0..bars)
        .map(|bar| {
            let begin = start
                .saturating_add(bar.saturating_mul(window))
                .min(data.samples.len());
            let end = (begin + window).min(data.samples.len());
            if begin >= end {
                return 0.0;
            }
            let peak = data.samples[begin..end]
                .iter()
                .map(|sample| sample.abs())
                .fold(0.0f32, f32::max);
            peak.clamp(0.0, 1.0)
        })
        .collect::<Vec<_>>();
    let glyphs = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    for bar in 0..bars {
        let level = (levels[bar] * height as f32).ceil() as u16;
        for row in 0..height {
            let filled = row < level;
            let ch = if filled {
                glyphs[((levels[bar] * 7.0).round() as usize).min(7)]
            } else {
                " "
            };
            frame.render_widget(
                Paragraph::new(ch).style(Style::default().fg(theme.accent).bg(theme.bg)),
                Rect::new(
                    area.x + bar as u16,
                    area.y + height.saturating_sub(row + 1),
                    1,
                    1,
                ),
            );
        }
    }
}

fn draw_picker_overlay(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    theme: AppTheme,
    images: &mut ImageRenderer,
    regions: &mut Vec<HitRegion>,
) {
    let Some(picker) = state.picker.as_ref() else {
        return;
    };
    let element_mode = picker.mode == PickerMode::ArticleElements;
    let theme_mode = picker.mode == PickerMode::Theme;
    let width = if element_mode {
        area.width
            .saturating_sub(4)
            .clamp(72.min(area.width.saturating_sub(4)), 128)
    } else if theme_mode {
        (area.width.saturating_mul(3) / 4)
            .clamp(48, 96)
            .min(area.width.saturating_sub(4))
    } else {
        (area.width.saturating_mul(4) / 5)
            .clamp(42, 88)
            .min(area.width.saturating_sub(4))
    };
    let height = (area.height.saturating_mul(3) / 4)
        .clamp(14, 26)
        .min(area.height.saturating_sub(4));
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);

    // Borderless panel (gtm-style): title + "Esc" hint, padded, no border box.
    let esc_label = "  Esc  ";
    let block = Block::default()
        .title(Line::from(Span::styled(
            format!(" {} ", picker.mode.title()),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )))
        .title(
            Line::from(Span::styled(esc_label, Style::default().fg(theme.fg_dim))).right_aligned(),
        )
        .padding(ratatui::widgets::Padding {
            left: 1,
            right: 1,
            top: 1,
            bottom: 1,
        })
        .style(Style::default().bg(theme.picker_bg).fg(theme.fg));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.width < 10 || inner.height < 4 {
        return;
    }

    // Search/filter line.
    let input_y = inner.y;
    let cursor = picker.input.display_with_cursor();
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" > ", Style::default().fg(theme.fg_dim)),
            Span::styled(cursor, Style::default().fg(theme.fg_bright)),
        ]))
        .style(Style::default().bg(theme.picker_bg)),
        Rect::new(inner.x, input_y, inner.width, 1),
    );
    hit(
        regions,
        Rect::new(popup.right().saturating_sub(7), popup.y, 6, 1),
        HitAction::PickerClose,
        11,
    );

    let mut content_top = inner.y + 1;

    // Article outline: filter chips, then list on top + queue-style preview strip below.
    if element_mode {
        let mut spans = vec![Span::styled(" ", Style::default().bg(theme.picker_bg))];
        let mut x = inner.x + 1;
        for filter in ElementFilter::ALL {
            let active = filter == picker.element_filter;
            let label = format!("{} {} ", filter.icon(), filter.label());
            let w = label.width() as u16;
            spans.push(Span::styled(
                label,
                if active {
                    Style::default()
                        .fg(theme.selection_fg)
                        .bg(theme.selection_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.fg_dim).bg(theme.picker_bg)
                },
            ));
            spans.push(Span::styled(" ", Style::default().bg(theme.picker_bg)));
            if x + w <= inner.right() {
                hit(
                    regions,
                    Rect::new(x, content_top, w, 1),
                    HitAction::PickerElementFilter(filter),
                    13,
                );
            }
            x += w + 1;
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.picker_bg)),
            Rect::new(inner.x, content_top, inner.width, 1),
        );
        content_top += 1;

        let preview_h: u16 = if inner.height >= 14 { 8 } else { 6 };
        let list_h = inner
            .bottom()
            .saturating_sub(content_top)
            .saturating_sub(preview_h);
        let list_area = Rect::new(inner.x, content_top, inner.width, list_h);
        let preview_area = Rect::new(
            inner.x,
            content_top.saturating_add(list_h),
            inner.width,
            preview_h.min(
                inner
                    .bottom()
                    .saturating_sub(content_top.saturating_add(list_h)),
            ),
        );
        draw_picker_rows(frame, state, picker, list_area, theme, regions, false);
        if preview_area.height >= 2 {
            draw_element_preview(frame, state, preview_area, theme, images);
        }
        return;
    }

    // Saved/Offline merged picker: category chips (`[`/`]` toggle).
    if picker.mode == PickerMode::Saved {
        let mut spans = vec![Span::styled(" ", Style::default().bg(theme.picker_bg))];
        for filter in SavedFilter::ALL {
            let active = filter == picker.saved_filter;
            let label = format!("{} {} ", filter.icon(), filter.label());
            spans.push(Span::styled(
                label,
                if active {
                    Style::default()
                        .fg(theme.selection_fg)
                        .bg(theme.selection_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.fg_dim).bg(theme.picker_bg)
                },
            ));
            spans.push(Span::styled(" ", Style::default().bg(theme.picker_bg)));
        }
        spans.push(Span::styled(
            "  [ ] switch",
            Style::default().fg(theme.fg_dim).bg(theme.picker_bg),
        ));
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.picker_bg)),
            Rect::new(inner.x, content_top, inner.width, 1),
        );
        content_top += 1;
    }

    // Theme picker: gtm-style rows with swatches (2 lines per item).
    if theme_mode {
        let list_area = Rect::new(
            inner.x,
            content_top,
            inner.width,
            inner.bottom().saturating_sub(content_top),
        );
        draw_theme_picker_rows(frame, state, picker, list_area, theme, regions);
        return;
    }

    // Generic picker list.
    let list_area = Rect::new(
        inner.x,
        content_top,
        inner.width,
        inner.bottom().saturating_sub(content_top),
    );
    draw_picker_rows(frame, state, picker, list_area, theme, regions, true);

    if picker.loading {
        frame.render_widget(
            Paragraph::new(format!("{} searching Wikipedia…", spinner(state)))
                .alignment(Alignment::Center)
                .style(
                    Style::default()
                        .fg(theme.secondary_accent)
                        .bg(theme.picker_bg),
                ),
            Rect::new(inner.x, popup.bottom().saturating_sub(2), inner.width, 1),
        );
    } else if let Some(error) = &picker.error {
        frame.render_widget(
            Paragraph::new(format!("⚠ {}", truncate(error, inner.width as usize)))
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.error).bg(theme.picker_bg)),
            Rect::new(inner.x, popup.bottom().saturating_sub(2), inner.width, 1),
        );
    }
    if picker.mode == PickerMode::Lists || picker.mode == PickerMode::AddToList {
        let footer = Rect::new(inner.x, popup.bottom().saturating_sub(2), inner.width, 1);
        frame.render_widget(
            Paragraph::new(" ctrl+n new reading list ")
                .alignment(Alignment::Right)
                .style(
                    Style::default()
                        .fg(theme.secondary_accent)
                        .bg(theme.picker_bg),
                ),
            footer,
        );
        let width = 26.min(footer.width);
        hit(
            regions,
            Rect::new(footer.right().saturating_sub(width), footer.y, width, 1),
            HitAction::PickerNewList,
            22,
        );
    }
}

fn draw_picker_rows(
    frame: &mut Frame<'_>,
    _state: &AppState,
    picker: &crate::core::state::Picker,
    list_area: Rect,
    theme: AppTheme,
    regions: &mut Vec<HitRegion>,
    _show_hints: bool,
) {
    if list_area.height < 1 {
        return;
    }
    let visible = list_area.height as usize;
    let start = if picker.selected >= visible {
        picker.selected + 1 - visible
    } else {
        0
    };
    for (row, (index, entry)) in picker.entries.iter().enumerate().skip(start).enumerate() {
        let y = list_area.y.saturating_add(row as u16);
        if y >= list_area.bottom() {
            break;
        }
        let selected = index == picker.selected;
        let bg = if selected {
            theme.selection_bg
        } else {
            theme.picker_bg
        };
        let fg = if selected {
            theme.selection_fg
        } else {
            theme.fg
        };
        let prefix = if selected { " > " } else { "   " };
        let hint = if entry.hint.is_empty() {
            String::new()
        } else {
            format!(
                "  {}",
                truncate(&entry.hint, list_area.width.saturating_sub(24) as usize)
            )
        };
        let badge = if entry.local { "  cached" } else { "" };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(theme.accent).bg(bg)),
                Span::styled(
                    truncate(&entry.label, list_area.width.saturating_sub(12) as usize),
                    Style::default().fg(fg).bg(bg).add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
                ),
                Span::styled(badge, Style::default().fg(theme.success).bg(bg)),
                Span::styled(hint, Style::default().fg(theme.fg_dim).bg(bg)),
            ]))
            .style(Style::default().bg(bg)),
            Rect::new(list_area.x, y, list_area.width, 1),
        );
        hit(
            regions,
            Rect::new(list_area.x, y, list_area.width, 1),
            HitAction::PickerEntry(index),
            12,
        );
    }
}

fn draw_theme_picker_rows(
    frame: &mut Frame<'_>,
    state: &AppState,
    picker: &crate::core::state::Picker,
    list_area: Rect,
    theme: AppTheme,
    regions: &mut Vec<HitRegion>,
) {
    if list_area.height < 2 {
        return;
    }
    // gtm-style: 2 lines per item (row + blank separator for swatch clarity).
    let item_h: u16 = 2;
    let visible = (list_area.height / item_h) as usize;
    let start = if picker.selected >= visible && visible > 0 {
        picker.selected + 1 - visible
    } else {
        0
    };
    let active_name = state.themes.get(state.theme_idx).map(|t| t.name.as_str());

    for (row, (index, entry)) in picker.entries.iter().enumerate().skip(start).enumerate() {
        if row >= visible {
            break;
        }
        let y = list_area.y.saturating_add(row as u16 * item_h);
        if y + 1 > list_area.bottom() {
            break;
        }
        let is_selected = index == picker.selected;
        let is_active = active_name == Some(entry.label.as_str());
        let prefix = if is_selected { " > " } else { "   " };
        let check = if is_active { " \u{2713}" } else { "" };
        let light_badge = if entry.hint.eq_ignore_ascii_case("light") {
            " \u{2600}"
        } else {
            ""
        };
        let name_part = format!("{prefix}{}{light_badge}", entry.label);

        let row_w = list_area.width as usize;
        // Look up palette for swatches.
        let pal = state
            .themes
            .iter()
            .find(|t| t.name == entry.label)
            .map(|t| t.theme)
            .unwrap_or(theme);
        let colors = [
            pal.bg,
            pal.fg,
            pal.accent,
            pal.secondary_accent,
            pal.tertiary_accent,
            pal.border,
        ];
        let swatch_run_w = colors.len() * 3; // "  " + " " per color
        let left_w = name_part.chars().count() + check.chars().count() + 1;
        let pad = row_w.saturating_sub(left_w + swatch_run_w + 1);

        let name_style = if is_selected {
            Style::default()
                .fg(theme.selection_fg)
                .bg(theme.selection_bg)
                .add_modifier(Modifier::BOLD)
        } else if is_active {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.fg)
        };
        let mut spans: Vec<Span> = vec![Span::styled(
            format!("{name_part}{:pad$}", "", pad = pad),
            name_style,
        )];
        for c in colors {
            spans.push(Span::styled("  ", Style::default().bg(c)));
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            check,
            Style::default().fg(if is_selected {
                theme.selection_fg
            } else {
                theme.success
            }),
        ));

        let row_bg = if is_selected {
            theme.selection_bg
        } else {
            theme.picker_bg
        };
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(row_bg)),
            Rect::new(list_area.x, y, list_area.width, 1),
        );
        // Blank separator line.
        frame.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.picker_bg)),
            Rect::new(list_area.x, y + 1, list_area.width, 1),
        );
        hit(
            regions,
            Rect::new(list_area.x, y, list_area.width, item_h),
            HitAction::PickerEntry(index),
            12,
        );
    }
}

fn draw_element_preview(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    theme: AppTheme,
    images: &mut ImageRenderer,
) {
    use crate::core::PickerAction;
    // Queue-style preview strip (gtm): top border only, accent, section bg.
    let block = Block::default()
        .borders(Borders::TOP)
        .title(" Preview ")
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.elevated_bg));
    frame.render_widget(block, area);
    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(1),
    );
    if inner.width < 6 || inner.height < 2 {
        return;
    }
    let Some((filter, entry)) = state.selected_element() else {
        return;
    };

    let scroll_line = match &entry.action {
        PickerAction::ScrollToLine(line) => Some(*line as usize),
        _ => None,
    };
    let doc = state.active_tab().and_then(|tab| tab.doc.as_ref());
    let placement_at = |kinds: &[MediaKind]| {
        scroll_line.and_then(|line| {
            doc.and_then(|doc| {
                doc.media
                    .iter()
                    .find(|p| p.line == line && kinds.contains(&p.kind))
                    .cloned()
            })
        })
    };
    let code_at = scroll_line.and_then(|line| {
        doc.and_then(|doc| doc.code.iter().find(|c| c.line as usize == line).cloned())
    });

    let mut y = inner.y;
    let heading = match filter {
        ElementFilter::RelatedLinks => "related link",
        ElementFilter::Headings => "heading",
        ElementFilter::Images => "image",
        ElementFilter::Code => "code",
        ElementFilter::Audio => "audio",
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {}", filter.icon()),
                Style::default()
                    .fg(theme.tertiary_accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {heading}"),
                Style::default()
                    .fg(theme.secondary_accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        Rect::new(inner.x, y, inner.width, 1),
    );
    y += 2;

    match filter {
        ElementFilter::RelatedLinks => {
            if let PickerAction::OpenLink(target) = &entry.action {
                if y < inner.bottom() {
                    frame.render_widget(
                        Paragraph::new(target.label.clone())
                            .wrap(Wrap { trim: true })
                            .style(
                                Style::default()
                                    .fg(theme.fg_bright)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        Rect::new(
                            inner.x,
                            y,
                            inner.width,
                            2.min(inner.bottom().saturating_sub(y)),
                        ),
                    );
                    y += 2;
                }
                if target.kind != LinkKind::Article {
                    if y < inner.bottom() {
                        frame.render_widget(
                            Paragraph::new(format!(
                                "{} · press enter to open",
                                link_kind_label(target.kind)
                            ))
                            .wrap(Wrap { trim: true })
                            .style(Style::default().fg(theme.fg_dim)),
                            Rect::new(inner.x, y, inner.width, inner.bottom().saturating_sub(y)),
                        );
                    }
                    return;
                }
                if state.picker_preview.loading && state.picker_preview.summary.is_none() {
                    frame.render_widget(
                        Paragraph::new(format!("{} fetching summary…", spinner(state)))
                            .alignment(Alignment::Center)
                            .style(Style::default().fg(theme.fg_dim)),
                        Rect::new(inner.x, y, inner.width, inner.bottom().saturating_sub(y)),
                    );
                    return;
                }
                if let Some(error) = &state.picker_preview.error {
                    frame.render_widget(
                        Paragraph::new(format!("⚠ {error}"))
                            .wrap(Wrap { trim: true })
                            .style(Style::default().fg(theme.warning)),
                        Rect::new(
                            inner.x,
                            y,
                            inner.width,
                            2.min(inner.bottom().saturating_sub(y)),
                        ),
                    );
                    y += 2;
                }
                // Summary image, if ready.
                if let Some(url) = &state.picker_preview.image {
                    let key = MediaSpec::new(url.clone(), MediaKind::Image).key();
                    let image_height = (inner.bottom().saturating_sub(y + 2)).min(6);
                    if image_height >= 2
                        && let Some(Loadable::Ready { value, .. }) = state.images.get(&key)
                    {
                        let frame_index = value.frame_at(Duration::from_millis(state.tick * 40)).0;
                        let area = Rect::new(inner.x, y, inner.width, image_height);
                        let (iw, ih) = value
                            .frames
                            .first()
                            .map(|f| (f.width(), f.height()))
                            .unwrap_or((100, 75));
                        let centered =
                            crate::render::images::ImageRenderer::fit_centered(area, iw, ih);
                        images.render_media(frame, &key, value, centered, frame_index);
                        y += image_height + 1;
                    }
                }
                if let Some(summary) = &state.picker_preview.summary {
                    if y < inner.bottom() {
                        frame.render_widget(
                            Paragraph::new(summary.clone())
                                .wrap(Wrap { trim: true })
                                .style(Style::default().fg(theme.fg_dim)),
                            Rect::new(inner.x, y, inner.width, inner.bottom().saturating_sub(y)),
                        );
                    }
                } else if y < inner.bottom() {
                    frame.render_widget(
                        Paragraph::new("no summary available")
                            .style(Style::default().fg(theme.muted_border)),
                        Rect::new(inner.x, y, inner.width, 1),
                    );
                }
            }
        }
        ElementFilter::Images => {
            let Some(placement) = placement_at(&[MediaKind::Image, MediaKind::Gif, MediaKind::Svg])
            else {
                frame.render_widget(
                    Paragraph::new("loading image…").style(Style::default().fg(theme.fg_dim)),
                    Rect::new(inner.x, y, inner.width, 1),
                );
                return;
            };
            if !placement.alt.is_empty() {
                frame.render_widget(
                    Paragraph::new(placement.alt.clone())
                        .wrap(Wrap { trim: true })
                        .style(Style::default().fg(theme.fg)),
                    Rect::new(
                        inner.x,
                        y,
                        inner.width,
                        2.min(inner.bottom().saturating_sub(y)),
                    ),
                );
                y += 2;
            }
            let key = placement.key.clone();
            let image_height = inner.bottom().saturating_sub(y);
            if image_height >= 2
                && let Some(Loadable::Ready { value, .. }) = state.images.get(&key)
            {
                let frame_index = value.frame_at(Duration::from_millis(state.tick * 40)).0;
                let area = Rect::new(inner.x, y, inner.width, image_height);
                let (iw, ih) = value
                    .frames
                    .first()
                    .map(|f| (f.width(), f.height()))
                    .unwrap_or((100, 75));
                let centered = crate::render::images::ImageRenderer::fit_centered(area, iw, ih);
                images.render_media(frame, &key, value, centered, frame_index);
            } else if image_height >= 1 {
                frame.render_widget(
                    Paragraph::new(format!("{} rendering image…", spinner(state)))
                        .style(Style::default().fg(theme.fg_dim)),
                    Rect::new(inner.x, y, inner.width, 1),
                );
            }
        }
        ElementFilter::Code => {
            let Some(code) = code_at else {
                frame.render_widget(
                    Paragraph::new("laying out code…").style(Style::default().fg(theme.fg_dim)),
                    Rect::new(inner.x, y, inner.width, 1),
                );
                return;
            };
            let language = code.language.as_deref().unwrap_or("code").to_string();
            frame.render_widget(
                Paragraph::new(format!("{} · {} lines", language, code.lines)).style(
                    Style::default()
                        .fg(theme.tertiary_accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Rect::new(inner.x, y, inner.width, 1),
            );
            y += 2;
            let lines = crate::render::syntax::highlight(
                &code.text,
                code.language.as_deref(),
                theme,
                state.config.render.syntax_highlight,
                inner.width.saturating_sub(2) as usize,
            );
            let visible = inner.bottom().saturating_sub(y) as usize;
            frame.render_widget(
                Paragraph::new(lines.into_iter().take(visible).collect::<Vec<_>>())
                    .style(Style::default().fg(theme.fg).bg(theme.elevated_bg)),
                Rect::new(inner.x, y, inner.width, inner.bottom().saturating_sub(y)),
            );
        }
        ElementFilter::Audio => {
            let Some(placement) = placement_at(&[MediaKind::Audio, MediaKind::Video]) else {
                frame.render_widget(
                    Paragraph::new("loading audio…").style(Style::default().fg(theme.fg_dim)),
                    Rect::new(inner.x, y, inner.width, 1),
                );
                return;
            };
            let (_, url) = parse_media_key_ui(&placement.key);
            let key = placement.key.clone();
            let playing = state.audio.key.as_deref() == Some(key.as_str())
                && state.audio.status == crate::media::AudioStatus::Playing;
            let title = if placement.alt.is_empty() {
                url.clone()
            } else {
                placement.alt.clone()
            };
            frame.render_widget(
                Paragraph::new(format!("{} {}", if playing { "▶" } else { "♪" }, title))
                    .wrap(Wrap { trim: true })
                    .style(
                        Style::default()
                            .fg(theme.fg_bright)
                            .add_modifier(Modifier::BOLD),
                    ),
                Rect::new(
                    inner.x,
                    y,
                    inner.width,
                    2.min(inner.bottom().saturating_sub(y)),
                ),
            );
            y += 2;
            if y < inner.bottom() {
                let status_line = if playing {
                    format!(
                        "playing · {}/{} · p to pause",
                        position_label(state.audio.position),
                        position_label(state.audio.duration)
                    )
                } else if state.audio.key.as_deref() == Some(key.as_str())
                    && state.audio.data.is_some()
                {
                    "paused · p to play".to_string()
                } else if state.audio.data.is_none()
                    && state.audio.key.as_deref() == Some(key.as_str())
                {
                    format!("{} decoding…", spinner(state))
                } else {
                    "press p to play".to_string()
                };
                frame.render_widget(
                    Paragraph::new(status_line).style(Style::default().fg(theme.fg_dim)),
                    Rect::new(inner.x, y, inner.width, 1),
                );
                y += 1;
            }
            if y < inner.bottom() {
                frame.render_widget(
                    Paragraph::new("enter opens · p toggles playback")
                        .style(Style::default().fg(theme.muted_border)),
                    Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
                );
            }
        }
        ElementFilter::Headings => {
            let line = scroll_line.unwrap_or(0);
            frame.render_widget(
                Paragraph::new(format!("line {}", line + 1))
                    .style(Style::default().fg(theme.fg_dim)),
                Rect::new(inner.x, y, inner.width, 1),
            );
        }
    }
}

fn parse_media_key_ui(key: &str) -> (MediaKind, String) {
    for (prefix, kind) in [
        ("image:", MediaKind::Image),
        ("animation:", MediaKind::Gif),
        ("vector:", MediaKind::Svg),
        ("audio:", MediaKind::Audio),
        ("video:", MediaKind::Video),
        ("math:", MediaKind::Math),
        ("document:", MediaKind::Document),
    ] {
        if let Some(rest) = key.strip_prefix(prefix) {
            return (kind, rest.to_string());
        }
    }
    (MediaKind::Image, key.to_string())
}

fn position_label(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

fn draw_overlay(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    theme: AppTheme,
    images: &mut ImageRenderer,
    regions: &mut Vec<HitRegion>,
) {
    use crate::core::Overlay;
    match state.overlay {
        Overlay::Help => {
            let popup = centered(
                area,
                area.width.saturating_sub(8).min(92),
                area.height.saturating_sub(4),
            );
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border_active))
                    .title(" Help · gtw ")
                    .style(Style::default().bg(theme.picker_bg).fg(theme.fg)),
                popup,
            );
            let inner = Rect::new(
                popup.x + 2,
                popup.y + 1,
                popup.width.saturating_sub(4),
                popup.height.saturating_sub(2),
            );
            let lines = help_lines(theme);
            let visible = inner.height as usize;
            let start = (state.help_scroll as usize).min(lines.len().saturating_sub(visible));
            let slice = lines
                .into_iter()
                .skip(start)
                .take(visible)
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(slice).style(Style::default().bg(theme.picker_bg)),
                inner,
            );
            hit(regions, popup, HitAction::HelpClose, 20);
        }
        Overlay::Activity => {
            let popup = centered(
                area,
                68.min(area.width.saturating_sub(4)),
                18.min(area.height.saturating_sub(4)),
            );
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border_active))
                    .title(" Activity ")
                    .style(Style::default().bg(theme.picker_bg).fg(theme.fg)),
                popup,
            );
            let inner = Rect::new(
                popup.x + 2,
                popup.y + 1,
                popup.width.saturating_sub(4),
                popup.height.saturating_sub(2),
            );
            if state.activity.is_empty() {
                frame.render_widget(
                    Paragraph::new("Idle. Network, decoding and cache work appears here.")
                        .wrap(Wrap { trim: true })
                        .style(Style::default().fg(theme.fg_dim)),
                    inner,
                );
            } else {
                let mut y = inner.y;
                for activity in &state.activity.activities {
                    if y >= inner.bottom() {
                        break;
                    }
                    frame.render_widget(
                        Paragraph::new(Line::from(vec![
                            Span::styled(
                                format!("{} ", spinner(state)),
                                Style::default().fg(theme.secondary_accent),
                            ),
                            Span::styled(
                                truncate(&activity.label, inner.width.saturating_sub(20) as usize),
                                Style::default().fg(theme.fg),
                            ),
                            Span::styled(
                                format!(
                                    "  {:?} · {}",
                                    activity.kind,
                                    activity.started_at.elapsed().as_secs_f64().round() as u64
                                ),
                                Style::default().fg(theme.fg_dim),
                            ),
                        ])),
                        Rect::new(inner.x, y, inner.width, 1),
                    );
                    y += 2;
                }
            }
            hit(regions, popup, HitAction::ActivityClose, 20);
        }
        Overlay::Toc => {
            let popup = centered(
                area,
                64.min(area.width.saturating_sub(4)),
                24.min(area.height.saturating_sub(4)),
            );
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border_active))
                    .title(" Contents ")
                    .title_bottom(" enter jump · esc close ")
                    .style(Style::default().bg(theme.picker_bg).fg(theme.fg)),
                popup,
            );
            let inner = Rect::new(
                popup.x + 2,
                popup.y + 1,
                popup.width.saturating_sub(4),
                popup.height.saturating_sub(3),
            );
            if let Some(doc) = state.active_tab().and_then(|tab| tab.doc.as_ref()) {
                let visible = inner.height as usize;
                let start = if state.toc_index >= visible {
                    state.toc_index + 1 - visible
                } else {
                    0
                };
                for (row, (index, entry)) in doc.toc.iter().enumerate().skip(start).enumerate() {
                    let y = inner.y.saturating_add(row as u16);
                    if y >= inner.bottom() {
                        break;
                    }
                    let selected = index == state.toc_index;
                    let indent = "  ".repeat(entry.level.saturating_sub(1) as usize);
                    frame.render_widget(
                        Paragraph::new(format!(
                            "{} {} {indent}{}",
                            if selected { "▸" } else { " " },
                            index + 1,
                            entry.title
                        ))
                        .style(
                            Style::default()
                                .fg(if selected {
                                    theme.selection_fg
                                } else {
                                    theme.fg
                                })
                                .bg(if selected {
                                    theme.selection_bg
                                } else {
                                    theme.picker_bg
                                }),
                        ),
                        Rect::new(inner.x, y, inner.width, 1),
                    );
                    hit(
                        regions,
                        Rect::new(inner.x, y, inner.width, 1),
                        HitAction::Toc(index),
                        21,
                    );
                }
            }
            hit(regions, popup, HitAction::CloseOverlay, 20);
        }
        Overlay::MediaViewer => draw_media_viewer(frame, state, area, theme, images, regions),
        Overlay::Peek => draw_peek(frame, state, area, theme, images, regions),
        Overlay::Calendar => draw_calendar(frame, state, area, theme, regions),
        Overlay::NewList => {
            let popup = centered(
                area,
                56.min(area.width.saturating_sub(4)),
                9.min(area.height.saturating_sub(4)),
            );
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border_active))
                    .title(" New reading list ")
                    .style(Style::default().bg(theme.picker_bg).fg(theme.fg)),
                popup,
            );
            let inner = Rect::new(popup.x + 2, popup.y + 2, popup.width.saturating_sub(4), 3);
            frame.render_widget(
                Paragraph::new("Name").style(Style::default().fg(theme.fg_dim)),
                Rect::new(inner.x, inner.y, inner.width, 1),
            );
            let value = state
                .new_list
                .as_ref()
                .map(|state| state.input.display_with_cursor())
                .unwrap_or_default();
            frame.render_widget(
                Paragraph::new(value)
                    .style(Style::default().fg(theme.fg_bright).bg(theme.elevated_bg)),
                Rect::new(inner.x, inner.y + 1, inner.width, 1),
            );
            let create = Rect::new(inner.x, inner.y + 3, 12, 1);
            frame.render_widget(
                Paragraph::new(" + create ").style(
                    Style::default()
                        .fg(theme.selection_fg)
                        .bg(theme.selection_bg),
                ),
                create,
            );
            hit(regions, create, HitAction::NewListSubmit, 22);
            let cancel = Rect::new(inner.x + 14, inner.y + 3, 10, 1);
            frame.render_widget(
                Paragraph::new(" cancel ")
                    .style(Style::default().fg(theme.fg_dim).bg(theme.elevated_bg)),
                cancel,
            );
            hit(regions, cancel, HitAction::CloseOverlay, 22);
        }
        Overlay::ConfirmDeleteList => {
            let popup = centered(
                area,
                58.min(area.width.saturating_sub(4)),
                9.min(area.height.saturating_sub(4)),
            );
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.error))
                    .title(" Delete reading list? ")
                    .style(Style::default().bg(theme.picker_bg).fg(theme.fg)),
                popup,
            );
            let name = state.active_list_name();
            frame.render_widget(
                Paragraph::new(format!(
                    "Delete “{name}”? The articles stay cached and saved if bookmarked."
                ))
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(theme.fg)),
                Rect::new(popup.x + 2, popup.y + 1, popup.width.saturating_sub(4), 3),
            );
            let yes = Rect::new(popup.x + 2, popup.bottom().saturating_sub(3), 10, 1);
            frame.render_widget(
                Paragraph::new(" yes (y) ")
                    .style(Style::default().fg(theme.selection_fg).bg(theme.error)),
                yes,
            );
            hit(regions, yes, HitAction::ConfirmDelete(true), 22);
            let no = Rect::new(popup.x + 14, popup.bottom().saturating_sub(3), 10, 1);
            frame.render_widget(
                Paragraph::new(" no (n) ")
                    .style(Style::default().fg(theme.fg).bg(theme.elevated_bg)),
                no,
            );
            hit(regions, no, HitAction::ConfirmDelete(false), 22);
        }
        Overlay::None => {}
    }
}

fn draw_calendar(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    theme: AppTheme,
    regions: &mut Vec<HitRegion>,
) {
    use chrono::Datelike;
    let cursor = state.calendar.cursor;
    let today = state.calendar.today;

    let popup = centered(
        area,
        30.min(area.width.saturating_sub(4)),
        12.min(area.height.saturating_sub(4)),
    );
    frame.render_widget(Clear, popup);
    let title = format!(" {} ", cursor.format("%B %Y"));
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_active))
            .title(Span::styled(
                title,
                Style::default()
                    .fg(theme.fg_bright)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Span::styled(
                " ↔ day · ↕ week · PgUp/PgDn month · enter go · t today ",
                Style::default().fg(theme.fg_dim),
            ))
            .style(Style::default().bg(theme.picker_bg).fg(theme.fg)),
        popup,
    );

    let inner = Rect::new(
        popup.x + 2,
        popup.y + 1,
        popup.width.saturating_sub(4),
        popup.height.saturating_sub(2),
    );

    // Header row: month nav arrows on the ends, weekday names in the middle.
    let weekdays = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
    let mut header = vec![Span::styled(
        " ‹ ",
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    )];
    for day in weekdays {
        header.push(Span::styled(
            format!("{day} "),
            Style::default().fg(theme.secondary_accent),
        ));
    }
    header.push(Span::styled(
        " › ",
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(
        Paragraph::new(Line::from(header)).style(Style::default().bg(theme.picker_bg)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    hit(
        regions,
        Rect::new(inner.x, inner.y, 3, 1),
        HitAction::CalendarMonth(-1),
        21,
    );
    hit(
        regions,
        Rect::new(inner.right().saturating_sub(3), inner.y, 3, 1),
        HitAction::CalendarMonth(1),
        21,
    );

    // Grid: weeks starting Sunday, 7 columns x 6 rows of day cells.
    let first = cursor.with_day(1).unwrap_or(cursor);
    let leading = first.weekday().num_days_from_sunday() as usize;
    let days = days_in_month(cursor.year(), cursor.month());
    let cell_width = 3u16;
    let grid_y = inner.y + 1;
    for row in 0..6u16 {
        if grid_y + row >= inner.bottom() {
            break;
        }
        for col in 0..7u16 {
            let index = row as i32 * 7 + col as i32 - leading as i32;
            let day_x = inner.x + col * cell_width;
            if index < 0 || index as u32 >= days {
                continue;
            }
            let day = index as u32 + 1;
            let Some(date) = cursor.with_day(day) else {
                continue;
            };
            let selected = date == cursor;
            let is_today = date == today;
            let future = date > today;
            let style = if selected {
                Style::default()
                    .fg(theme.selection_fg)
                    .bg(theme.selection_bg)
                    .add_modifier(Modifier::BOLD)
            } else if future {
                Style::default().fg(theme.muted_border)
            } else if is_today {
                // Theme-aware pill: accent background with the theme's
                // background as the contrasting foreground.
                Style::default()
                    .fg(theme.bg)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };
            let cell_bg = if is_today && !selected && !future {
                theme.accent
            } else {
                theme.picker_bg
            };
            frame.render_widget(
                Paragraph::new(format!("{day:>2} ")).style(style.bg(cell_bg)),
                Rect::new(day_x, grid_y + row, cell_width, 1),
            );
            hit(
                regions,
                Rect::new(day_x, grid_y + row, cell_width, 1),
                HitAction::CalendarDay(date),
                22,
            );
        }
    }
}

fn draw_peek(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    theme: AppTheme,
    images: &mut ImageRenderer,
    regions: &mut Vec<HitRegion>,
) {
    let peek = &state.peek;
    let width = (area.width.saturating_mul(2) / 3)
        .clamp(44, 72)
        .min(area.width.saturating_sub(4));
    let height = (area.height.saturating_mul(2) / 3)
        .clamp(12, 22)
        .min(area.height.saturating_sub(4));
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);
    let title = format!(
        " peek · {} ",
        truncate(&peek.title, popup.width.saturating_sub(12) as usize)
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_active))
            .title(Span::styled(
                title,
                Style::default()
                    .fg(theme.fg_bright)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Span::styled(
                " ctrl+k / esc close ",
                Style::default().fg(theme.fg_dim),
            ))
            .style(Style::default().bg(theme.picker_bg).fg(theme.fg)),
        popup,
    );
    let close = Rect::new(popup.right().saturating_sub(5), popup.y, 3, 1);
    frame.render_widget(
        Paragraph::new(" × ").style(Style::default().fg(theme.error).bg(theme.picker_bg)),
        close,
    );
    hit(regions, close, HitAction::PeekClose, 23);

    let inner = Rect::new(
        popup.x + 2,
        popup.y + 1,
        popup.width.saturating_sub(4),
        popup.height.saturating_sub(2),
    );
    if peek.loading && peek.summary.is_none() {
        frame.render_widget(
            Paragraph::new(format!("{} fetching summary…", spinner(state)))
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.fg_dim).bg(theme.picker_bg)),
            inner,
        );
        return;
    }
    let mut y = inner.y;
    // Preview image first when available and there is room.
    if let Some(url) = &peek.image {
        let key = MediaSpec::new(url.clone(), MediaKind::Image).key();
        let image_height = (inner.height.saturating_sub(4)).min(10);
        if image_height >= 3
            && let Some(Loadable::Ready { value, .. }) = state.images.get(&key)
        {
            let frame_index = value.frame_at(Duration::from_millis(state.tick * 40)).0;
            let area = Rect::new(inner.x, y, inner.width, image_height);
            let (iw, ih) = value
                .frames
                .first()
                .map(|f| (f.width(), f.height()))
                .unwrap_or((100, 75));
            let centered = crate::render::images::ImageRenderer::fit_centered(area, iw, ih);
            images.render_media(frame, &key, value, centered, frame_index);
            y += image_height + 1;
        }
    }
    if let Some(error) = &peek.error {
        frame.render_widget(
            Paragraph::new(format!("⚠ {}", truncate(error, inner.width as usize)))
                .style(Style::default().fg(theme.warning).bg(theme.picker_bg)),
            Rect::new(inner.x, y, inner.width, 1),
        );
        y += 2;
    }
    if y < inner.bottom() {
        let summary = peek.summary.clone().unwrap_or_default();
        let target = peek
            .target
            .as_ref()
            .map(|t| t.target.clone())
            .unwrap_or_default();
        frame.render_widget(
            Paragraph::new(summary)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(theme.fg).bg(theme.picker_bg)),
            Rect::new(
                inner.x,
                y,
                inner.width,
                inner.bottom().saturating_sub(y).saturating_sub(2),
            ),
        );
        if !target.is_empty() && inner.height > 4 {
            frame.render_widget(
                Paragraph::new(format!(
                    "↗ {}",
                    truncate(&target, inner.width.saturating_sub(3) as usize)
                ))
                .style(
                    Style::default()
                        .fg(theme.secondary_accent)
                        .bg(theme.picker_bg),
                ),
                Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
            );
        }
    }
}

fn draw_media_viewer(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    theme: AppTheme,
    images: &mut ImageRenderer,
    regions: &mut Vec<HitRegion>,
) {
    let Some(spec) = state.media_viewer.as_ref() else {
        return;
    };
    let popup = centered(
        area,
        area.width.saturating_sub(6),
        area.height.saturating_sub(3),
    );
    frame.render_widget(Clear, popup);
    let title = if spec.title.is_empty() {
        " Media viewer ".to_string()
    } else {
        format!(
            " {} ",
            truncate(&spec.title, popup.width.saturating_sub(6) as usize)
        )
    };
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_active))
            .title(title)
            .title_bottom(" space play/pause · esc close ")
            .style(Style::default().bg(theme.picker_bg).fg(theme.fg)),
        popup,
    );
    let close = Rect::new(popup.right().saturating_sub(5), popup.y, 3, 1);
    frame.render_widget(
        Paragraph::new(" × ").style(Style::default().fg(theme.error).bg(theme.picker_bg)),
        close,
    );
    hit(regions, close, HitAction::CloseOverlay, 23);
    let inner = Rect::new(
        popup.x + 2,
        popup.y + 1,
        popup.width.saturating_sub(4),
        popup.height.saturating_sub(3),
    );
    let key = if spec.kind == MediaKind::Math {
        format!("math:{}", spec.url)
    } else {
        spec.key()
    };
    match spec.kind {
        MediaKind::Video => {
            if state.video.key.as_deref() == Some(key.as_str()) {
                let media_area = Rect::new(
                    inner.x,
                    inner.y,
                    inner.width,
                    inner.height.saturating_sub(2),
                );
                draw_video(frame, state, images, media_area, theme);
                draw_player_bar(frame, state, theme, regions, media_area);
            } else {
                draw_media_viewer_status(frame, inner, theme, "Decoding video frames…", state);
            }
        }
        MediaKind::Audio => {
            if state.audio.data.is_some() {
                let media_area = Rect::new(
                    inner.x,
                    inner.y,
                    inner.width,
                    inner.height.saturating_sub(2),
                );
                draw_audio(frame, state, media_area, theme);
                draw_player_bar(frame, state, theme, regions, media_area);
            } else {
                draw_media_viewer_status(frame, inner, theme, "Loading audio…", state);
            }
        }
        MediaKind::Math => {
            draw_loaded_media(frame, state, images, inner, theme, &key, true);
        }
        _ => {
            draw_loaded_media(frame, state, images, inner, theme, &key, false);
        }
    }
}

fn draw_player_bar(
    frame: &mut Frame<'_>,
    state: &AppState,
    theme: AppTheme,
    regions: &mut Vec<HitRegion>,
    area: Rect,
) {
    let playing = match state.media_viewer.as_ref().map(|spec| spec.kind) {
        Some(MediaKind::Audio) => state.audio.status == crate::media::AudioStatus::Playing,
        _ => state.video.playing,
    };
    let (position, duration) = match state.media_viewer.as_ref().map(|spec| spec.kind) {
        Some(MediaKind::Audio) => (state.audio.position, state.audio.duration),
        _ => (state.video.position, state.video.duration),
    };
    let progress = if duration > 0.0 {
        (position / duration).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let bar = Rect::new(
        area.x,
        area.bottom().saturating_sub(2),
        area.width.saturating_sub(10),
        1,
    );
    let filled = (bar.width as f64 * progress).round() as u16;
    for x in 0..bar.width {
        let ch = if x < filled { "━" } else { "─" };
        frame.render_widget(
            Paragraph::new(ch).style(
                Style::default()
                    .fg(if x < filled {
                        theme.accent
                    } else {
                        theme.muted_border
                    })
                    .bg(theme.picker_bg),
            ),
            Rect::new(bar.x + x, bar.y, 1, 1),
        );
    }
    for segment in 0..20u16 {
        let x = bar.x + (bar.width * segment / 20);
        let width = (bar.width / 20).max(1);
        hit(
            regions,
            Rect::new(x, bar.y, width, 1),
            HitAction::MediaSeek((segment as f64 + 0.5) / 20.0),
            24,
        );
    }
    let play = Rect::new(bar.right().saturating_add(1), bar.y, 5, 1);
    frame.render_widget(
        Paragraph::new(if playing { " ❚❚ " } else { " ▶ " }).style(
            Style::default()
                .fg(theme.selection_fg)
                .bg(theme.selection_bg),
        ),
        play,
    );
    hit(regions, play, HitAction::MediaPlayPause, 24);
}

fn draw_media_viewer_status(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: AppTheme,
    label: &str,
    state: &AppState,
) {
    frame.render_widget(
        Paragraph::new(format!("{} {label}", spinner(state)))
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.fg_dim).bg(theme.picker_bg)),
        area,
    );
}

fn draw_toast(frame: &mut Frame<'_>, state: &AppState, area: Rect, theme: AppTheme) {
    let Some(toast) = &state.toast else {
        return;
    };
    if toast.expired() {
        return;
    }
    let width = (toast.text.width() as u16 + 6)
        .clamp(20, 54)
        .min(area.width.saturating_sub(2));
    let popup = Rect::new(
        area.right().saturating_sub(width + 1),
        area.bottom().saturating_sub(4),
        width,
        3,
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if toast.error {
                theme.error
            } else {
                theme.notification_border
            }))
            .style(Style::default().bg(theme.elevated_bg).fg(theme.fg)),
        popup,
    );
    frame.render_widget(
        Paragraph::new(truncate(&toast.text, width.saturating_sub(4) as usize)).style(
            Style::default()
                .fg(if toast.error { theme.error } else { theme.fg })
                .bg(theme.elevated_bg),
        ),
        Rect::new(popup.x + 2, popup.y + 1, width.saturating_sub(4), 1),
    );
}

fn draw_load_state<T>(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: Rect,
    loadable: &Loadable<T>,
    label: &str,
) -> bool {
    if loadable.value().is_some() {
        if loadable.is_loading() || loadable.error().is_some() {
            let banner = if let Some(error) = loadable.error() {
                format!(" ! {} ", error.message)
            } else {
                format!(
                    " {} refreshing {} ",
                    spinner(state),
                    loadable.stage().map(LoadStage::label).unwrap_or("")
                )
            };
            frame.render_widget(
                Paragraph::new(banner).style(
                    Style::default()
                        .fg(if loadable.error().is_some() {
                            theme_color(state).error
                        } else {
                            theme_color(state).secondary_accent
                        })
                        .bg(theme_color(state).elevated_bg),
                ),
                Rect::new(area.x, area.y, area.width, 1),
            );
        }
        return false;
    }
    match loadable {
        Loadable::Failed { error, .. } => draw_error(frame, state, area, error, label),
        Loadable::Unsupported { reason, .. } => {
            empty_state(frame, area, theme_color(state), "Unsupported", reason);
        }
        Loadable::Idle
        | Loadable::Queued { .. }
        | Loadable::Loading { .. }
        | Loadable::Refreshing { .. } => {
            draw_skeleton(frame, state, area, label);
        }
        Loadable::Ready { .. } => {}
    }
    true
}

fn draw_skeleton(frame: &mut Frame<'_>, state: &AppState, area: Rect, label: &str) {
    let theme = theme_color(state);
    let elapsed = state
        .home
        .started_at()
        .map(|started| started.elapsed())
        .unwrap_or_default();
    let timed_out = elapsed >= std::time::Duration::from_secs(45);
    let title = if timed_out {
        format!(
            " {label} · timed out after {}s — press r to retry ",
            elapsed.as_secs()
        )
    } else {
        format!(" {} · {} {}s ", label, spinner(state), elapsed.as_secs())
    };
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if timed_out {
                theme.warning
            } else {
                theme.muted_border
            }))
            .title(Span::styled(
                title,
                Style::default().fg(if timed_out {
                    theme.warning
                } else {
                    theme.secondary_accent
                }),
            ))
            .style(Style::default().bg(theme.bg)),
        area,
    );
    let inner = Rect::new(
        area.x + 2,
        area.y + 2,
        area.width.saturating_sub(4),
        area.height.saturating_sub(3),
    );
    let mut lines = Vec::new();
    let widths = [78u16, 54, 91, 67, 42, 83, 58];
    for (index, width) in widths.iter().enumerate() {
        if index as u16 >= inner.height {
            break;
        }
        let w = (*width as u32 * inner.width as u32 / 100).max(4) as usize;
        lines.push(Line::from(Span::styled(
            "░".repeat(w),
            Style::default().fg(theme.elevated_bg),
        )));
        lines.push(Line::from(""));
    }
    frame.render_widget(Paragraph::new(lines), inner);
    frame.render_widget(
        Paragraph::new("Ctrl+C cancel · the UI stays responsive")
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.fg_dim).bg(theme.bg)),
        Rect::new(area.x, area.bottom().saturating_sub(2), area.width, 1),
    );
}

fn draw_error(frame: &mut Frame<'_>, state: &AppState, area: Rect, error: &LoadError, label: &str) {
    let theme = theme_color(state);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.error))
            .title(format!(" {label} "))
            .style(Style::default().bg(theme.bg)),
        area,
    );
    let inner = Rect::new(
        area.x + 3,
        area.y + 2,
        area.width.saturating_sub(6),
        area.height.saturating_sub(4),
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "⚠ ",
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                error.message.clone(),
                Style::default()
                    .fg(theme.fg_bright)
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .wrap(Wrap { trim: true }),
        Rect::new(inner.x, inner.y, inner.width, 4),
    );
    if inner.height > 6 {
        frame.render_widget(
            Paragraph::new(format!(
                "{}\n\nPress r or click here to retry.\nCtrl+C quits safely; cached reading still works offline.",
                error.hint().unwrap_or("")
            ))
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(theme.fg_dim)),
            Rect::new(inner.x, inner.y + 5, inner.width, inner.height.saturating_sub(5)),
        );
    }
}

fn empty_state(frame: &mut Frame<'_>, area: Rect, theme: AppTheme, title: &str, body: &str) {
    let copy = vec![
        Line::from(Span::styled(
            title.to_string(),
            Style::default()
                .fg(theme.fg_bright)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            body.to_string(),
            Style::default().fg(theme.fg_dim),
        )),
    ];
    frame.render_widget(
        Paragraph::new(copy)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .style(Style::default().bg(theme.bg).fg(theme.fg)),
        area,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn relative_time(ts: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let delta = (now - ts).max(0);
    if delta < 60 {
        "just now".into()
    } else if delta < 3600 {
        format!("{}m ago", delta / 60)
    } else if delta < 86_400 {
        format!("{}h ago", delta / 3600)
    } else {
        format!("{}d ago", delta / 86_400)
    }
}

fn link_kind_label(kind: LinkKind) -> &'static str {
    match kind {
        LinkKind::Article => "article",
        LinkKind::External => "web",
        LinkKind::Media => "media",
        LinkKind::Citation => "citation",
    }
}

fn theme_color(state: &AppState) -> AppTheme {
    state.theme()
}

fn help_lines(theme: AppTheme) -> Vec<Line<'static>> {
    let key = Style::default()
        .fg(theme.secondary_accent)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(theme.fg_dim);
    let mut lines = vec![
        Line::from(Span::styled(
            "gtw — Wikipedia in your terminal",
            Style::default()
                .fg(theme.fg_bright)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Mouse-first: click navigation, feed stories, links, images, and controls. Scroll the wheel to move.",
            dim,
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  /  ", key),
            Span::styled("Search Wikipedia and your offline cache", dim),
        ]),
        Line::from(vec![
            Span::styled("  o  ", key),
            Span::styled(
                "Open the offline library picker (ctrl+o from an article)",
                dim,
            ),
        ]),
        Line::from(vec![
            Span::styled("  l  ", key),
            Span::styled("Browse reading lists (ctrl+l from anywhere)", dim),
        ]),
        Line::from(vec![
            Span::styled("  n  ", key),
            Span::styled("Create a new reading list", dim),
        ]),
        Line::from(vec![
            Span::styled("  h b y l m p ", key),
            Span::styled("Home, saved, history, lists, media lab, themes", dim),
        ]),
        Line::from(vec![
            Span::styled("  alt+space ", key),
            Span::styled("Open the navigation picker (Home, saved, lists…)", dim),
        ]),
        Line::from(vec![
            Span::styled("  alt+e ", key),
            Span::styled(
                "Article outline: links, headings, images, code, audio (queue-style preview strip below; [ ] filter; p toggles audio)",
                dim,
            ),
        ]),
        Line::from(vec![
            Span::styled("  [ ] ", key),
            Span::styled(
                "Home: switch feed categories · Outline picker: cycle element filters",
                dim,
            ),
        ]),
        Line::from(vec![
            Span::styled("  alt+` ", key),
            Span::styled("List and switch open tabs (x closes a tab)", dim),
        ]),
        Line::from(vec![
            Span::styled("  alt+d ", key),
            Span::styled("Jump-to-date calendar for the daily feed", dim),
        ]),
        Line::from(vec![
            Span::styled("  alt+h alt+o alt+l alt+t alt+a ", key),
            Span::styled(
                "History, saved+offline ([ ] categories), lists, themes, activity",
                dim,
            ),
        ]),
        Line::from(vec![
            Span::styled("  alt+j / alt+k ", key),
            Span::styled(
                "Article: jump next/previous link (vim-style, configurable peek)",
                dim,
            ),
        ]),
        Line::from(vec![
            Span::styled("  alt+c ", key),
            Span::styled("Table of contents overlay", dim),
        ]),
        Line::from(vec![
            Span::styled("  ctrl+f ", key),
            Span::styled("Find within the article (Enter/Up/Down jump matches)", dim),
        ]),
        Line::from(vec![
            Span::styled("  : / ; ", key),
            Span::styled("Command palette: navigation and runnable commands", dim),
        ]),
        Line::from(vec![
            Span::styled("  j/k ↑/↓ ", key),
            Span::styled("Move or scroll freely; wheel and PgUp/PgDn too", dim),
        ]),
        Line::from(vec![
            Span::styled("  ctrl+k ", key),
            Span::styled(
                "Peek the hovered link (or first visible link): summary + image",
                dim,
            ),
        ]),
        Line::from(vec![
            Span::styled("  enter ", key),
            Span::styled("Open selection; click a link to follow it", dim),
        ]),
        Line::from(vec![
            Span::styled("  B  ", key),
            Span::styled("Bookmark the current article (pinned, never expires)", dim),
        ]),
        Line::from(vec![
            Span::styled("  a  ", key),
            Span::styled("Add the current article to a reading list", dim),
        ]),
        Line::from(vec![
            Span::styled("  r  ", key),
            Span::styled("Refresh the current feed or article", dim),
        ]),
        Line::from(vec![
            Span::styled("  o  ", key),
            Span::styled("While reading: open the article in your browser", dim),
        ]),
        Line::from(vec![
            Span::styled("  space ", key),
            Span::styled("Play/pause audio or video", dim),
        ]),
        Line::from(vec![
            Span::styled("  ?  ", key),
            Span::styled("This help overlay", dim),
        ]),
        Line::from(vec![
            Span::styled("  A  ", key),
            Span::styled("Activity center (or click activity in the header)", dim),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Offline: articles are HTML-parsed, zstd-compressed at rest, and refreshed on every open. The 3-day timer slides on each read; bookmarks and reading-list entries are exempt.",
            dim,
        )),
        Line::from(Span::styled(
            "Media: images/GIF/SVG via ratatui-image, math via RaTeX + resvg, audio via Symphonia + rodio, video via pure-Rust zvidlib (VP8/VP9 WebM previews).",
            dim,
        )),
        Line::from(Span::styled(
            "Data: Wikipedia action API, Wikimedia featured feed, and Wikimedia Commons. gtw is not affiliated with the Wikimedia Foundation.",
            dim,
        )),
        Line::from(Span::styled(
            "Inspired by nullspace's terminal math rendering. Licensed GPL-3.0.",
            dim,
        )),
        Line::from(""),
        Line::from(Span::styled("q or ctrl+c quits. esc closes overlays.", key)),
    ];
    lines.truncate(140);
    lines
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use crate::core::{LoadStage, Overlay};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    const WIDTH: u16 = 100;
    const HEIGHT: u16 = 30;

    fn loading<T>() -> Loadable<T> {
        Loadable::Loading {
            stage: LoadStage::Downloading,
            progress: None,
            started_at: std::time::Instant::now(),
            previous: None,
            cancel: tokio_util::sync::CancellationToken::new(),
        }
    }

    fn state() -> AppState {
        use chrono::NaiveDate;
        let mut s = AppState::new(crate::core::Config::default(), false, false);
        let fixed = NaiveDate::from_ymd_opt(2026, 11, 5).unwrap();
        s.feed_date = fixed;
        s.calendar.cursor = fixed;
        s.calendar.today = fixed;
        s
    }

    fn render(name: &str, state: &mut AppState) {
        let backend = TestBackend::new(WIDTH, HEIGHT);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut images = ImageRenderer::headless();
        terminal
            .draw(|frame| draw(frame, state, &mut images))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                rendered.push_str(buffer[(x, y)].symbol());
            }
            rendered.push('\n');
            while rendered.ends_with(" \n") {
                rendered.pop();
                rendered.pop();
                rendered.push('\n');
            }
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots")
            .join(format!("{name}.txt"));
        if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
            std::fs::create_dir_all(path.parent().expect("snapshot dir")).expect("mkdir");
            std::fs::write(&path, &rendered).expect("write snapshot");
            return;
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "missing snapshot {} ({error}); run UPDATE_SNAPSHOTS=1 cargo test",
                path.display()
            )
        });
        assert_eq!(rendered, expected, "snapshot changed: {}", path.display());
    }

    #[test]
    fn loading_states_match_snapshots() {
        use chrono::NaiveDate;
        let fixed = NaiveDate::from_ymd_opt(2026, 11, 5).unwrap();
        let mut home = state();
        home.screen = Screen::Home;
        home.home = loading();
        home.feed_date = fixed;
        home.calendar.cursor = fixed;
        home.calendar.today = fixed;
        render("loading_home", &mut home);

        let mut article = state();
        article.open_article("Terminal emulator", true);
        render("loading_article", &mut article);

        let mut bookmarks = state();
        bookmarks.screen = Screen::Bookmarks;
        bookmarks.bookmarks = loading();
        render("loading_bookmarks", &mut bookmarks);

        let mut history = state();
        history.screen = Screen::History;
        history.history = loading();
        render("loading_history", &mut history);

        let mut lists = state();
        lists.screen = Screen::Lists;
        lists.lists = loading();
        render("loading_lists", &mut lists);

        let mut detail = state();
        detail.screen = Screen::ListDetail;
        detail.active_list = Some(1);
        detail.list_items = loading();
        render("loading_list_detail", &mut detail);

        let mut lab = state();
        lab.screen = Screen::MediaLab;
        render("media_lab", &mut lab);

        let mut themes = state();
        themes.screen = Screen::Home;
        themes.open_theme_picker();
        render("theme_picker", &mut themes);

        let mut help = state();
        help.screen = Screen::Home;
        help.overlay = Overlay::Help;
        render("overlay_help", &mut help);
    }
}
