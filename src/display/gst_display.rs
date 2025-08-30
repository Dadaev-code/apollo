//! GStreamer-based display with hardware acceleration and zero-copy pipeline

use color_eyre::{eyre::eyre, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use tracing::{info, warn};

use crate::{CaptureConfig, DisplayConfig};

/// GStreamer-based complete pipeline from capture to display
/// This provides the best performance by keeping everything in GStreamer
pub struct GstDisplay {
    pipeline: gst::Pipeline,
    config: DisplayConfig,
}

impl GstDisplay {
    /// Create a complete GStreamer pipeline from camera to display
    pub fn new_pipeline(
        capture_config: &CaptureConfig,
        display_config: &DisplayConfig,
    ) -> Result<Self> {
        // Initialize GStreamer
        gst::init().map_err(|e| eyre!("Failed to initialize GStreamer: {}", e))?;

        info!("Creating complete GStreamer pipeline for optimal performance");

        // Build the complete pipeline string
        let pipeline_str = Self::build_complete_pipeline(capture_config, display_config)?;
        info!("Pipeline: {}", pipeline_str);

        // Create pipeline
        let pipeline = gst::parse::launch(&pipeline_str)?
            .downcast::<gst::Pipeline>()
            .map_err(|_| eyre!("Failed to create pipeline"))?;

        Ok(Self {
            pipeline,
            config: display_config.clone(),
        })
    }

    /// Build optimized complete pipeline string
    fn build_complete_pipeline(capture: &CaptureConfig, display: &DisplayConfig) -> Result<String> {
        let device = &capture.device.path;
        let width = capture.width;
        let height = capture.height;
        let fps = capture.fps;

        // Detect best video sink
        let video_sink = Self::detect_video_sink();
        info!("Using video sink: {}", video_sink);

        // Detect hardware decoders if needed
        let jpeg_decoder = if capture.format == crate::capture::frame::PixelFormat::Mjpeg {
            Self::detect_jpeg_decoder()
        } else {
            ""
        };

        // Build pipeline based on format
        let pipeline = match capture.format {
            crate::capture::frame::PixelFormat::Mjpeg => {
                // MJPEG pipeline with hardware decoder and display
                format!(
                    "v4l2src device={} name=source ! \
                     image/jpeg,width={},height={},framerate={}/1 ! \
                     queue max-size-buffers=2 leaky=downstream ! \
                     {} ! \
                     videoconvert ! \
                     videoscale ! \
                     video/x-raw,width={},height={} ! \
                     {}",
                    device,
                    width,
                    height,
                    fps,
                    jpeg_decoder,
                    display.width,
                    display.height,
                    Self::build_video_sink(video_sink, display.width, display.height)
                )
            }
            crate::capture::frame::PixelFormat::Yuyv4 => {
                // YUYV pipeline with hardware conversion
                format!(
                    "v4l2src device={} name=source ! \
                     video/x-raw,format=YUY2,width={},height={},framerate={}/1 ! \
                     queue max-size-buffers=2 leaky=downstream ! \
                     videoconvert ! \
                     videoscale ! \
                     video/x-raw,width={},height={} ! \
                     {}",
                    device,
                    width,
                    height,
                    fps,
                    display.width,
                    display.height,
                    Self::build_video_sink(video_sink, display.width, display.height)
                )
            }
            crate::capture::frame::PixelFormat::Rgb24 => {
                // Direct RGB pipeline (fastest)
                format!(
                    "v4l2src device={} name=source ! \
                     video/x-raw,format=RGB,width={},height={},framerate={}/1 ! \
                     queue max-size-buffers=2 leaky=downstream ! \
                     videoscale ! \
                     video/x-raw,width={},height={} ! \
                     {}",
                    device,
                    width,
                    height,
                    fps,
                    display.width,
                    display.height,
                    Self::build_video_sink(video_sink, display.width, display.height)
                )
            }
            _ => return Err(eyre!("Unsupported pixel format")),
        };

        Ok(pipeline)
    }

    /// Detect best available video sink (hardware accelerated > software)
    fn detect_video_sink() -> &'static str {
        // Check for video sinks in order of preference
        let sinks = [
            "glimagesink",   // OpenGL (hardware accelerated)
            "waylandsink",   // Wayland native (if available)
            "xvimagesink",   // X11 with XVideo extension
            "ximagesink",    // X11 basic
            "autovideosink", // Auto-detect
        ];

        for sink in &sinks {
            if let Some(factory) = gst::ElementFactory::find(sink) {
                info!(
                    "Found video sink: {} - {}",
                    sink,
                    factory.metadata("long-name").unwrap_or("")
                );
                return sink;
            }
        }

        warn!("Using auto video sink");
        "autovideosink"
    }

    /// Detect best available JPEG decoder
    fn detect_jpeg_decoder() -> &'static str {
        let decoders = [
            "nvjpegdec",    // NVIDIA hardware decoder
            "vaapijpegdec", // Intel/AMD VAAPI hardware decoder
            "v4l2jpegdec",  // V4L2 hardware decoder
            "jpegdec",      // Software decoder (fallback)
        ];

        for decoder in &decoders {
            if gst::ElementFactory::find(decoder).is_some() {
                info!("Using JPEG decoder: {}", decoder);
                return decoder;
            }
        }

        "jpegdec"
    }

    /// Build video sink with proper window sizing and FPS display
    fn build_video_sink(sink_name: &str, width: u32, height: u32) -> String {
        // Create the base sink with proper sizing
        // Note: We add name=videosink to allow programmatic access
        let base_sink = match sink_name {
            "glimagesink" => {
                // OpenGL sink - best performance
                // Use render-rectangle to set the viewport size
                format!("glimagesink name=videosink force-aspect-ratio=false render-rectangle=\"<0,0,{},{}>\"", 
                       width, height)
            }
            "xvimagesink" => {
                // XVideo sink - good performance
                // Will be resized programmatically after pipeline creation
                "xvimagesink name=videosink force-aspect-ratio=false".to_string()
            }
            "ximagesink" => {
                // Basic X11 sink - will resize window programmatically
                "ximagesink name=videosink force-aspect-ratio=false".to_string()
            }
            "waylandsink" => {
                // Wayland native sink with window sizing
                format!("waylandsink name=videosink fullscreen=false")
            }
            _ => {
                // Auto sink will pick the best available
                "autovideosink name=videosink".to_string()
            }
        };

        // Wrap in fpsdisplaysink for FPS overlay
        format!(
            "fpsdisplaysink video-sink=\"{}\" text-overlay=true sync=false",
            base_sink
        )
    }

    /// Start the display pipeline
    pub fn start(&mut self) -> Result<()> {
        info!("Starting GStreamer display pipeline");

        // Set window properties if we can access the video sink
        if let Some(sink) = self.pipeline.by_name("videosink") {
            // Try to set window size properties
            if sink.has_property("window-width", None) {
                sink.set_property("window-width", self.config.width as i32);
            }
            if sink.has_property("window-height", None) {
                sink.set_property("window-height", self.config.height as i32);
            }
            if sink.has_property("force-aspect-ratio", None) {
                sink.set_property("force-aspect-ratio", false);
            }
        }

        // Set pipeline to playing state
        self.pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| eyre!("Failed to start pipeline: {:?}", e))?;

        Ok(())
    }

    /// Run the display pipeline (blocking)
    pub fn run(&mut self) -> Result<()> {
        self.start()?;

        // Get the bus
        let bus = self
            .pipeline
            .bus()
            .ok_or_else(|| eyre!("Pipeline has no bus"))?;

        // Main message loop
        for msg in bus.iter_timed(gst::ClockTime::NONE) {
            use gst::MessageView;

            match msg.view() {
                MessageView::Eos(..) => {
                    info!("End of stream");
                    break;
                }
                MessageView::Error(err) => {
                    return Err(eyre!(
                        "Error from {:?}: {} ({:?})",
                        err.src().map(|s| s.path_string()),
                        err.error(),
                        err.debug()
                    ));
                }
                MessageView::Warning(warning) => {
                    warn!(
                        "Warning from {:?}: {} ({:?})",
                        warning.src().map(|s| s.path_string()),
                        warning.error(),
                        warning.debug()
                    );
                }
                MessageView::Info(info) => {
                    info!(
                        "Info from {:?}: {} ({:?})",
                        info.src().map(|s| s.path_string()),
                        info.error(),
                        info.debug()
                    );
                }
                _ => {}
            }
        }

        self.stop()?;
        Ok(())
    }

    /// Stop the display pipeline
    pub fn stop(&mut self) -> Result<()> {
        info!("Stopping GStreamer display pipeline");

        self.pipeline
            .set_state(gst::State::Null)
            .map_err(|e| eyre!("Failed to stop pipeline: {:?}", e))?;

        Ok(())
    }

    /// Get pipeline statistics
    pub fn get_stats(&self) -> DisplayStats {
        let position = self.pipeline.query_position::<gst::ClockTime>();

        // Query latency using the latency query
        let mut query = gst::query::Latency::new();
        let latency_ms = if self.pipeline.query(query.query_mut()) {
            let (_, max, _) = query.result();
            max.mseconds()
        } else {
            0
        };

        DisplayStats {
            position: position.map(|p| p.mseconds()),
            latency: latency_ms,
            state: format!("{:?}", self.pipeline.current_state()),
        }
    }
}

