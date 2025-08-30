//! Video Pipeline
//!
//! Zero-copy, lock-free, GPU-accelerated video processing

#![warn(rust_2018_idioms)]
#![forbid(unsafe_code)] // We'll allow unsafe only where needed

use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

pub mod capture;
pub mod display;
pub mod error;
pub mod pipeline;

/// Global configuration that can be atomically swapped at runtime
pub static CONFIG: once_cell::sync::Lazy<ArcSwap<Config>> =
    once_cell::sync::Lazy::new(|| ArcSwap::from_pointee(Config::default()));

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
    Yuyv422,
    Mjpeg,
    Nv12, // Hardware-accelerated format
}

/// System configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub capture: CaptureConfig,
    pub display: DisplayConfig,
    pub pipeline: PipelineConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureConfig {
    pub device: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub format: PixelFormat,
    pub buffer_count: u32,
    pub use_mmap: bool,   // Memory-mapped I/O
    pub use_dmabuf: bool, // DMA-BUF for zero-copy to GPU
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    pub width: u32,
    pub height: u32,
    pub vsync: bool,
    pub fullscreen: bool,
    pub gpu_backend: GpuBackend,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum GpuBackend {
    Vulkan,
    Metal,
    Dx12,
    OpenGl,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    pub ring_buffer_size: usize,
    pub decode_threads: usize,
    pub enable_profiling: bool,
    pub target_latency_ms: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            capture: CaptureConfig {
                device: "/dev/video0".into(),
                width: 1920,
                height: 1080,
                fps: 30,
                format: PixelFormat::Mjpeg,
                buffer_count: 4,
                use_mmap: true,
                use_dmabuf: false, // Requires kernel 5.19+
            },
            display: DisplayConfig {
                width: 1920,
                height: 1080,
                vsync: false,
                fullscreen: false,
                gpu_backend: GpuBackend::Auto,
            },
            pipeline: PipelineConfig {
                ring_buffer_size: 8,
                decode_threads: 2,
                enable_profiling: false,
                target_latency_ms: 16, // 60fps target
            },
        }
    }
}

/// Performance metrics collected throughout the pipeline
#[derive(Debug, Default)]
pub struct Metrics {
    pub capture_fps: f64,
    pub display_fps: f64,
    pub dropped_frames: u64,
    pub avg_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub decode_time_us: u64,
    pub render_time_us: u64,
}
