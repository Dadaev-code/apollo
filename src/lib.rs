pub mod capture;
pub mod display;
pub mod utils;

use arc_swap::ArcSwap;
use capture::frame::PixelFormat;
use serde::{Deserialize, Serialize};

use crate::utils::FoundDevice;

/// Global configuration that can be atomically swapped at runtime
pub static CONFIG: once_cell::sync::Lazy<ArcSwap<Config>> =
    once_cell::sync::Lazy::new(|| ArcSwap::from_pointee(Config::default()));

/// System configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub capture: CaptureConfig,
    pub display: DisplayConfig,
    pub pipeline: PipelineConfig,
    #[cfg(feature = "gstreamer-pipeline")]
    pub gstreamer: GStreamerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureConfig {
    pub device: FoundDevice,
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

#[cfg(feature = "gstreamer-pipeline")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GStreamerConfig {
    pub use_hardware_acceleration: bool,
    pub prefer_zero_copy: bool,
    pub custom_pipeline: Option<String>,
    pub enable_fps_overlay: bool,
    pub buffer_pool_size: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            capture: CaptureConfig {
                device: FoundDevice::new("/dev/video0".into(), PixelFormat::Mjpeg.into()),
                width: 800,
                height: 600,
                fps: 30,
                format: PixelFormat::Mjpeg,
                buffer_count: 4,
                use_mmap: true,
                use_dmabuf: false, // Requires kernel 5.19+
            },
            display: DisplayConfig {
                width: 800,
                height: 600,
            },
            pipeline: PipelineConfig {
                ring_buffer_size: 8,
                decode_threads: 2,
                enable_profiling: false,
                // target_latency_ms: 16, // 60fps target
                target_latency_ms: 32, // 30fps target
            },
            #[cfg(feature = "gstreamer-pipeline")]
            gstreamer: GStreamerConfig {
                use_hardware_acceleration: true,
                prefer_zero_copy: true,
                custom_pipeline: None,
                enable_fps_overlay: true,
                buffer_pool_size: 4,
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
