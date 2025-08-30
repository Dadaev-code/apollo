//! Apollo Video Pipeline with SDL2 and Camera Integration

use std::sync::Arc;
use std::time::Duration;

use apollo::{capture, Config, DisplayConfig};
use color_eyre::{eyre::eyre, Result};
use flume::bounded;
use tracing::{error, info};

use apollo::display::Sdl2Display;
use apollo::utils;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize error handling and logging
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter("apollo=debug")
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .init();

    info!("Apollo Launching...");

    // Load configuration
    let config = Config::default();
    apollo::CONFIG.store(Arc::new(config.clone()));

    // Auto-detect capture device if needed
    let device = if config.capture.device.path.is_empty() {
        utils::auto_detect_device().await?
    } else {
        config.capture.device.clone()
    };

    info!("Using capture device: {:?}", device);

    // Initialize capture
    let mut capture_config = config.capture;
    capture_config.device = device;
    let mut capture = capture::V4l2Capture::new(capture_config)?;
    capture.start_stream()?;

    // Set up tx/rx
    let (tx, rx) = bounded::<capture::Frame>(config.pipeline.ring_buffer_size);

    // Spawn capture task
    let _capture_handle = tokio::spawn(async move {
        loop {
            match capture.capture_frame().await {
                Ok(frame) => {
                    if let Err(e) = tx.send_async(frame).await {
                        error!("Failed to send frame: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    error!("Capture error: {}", e);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
    });

    // Set up display configuration
    let display_config = DisplayConfig {
        width: config.display.width,
        height: config.display.height,
    };

    // Initialize SDL2
    let sdl_context = sdl2::init().map_err(|e| eyre!(e))?;

    // Get display dimensions
    let width = display_config.width;
    let height = display_config.height;

    // Create and run video app
    let mut app = Sdl2Display::new(&sdl_context, width, height)?;
    app.run(&sdl_context, rx)?;

    info!("Apollo shutting down");
    Ok(())
}
