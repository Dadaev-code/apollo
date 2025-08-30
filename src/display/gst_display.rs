//! Simplified GStreamer display that ensures proper window sizing

use color_eyre::{eyre::eyre, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use tracing::info;

use crate::{CaptureConfig, DisplayConfig};

/// Create and run a simple, properly-sized GStreamer pipeline
pub fn run_gstreamer_pipeline(
    capture_config: &CaptureConfig,
    display_config: &DisplayConfig,
) -> Result<()> {
    // Initialize GStreamer
    gst::init().map_err(|e| eyre!("Failed to initialize GStreamer: {}", e))?;

    info!(
        "Creating GStreamer pipeline with window size {}x{}",
        display_config.width, display_config.height
    );

    // Build a simpler pipeline that ensures window sizing
    let pipeline_str = build_sized_pipeline(capture_config, display_config)?;
    info!("Pipeline: {}", pipeline_str);

    // Create and run the pipeline
    let pipeline = gst::parse::launch(&pipeline_str)?;

    // Set to PLAYING
    pipeline
        .set_state(gst::State::Playing)
        .map_err(|_| eyre!("Failed to start pipeline"))?;

    // Wait for EOS or error
    let bus = pipeline.bus().unwrap();
    for msg in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView;

        match msg.view() {
            MessageView::Eos(..) => break,
            MessageView::Error(err) => {
                pipeline.set_state(gst::State::Null).ok();
                return Err(eyre!(
                    "Pipeline error: {} ({})",
                    err.error(),
                    err.debug().unwrap_or_default()
                ));
            }
            _ => {}
        }
    }

    pipeline
        .set_state(gst::State::Null)
        .map_err(|_| eyre!("Failed to stop pipeline"))?;

    Ok(())
}

fn build_sized_pipeline(capture: &CaptureConfig, display: &DisplayConfig) -> Result<String> {
    let device = &capture.device.path;

    // Detect the best decoder
    let decoder = detect_best_decoder(capture);

    // Build pipeline ensuring proper window size
    // The key is to make sure we scale to the desired display size
    let pipeline = match capture.format {
        crate::capture::frame::PixelFormat::Mjpeg => {
            format!(
                "v4l2src device={} ! \
                 image/jpeg,width={},height={},framerate={}/1 ! \
                 queue ! \
                 {} ! \
                 videoconvert ! \
                 videoscale ! \
                 video/x-raw,width={},height={} ! \
                 fpsdisplaysink video-sink=\"xvimagesink force-aspect-ratio=false\" sync=false",
                device,
                capture.width,
                capture.height,
                capture.fps,
                decoder,
                display.width,
                display.height
            )
        }
        _ => {
            format!(
                "v4l2src device={} ! \
                 video/x-raw,width={},height={},framerate={}/1 ! \
                 queue ! \
                 videoconvert ! \
                 videoscale ! \
                 video/x-raw,width={},height={} ! \
                 fpsdisplaysink video-sink=\"xvimagesink force-aspect-ratio=false\" sync=false",
                device, capture.width, capture.height, capture.fps, display.width, display.height
            )
        }
    };

    Ok(pipeline)
}

fn detect_best_decoder(capture: &CaptureConfig) -> &'static str {
    if capture.format != crate::capture::frame::PixelFormat::Mjpeg {
        return "";
    }

    // Try hardware decoders first
    let decoders = ["nvjpegdec", "vaapijpegdec", "v4l2jpegdec", "jpegdec"];

    for decoder in &decoders {
        if gst::ElementFactory::find(decoder).is_some() {
            info!("Using decoder: {}", decoder);
            return decoder;
        }
    }

    "jpegdec"
}