impl Drop for GstDisplay {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Display statistics for monitoring
#[derive(Debug)]
pub struct DisplayStats {
    pub position: Option<u64>,
    pub latency: u64,
    pub state: String,
}

/// Alternative: GStreamer display sink that receives frames from Rust
/// This is useful if you want to process frames in Rust before display
pub struct GstFrameDisplay {
    pipeline: gst::Pipeline,
    appsrc: gstreamer_app::AppSrc,
}

impl GstFrameDisplay {
    /// Create a display pipeline that accepts frames from Rust
    pub fn new(config: &DisplayConfig) -> Result<Self> {
        gst::init().map_err(|e| eyre!("Failed to initialize GStreamer: {}", e))?;

        let video_sink = Self::detect_video_sink();

        // Build pipeline for receiving RGB frames
        let pipeline_str = format!(
            "appsrc name=appsrc caps=video/x-raw,format=RGB,width={},height={},framerate=30/1 ! \
             videoconvert ! \
             videoscale ! \
             fpsdisplaysink video-sink=\"{}\" sync=false",
            config.width, config.height, video_sink
        );

        let pipeline = gst::parse::launch(&pipeline_str)?
            .downcast::<gst::Pipeline>()
            .map_err(|_| eyre!("Failed to create pipeline"))?;

        let appsrc = pipeline
            .by_name("appsrc")
            .ok_or_else(|| eyre!("Failed to find appsrc"))?
            .downcast::<gstreamer_app::AppSrc>()
            .map_err(|_| eyre!("Failed to cast to AppSrc"))?;

        // Configure appsrc
        appsrc.set_property("is-live", true);
        appsrc.set_property("block", false);
        appsrc.set_property("format", gst::Format::Time);

        Ok(Self { pipeline, appsrc })
    }

