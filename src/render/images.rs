use std::collections::HashMap;

use ::image::RgbaImage;
use ratatui::Frame;
use ratatui::layout::{Rect, Size};
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use ratatui_image::{Image, Resize};

use crate::media::DecodedMedia;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    key: String,
    width: u16,
    height: u16,
    frame: usize,
}

/// Terminal image backend. Uses the best protocol the terminal advertises and
/// falls back to Unicode half blocks, which work everywhere.
pub struct ImageRenderer {
    picker: Picker,
    protocols: HashMap<CacheKey, Protocol>,
}

impl ImageRenderer {
    pub fn new() -> Self {
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
        Self {
            picker,
            protocols: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub fn headless() -> Self {
        Self {
            picker: Picker::halfblocks(),
            protocols: HashMap::new(),
        }
    }

    pub fn protocol_name(&self) -> &'static str {
        // Picker does not expose a stable protocol name; halfblocks is the
        // universal fallback and the doctor reports the same distinction.
        if self.picker.font_size().width == 0 || self.picker.font_size().height == 0 {
            "halfblocks"
        } else {
            "terminal graphics"
        }
    }

    /// Render media. Encodes/caches at `encode_area` (stable) but draws to
    /// `draw_area` (may be a visible/growing sub-rect) to avoid mid-scroll
    /// resize thrash while allowing reveal-as-you-scroll. Pass the same rect
    /// for both for stable rendering.
    pub fn render_media_reveal(
        &mut self,
        frame: &mut Frame<'_>,
        key: &str,
        media: &DecodedMedia,
        draw_area: Rect,
        encode_area: Rect,
        frame_index: usize,
    ) {
        if draw_area.width < 2 || draw_area.height < 2 || media.frames.is_empty() {
            return;
        }
        let index = frame_index.min(media.frames.len() - 1);
        self.render_rgba_reveal(
            frame,
            &format!("{key}#{index}"),
            &media.frames[index],
            draw_area,
            encode_area,
        );
    }

    pub fn render_media(
        &mut self,
        frame: &mut Frame<'_>,
        key: &str,
        media: &DecodedMedia,
        area: Rect,
        frame_index: usize,
    ) {
        self.render_media_reveal(frame, key, media, area, area, frame_index);
    }

    pub fn render_rgba_reveal(
        &mut self,
        frame: &mut Frame<'_>,
        key: &str,
        image: &RgbaImage,
        draw_area: Rect,
        encode_area: Rect,
    ) {
        if draw_area.width < 2
            || draw_area.height < 2
            || encode_area.width < 2
            || encode_area.height < 2
            || image.width() == 0
            || image.height() == 0
        {
            return;
        }
        let cache_key = CacheKey {
            key: key.to_string(),
            width: encode_area.width,
            height: encode_area.height,
            frame: 0,
        };
        if !self.protocols.contains_key(&cache_key) {
            if self.protocols.len() > 72 {
                self.protocols.clear();
            }
            let dynamic = ::image::DynamicImage::ImageRgba8(image.clone());
            match self.picker.new_protocol(
                dynamic,
                Size::new(encode_area.width, encode_area.height),
                Resize::Fit(None),
            ) {
                Ok(protocol) => {
                    self.protocols.insert(cache_key.clone(), protocol);
                }
                Err(error) => {
                    tracing::debug!("image protocol encode failed: {error}");
                    return;
                }
            }
        }
        if let Some(protocol) = self.protocols.get(&cache_key) {
            frame.render_widget(Image::new(protocol), draw_area);
        }
    }

    pub fn render_rgba(&mut self, frame: &mut Frame<'_>, key: &str, image: &RgbaImage, area: Rect) {
        self.render_rgba_reveal(frame, key, image, area, area);
    }

    /// Center `desired_w × desired_h` (in cells) within `area`, preserving
    /// the source image's aspect ratio where possible.
    pub fn fit_centered(area: Rect, img_w: u32, img_h: u32) -> Rect {
        if area.width < 2 || area.height < 2 || img_w == 0 || img_h == 0 {
            return area;
        }
        // Cells are roughly twice as tall as wide in pixels.
        let img_aspect = img_w as f64 / (img_h as f64 * 0.5);
        let area_aspect = area.width as f64 / area.height.max(1) as f64;
        let (w, h) = if img_aspect > area_aspect {
            let w = area.width;
            let h = ((w as f64 / img_aspect).round() as u16)
                .max(2)
                .min(area.height);
            (w, h)
        } else {
            let h = area.height;
            let w = ((h as f64 * img_aspect).round() as u16)
                .max(4)
                .min(area.width);
            (w, h)
        };
        Rect::new(
            area.x + (area.width.saturating_sub(w)) / 2,
            area.y + (area.height.saturating_sub(h)) / 2,
            w,
            h,
        )
    }

    pub fn clear(&mut self) {
        self.protocols.clear();
    }
}

impl Default for ImageRenderer {
    fn default() -> Self {
        Self::new()
    }
}
