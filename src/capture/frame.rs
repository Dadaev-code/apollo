use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Frame data with zero-copy semantics
#[derive(Clone)]
pub struct Frame {
    /// Immutable frame data - can be shared across threads without copying
    pub data: Bytes,

    /// Frame metadata
    pub meta: Arc<FrameMetadata>,

    /// Capture timestamp for latency tracking
    pub timestamp: Instant,
}

/// Frame metadata
#[derive(Debug, Clone)]
pub struct FrameMetadata {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: PixelFormat,
    pub device_timestamp: Option<Duration>, // Hardware timestamp if available
}

/// Pixel formats we support
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PixelFormat {
    Rgb24,
    Bgr24,
    Yuyv4,
    Mjpeg,
    Nv12,
}
