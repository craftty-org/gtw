pub mod audio;
pub mod image;
pub mod video;

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use ::image::RgbaImage;

use crate::core::MediaKind;

#[derive(Clone)]
pub struct DecodedMedia {
    pub frames: Vec<RgbaImage>,
    pub delays: Vec<Duration>,
    pub width: u32,
    pub height: u32,
    pub kind: MediaKind,
}

impl fmt::Debug for DecodedMedia {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecodedMedia")
            .field("frames", &self.frames.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .field("kind", &self.kind)
            .finish()
    }
}

impl DecodedMedia {
    pub fn single(image: RgbaImage, kind: MediaKind) -> Self {
        Self {
            width: image.width(),
            height: image.height(),
            frames: vec![image],
            delays: vec![Duration::from_millis(100)],
            kind,
        }
    }

    pub fn animated(frames: Vec<RgbaImage>, delays: Vec<Duration>, kind: MediaKind) -> Self {
        let first = frames.first();
        Self {
            width: first.map(RgbaImage::width).unwrap_or(1),
            height: first.map(RgbaImage::height).unwrap_or(1),
            frames,
            delays,
            kind,
        }
    }

    pub fn frame_at(&self, elapsed: Duration) -> (usize, &RgbaImage) {
        if self.frames.len() <= 1 {
            return (0, &self.frames[0]);
        }
        let cycle: u64 = self
            .delays
            .iter()
            .map(|d| d.as_millis().max(1) as u64)
            .sum::<u64>()
            .max(1);
        let mut t = elapsed.as_millis() as u64 % cycle;
        for (i, delay) in self.delays.iter().enumerate() {
            let ms = delay.as_millis().max(1) as u64;
            if t < ms {
                return (i, &self.frames[i]);
            }
            t -= ms;
        }
        (self.frames.len() - 1, &self.frames[self.frames.len() - 1])
    }
}

#[derive(Clone)]
pub struct AudioData {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration: f64,
    pub title: String,
}

impl fmt::Debug for AudioData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioData")
            .field("samples", &self.samples.len())
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("duration", &self.duration)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioStatus {
    Idle,
    Loading,
    Ready,
    Playing,
    Paused,
    Failed,
}

#[derive(Debug, Clone)]
pub struct AudioState {
    pub key: Option<String>,
    pub title: String,
    pub status: AudioStatus,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub data: Option<Arc<AudioData>>,
    pub play_requested: bool,
    pub pause_requested: bool,
    pub stop_requested: bool,
    pub seek_to: Option<f64>,
    pub error: Option<String>,
    /// Set when the user asked to play before the decode finished; the decode
    /// handler honours it so the clip starts automatically.
    pub autoplay_intent: bool,
}

impl Default for AudioState {
    fn default() -> Self {
        Self {
            key: None,
            title: String::new(),
            status: AudioStatus::Idle,
            position: 0.0,
            duration: 0.0,
            volume: 0.75,
            data: None,
            play_requested: false,
            pause_requested: false,
            stop_requested: false,
            seek_to: None,
            error: None,
            autoplay_intent: false,
        }
    }
}

impl AudioState {
    pub fn ready(key: String, data: AudioData, autoplay: bool) -> Self {
        Self {
            key: Some(key),
            title: data.title.clone(),
            status: AudioStatus::Ready,
            duration: data.duration,
            data: Some(Arc::new(data)),
            play_requested: autoplay,
            ..Default::default()
        }
    }

    pub fn take_play_request(&mut self) -> bool {
        std::mem::take(&mut self.play_requested)
    }

    pub fn take_stop_request(&mut self) -> bool {
        std::mem::take(&mut self.stop_requested)
    }

    pub fn take_pause_request(&mut self) -> bool {
        std::mem::take(&mut self.pause_requested)
    }

    pub fn take_seek(&mut self) -> Option<f64> {
        self.seek_to.take()
    }

    pub fn position_label(&self) -> String {
        format!(
            "{}:{:02} / {}:{:02}",
            (self.position.max(0.0) as u64) / 60,
            (self.position.max(0.0) as u64) % 60,
            (self.duration.max(0.0) as u64) / 60,
            (self.duration.max(0.0) as u64) % 60,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoStatus {
    Idle,
    Loading,
    Ready,
    Playing,
    Paused,
    Failed,
}

#[derive(Debug, Clone)]
pub struct VideoState {
    pub key: Option<String>,
    pub title: String,
    pub status: VideoStatus,
    pub total: u32,
    pub index: u32,
    pub position: f64,
    pub duration: f64,
    pub playing: bool,
    pub volume: f32,
    pub frame: Option<Arc<RgbaImage>>,
    pub error: Option<String>,
}

impl Default for VideoState {
    fn default() -> Self {
        Self {
            key: None,
            title: String::new(),
            status: VideoStatus::Idle,
            total: 0,
            index: 0,
            position: 0.0,
            duration: 0.0,
            playing: false,
            volume: 0.75,
            frame: None,
            error: None,
        }
    }
}

impl VideoState {
    pub fn set_frame(
        &mut self,
        key: String,
        index: u32,
        total: u32,
        frame: Arc<RgbaImage>,
        position: f64,
    ) {
        self.key = Some(key);
        self.index = index;
        self.total = total;
        self.frame = Some(frame);
        self.position = position;
        self.status = if self.playing {
            VideoStatus::Playing
        } else {
            VideoStatus::Ready
        };
    }

    pub fn ready(&mut self, key: String, total: u32, duration: f64) {
        self.key = Some(key);
        self.total = total;
        self.duration = duration;
        self.status = VideoStatus::Ready;
        self.playing = false;
    }

    pub fn apply_state(&mut self, key: &str, playing: bool, position: f64, volume: f32) {
        if self.key.as_deref() == Some(key) {
            self.playing = playing;
            self.position = position;
            self.volume = volume;
            self.status = if playing {
                VideoStatus::Playing
            } else {
                VideoStatus::Paused
            };
        }
    }

    pub fn fail(&mut self, key: String, error: String) {
        self.key = Some(key);
        self.status = VideoStatus::Failed;
        self.error = Some(error);
        self.playing = false;
    }

    pub fn position_label(&self) -> String {
        format!(
            "{}:{:02} / {}:{:02}",
            (self.position.max(0.0) as u64) / 60,
            (self.position.max(0.0) as u64) % 60,
            (self.duration.max(0.0) as u64) / 60,
            (self.duration.max(0.0) as u64) % 60,
        )
    }
}