    /// Push a frame to the display
    pub fn push_frame(&self, data: &[u8], width: u32, height: u32) -> Result<()> {
        let size = (width * height * 3) as usize;
        if data.len() != size {
            return Err(eyre!("Invalid frame size"));
        }

        // Create GStreamer buffer
        let mut buffer =
            gst::Buffer::with_size(size).map_err(|_| eyre!("Failed to allocate buffer"))?;

        {
            let buffer_ref = buffer.make_mut();
            buffer_ref
                .copy_from_slice(0, data)
                .map_err(|_| eyre!("Failed to copy data to buffer"))?;
        }

        // Push buffer to appsrc
        self.appsrc
            .push_buffer(buffer)
            .map_err(|_| eyre!("Failed to push buffer"))?;

        Ok(())
    }

    fn detect_video_sink() -> &'static str {
        let sinks = [
            "glimagesink",
            "waylandsink",
            "xvimagesink",
            "ximagesink",
            "autovideosink",
        ];

        for sink in &sinks {
            if gst::ElementFactory::find(sink).is_some() {
                return sink;
            }
        }

        "autovideosink"
    }

    pub fn start(&mut self) -> Result<()> {
        self.pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| eyre!("Failed to start pipeline: {:?}", e))?;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        self.pipeline
            .set_state(gst::State::Null)
            .map_err(|e| eyre!("Failed to stop pipeline: {:?}", e))?;
        Ok(())
    }
}
