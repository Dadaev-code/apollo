//! GStreamer-based high-performance video capture with hardware acceleration

use std::sync::{Arc, Mutex};
use std::time::Instant;

use bytes::Bytes;
use color_eyre::{eyre::eyre, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use tracing::{debug, info, warn};

use crate::capture::frame::{Frame, FrameMetadata, PixelFormat};
use crate::CaptureConfig;

/// GStreamer-based capture with zero-copy and hardware acceleration
pub struct GstCapture {
    pipeline: gst::Pipeline,
    appsink: gst_app::AppSink,
    config: CaptureConfig,
    sequence: Arc<Mutex<u64>>,
}

impl GstCapture {
    /// Create a new GStreamer capture pipeline with hardware acceleration
    pub fn new(config: CaptureConfig) -> Result<Self> {
        // Initialize GStreamer
        gst::init().map_err(|e| eyre!("Failed to initialize GStreamer: {}", e))?;

        info!("Initializing GStreamer capture pipeline");

        // Build pipeline string based on configuration
        let pipeline_str = Self::build_pipeline_string(&config)?;
        info!("Pipeline: {}", pipeline_str);

        // Create pipeline from string
        let pipeline = gst::parse::launch(&pipeline_str)?
            .downcast::<gst::Pipeline>()
            .map_err(|_| eyre!("Failed to create pipeline"))?;

        // Get appsink element
        let appsink = pipeline
            .by_name("appsink")
            .ok_or_else(|| eyre!("Failed to find appsink element"))?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| eyre!("Failed to cast to AppSink"))?;

        // Configure appsink for zero-copy operation
        appsink.set_property("emit-signals", false);
        appsink.set_property("max-buffers", 3u32);
        appsink.set_property("drop", true); // Drop old buffers if we can't keep up
        appsink.set_property("sync", false); // Don't sync to clock for lowest latency

        Ok(Self {
            pipeline,
            appsink,
            config,
            sequence: Arc::new(Mutex::new(0)),
        })
    }

    /// Build optimized GStreamer pipeline string
    fn build_pipeline_string(config: &CaptureConfig) -> Result<String> {
        let device = &config.device.path;
        let width = config.width;
        let height = config.height;
        let fps = config.fps;

        // Detect available hardware decoders
        let jpeg_decoder = Self::detect_jpeg_decoder();
        info!("Using JPEG decoder: {}", jpeg_decoder);

        // Build pipeline based on format
        let pipeline = match config.format {
            PixelFormat::Mjpeg => {
                // MJPEG pipeline with hardware decoder
                format!(
                    "v4l2src device={} name=source ! \
                     image/jpeg,width={},height={},framerate={}/1 ! \
                     queue max-size-buffers=2 max-size-time=0 max-size-bytes=0 ! \
                     {} ! \
                     videoconvert ! \
                     video/x-raw,format=RGB ! \
                     appsink name=appsink",
                    device, width, height, fps, jpeg_decoder
                )
            }
            PixelFormat::Yuyv4 => {
                // YUYV pipeline with hardware color conversion
                format!(
                    "v4l2src device={} name=source ! \
                     video/x-raw,format=YUY2,width={},height={},framerate={}/1 ! \
                     queue max-size-buffers=2 max-size-time=0 max-size-bytes=0 ! \
                     videoconvert ! \
                     video/x-raw,format=RGB ! \
                     appsink name=appsink",
                    device, width, height, fps
                )
            }
            PixelFormat::Rgb24 => {
                // Direct RGB capture (fastest path)
                format!(
                    "v4l2src device={} name=source ! \
                     video/x-raw,format=RGB,width={},height={},framerate={}/1 ! \
                     queue max-size-buffers=2 max-size-time=0 max-size-bytes=0 ! \
                     appsink name=appsink",
                    device, width, height, fps
                )
            }
            _ => return Err(eyre!("Unsupported pixel format: {:?}", config.format)),
        };

        Ok(pipeline)
    }

    /// Detect best available JPEG decoder (hardware > software)
    fn detect_jpeg_decoder() -> &'static str {
        // Check for hardware decoders in order of preference
        let decoders = [
            "nvjpegdec",       // NVIDIA hardware decoder
            "vaapijpegdec",    // Intel/AMD VAAPI hardware decoder  
            "v4l2jpegdec",     // V4L2 hardware decoder
            "jpegdec",         // Software decoder (fallback)
        ];

        for decoder in &decoders {
            if let Some(factory) = gst::ElementFactory::find(decoder) {
                debug!("Found decoder: {} - {}", decoder, factory.metadata("long-name").unwrap_or(""));
                return decoder;
            }
        }

        warn!("No hardware JPEG decoder found, using software decoder");
        "jpegdec"
    }

    /// Start the capture pipeline
    pub fn start_stream(&mut self) -> Result<()> {
        info!("Starting GStreamer pipeline");
        
        // Set pipeline to playing state
        self.pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| eyre!("Failed to start pipeline: {:?}", e))?;

        // Wait for pipeline to reach playing state
        let (state_change, _, _) = self.pipeline.state(Some(gst::ClockTime::from_seconds(5)));
        
        match state_change {
            Ok(gst::StateChangeSuccess::Success) => {
                info!("Pipeline started successfully");
                Ok(())
            }
            Ok(gst::StateChangeSuccess::Async) => {
                info!("Pipeline starting asynchronously");
                Ok(())
            }
            _ => Err(eyre!("Failed to start pipeline")),
        }
    }

    /// Stop the capture pipeline
    pub fn stop_stream(&mut self) -> Result<()> {
        info!("Stopping GStreamer pipeline");
        
        self.pipeline
            .set_state(gst::State::Null)
            .map_err(|e| eyre!("Failed to stop pipeline: {:?}", e))?;
        
        Ok(())
    }

    /// Capture a frame with zero-copy when possible
    pub async fn capture_frame(&mut self) -> Result<Frame> {
        let timestamp = Instant::now();

        // Pull sample from appsink (blocking)
        let sample = self
            .appsink
            .pull_sample()
            .map_err(|_| eyre!("Failed to pull sample from pipeline"))?;

        // Get buffer from sample
        let buffer = sample
            .buffer()
            .ok_or_else(|| eyre!("Sample contains no buffer"))?;

        // Map buffer for reading (zero-copy when possible)
        let map = buffer
            .map_readable()
            .map_err(|_| eyre!("Failed to map buffer"))?;

        // Create Bytes from buffer data
        // Note: This is still a copy, but GStreamer may have already done zero-copy from V4L2
        let data = Bytes::copy_from_slice(map.as_slice());

        // Get caps for metadata
        let caps = sample
            .caps()
            .ok_or_else(|| eyre!("Sample has no caps"))?;
        
        let video_info = gst_video::VideoInfo::from_caps(caps)
            .map_err(|_| eyre!("Failed to parse video info from caps"))?;

        // Update sequence number
        let sequence = {
            let mut seq = self.sequence.lock().unwrap();
            *seq += 1;
            *seq
        };

        // Build frame metadata
        let meta = Arc::new(FrameMetadata {
            sequence,
            width: video_info.width(),
            height: video_info.height(),
            stride: video_info.width(),
            format: PixelFormat::Rgb24, // Output is always RGB after conversion
            device_timestamp: buffer.pts().map(|pts| pts.into()),
        });

        Ok(Frame {
            data,
            meta,
            timestamp,
        })
    }

    /// Get pipeline statistics
    pub fn get_stats(&self) -> PipelineStats {
        let position = self.pipeline.query_position::<gst::ClockTime>();
        
        // Query latency using the latency query
        let mut query = gst::query::Latency::new();
        let latency_ms = if self.pipeline.query(query.query_mut()) {
            let (_, max, _) = query.result();
            max.mseconds()
        } else {
            0
        };
        
        PipelineStats {
            position: position.map(|p| p.mseconds()),
            latency: latency_ms,
            state: format!("{:?}", self.pipeline.current_state()),
        }
    }
}

impl Drop for GstCapture {
    fn drop(&mut self) {
        let _ = self.stop_stream();
    }
}

/// Pipeline statistics for monitoring
#[derive(Debug)]
pub struct PipelineStats {
    pub position: Option<u64>,
    pub latency: u64,
    pub state: String,
}