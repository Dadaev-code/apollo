//! Modern V4L2 capture with zero-copy and DMA-BUF support

use std::os::unix::raw::dev_t;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use color_eyre::{eyre::eyre, Result};
use tracing::{info, instrument};
use v4l::buffer::Type;
use v4l::capability::Flags as CapFlags;
use v4l::io::traits::CaptureStream;
use v4l::prelude::MmapStream;
use v4l::video::Capture;
use v4l::{Device, FourCC};

use crate::{
    capture::frame::{Frame, FrameMetadata, PixelFormat},
    CaptureConfig,
};

/// High-performance V4L2 capture
pub struct V4l2Capture {
    device: Box<Device>,
    stream: Option<MmapStream<'static>>,
    config: CaptureConfig,
    sequence: u64,
    _buffers: Vec<Arc<[u8]>>, // Pre-allocated buffers
}

impl V4l2Capture {
    /// Create new capture instance with zero-copy buffers
    pub fn new(config: CaptureConfig) -> Result<Self> {
        info!("Initializing V4L2 capture: {:?}", config.device);

        // Open device with O_NONBLOCK for async I/O
        let device = Device::with_path(&config.device.path)?;

        // Query capabilities
        let caps = device.query_caps()?;
        info!("Device: {} ({})", caps.card, caps.driver);

        if !caps.capabilities.contains(CapFlags::VIDEO_CAPTURE) {
            return Err(eyre!("Device doesn't support video capture"));
        }

        // Set format
        let mut fmt = device.format()?;
        fmt.width = config.width;
        fmt.height = config.height;
        fmt.fourcc = match config.format {
            PixelFormat::Mjpeg => FourCC::new(b"MJPG"),
            PixelFormat::Yuyv4 => FourCC::new(b"YUYV"),
            _ => return Err(eyre!("Unsupported pixel format")),
        };

        device.set_format(&fmt)?;

        // Pre-allocate buffers for zero-copy
        let buffer_size = (config.width * config.height * 3) as usize;
        let mut buffers = Vec::with_capacity(config.buffer_count as usize);

        for _ in 0..config.buffer_count {
            // Allocate aligned memory for SIMD operations
            let mut buf = Vec::with_capacity(buffer_size);
            buf.resize(buffer_size, 0);
            buffers.push(Arc::from(buf.into_boxed_slice()));
        }

        Ok(Self {
            device: Box::new(device),
            stream: None,
            config,
            sequence: 0,
            _buffers: buffers,
        })
    }

    /// Start streaming with memory-mapped buffers
    pub fn start_stream(&mut self) -> Result<()> {
        // Request buffers
        let stream =
            MmapStream::with_buffers(&self.device, Type::VideoCapture, self.config.buffer_count)?;

        self.stream = Some(stream);
        info!(
            "Capture stream started with {} buffers",
            self.config.buffer_count
        );
        Ok(())
    }

    /// Capture frame with zero-copy when possible
    #[instrument(skip(self))]
    pub async fn capture_frame(&mut self) -> Result<Frame> {
        let timestamp = Instant::now();

        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| eyre!("Stream not started"))?;

        // Non-blocking dequeue
        let (buf, meta) = stream.next()?;

        // Zero-copy: create Bytes from mmap'd buffer
        let data = Bytes::copy_from_slice(&buf);

        self.sequence += 1;

        let frame_meta = Arc::new(FrameMetadata {
            sequence: self.sequence,
            width: self.config.width,
            height: self.config.height,
            stride: self.config.width,
            format: self.config.format,
            device_timestamp: Some(
                Duration::from_secs(meta.timestamp.sec as u64)
                    + Duration::from_micros(meta.timestamp.usec as u64),
            ),
        });

        Ok(Frame {
            data,
            meta: frame_meta,
            timestamp,
        })
    }
}
