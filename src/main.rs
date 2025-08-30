//! Apollo Video Pipeline with GStreamer or SDL2 Display

use std::sync::Arc;

use apollo::Config;
use color_eyre::Result;
use tracing::{error, info};

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

    #[cfg(feature = "gstreamer-pipeline")]
    {
        run_gstreamer_pipeline_main(config).await
    }

    #[cfg(not(feature = "gstreamer-pipeline"))]
    {
        run_legacy_pipeline(config).await
    }
}

#[cfg(feature = "gstreamer-pipeline")]
async fn run_gstreamer_pipeline(config: Config) -> Result<()> {
    info!("Running high-performance GStreamer pipeline");

    // Auto-detect capture device if needed
    let mut capture_config = config.capture.clone();
    if capture_config.device.path.is_empty() {
        let device = apollo::utils::auto_detect_device().await?;
        capture_config.device = device;
    }

    info!("Using capture device: {:?}", capture_config.device);

    // Run the simple pipeline that should size the window correctly
    match run_gstreamer_pipeline(&capture_config, &config.display) {
        Ok(_) => info!("Pipeline completed successfully"),
        Err(e) => error!("Pipeline error: {}", e),
    }

    info!("Apollo shutting down");
    Ok(())
}
