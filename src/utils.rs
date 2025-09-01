use crate::capture::frame::PixelFormat;
use color_eyre::{eyre::eyre, Result};
use serde::{Deserialize, Serialize};
use tracing::info;
use v4l::{capability::Flags, video::Capture, Device, FourCC};

// Detected capture device info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoundDevice {
    pub path: String,
    pub format: PixelFormat,
}

impl FoundDevice {
    pub fn new(path: String, format: PixelFormat) -> Self {
        Self { path, format }
    }
}

/// Auto-detect best capture device
pub async fn auto_detect_device() -> Result<FoundDevice> {
    use std::path::Path;

    info!("Auto-detecting capture devices...");

    for i in 0..10 {
        let path = format!("/dev/video{}", i);
        if !Path::new(&path).exists() {
            continue;
        }

        if let Ok(dev) = Device::with_path(&path) {
            if let Ok(caps) = dev.query_caps() {
                // Check for capture capability
                if caps.capabilities.contains(Flags::VIDEO_CAPTURE) {
                    // Prefer devices with MJPEG support
                    if let Ok(formats) = dev.enum_formats() {
                        for fmt in formats {
                            if fmt.fourcc == FourCC::new(b"MJPG") {
                                info!("Found MJPEG device: {} - {}", path, caps.card);
                                return Ok(FoundDevice {
                                    path,
                                    format: PixelFormat::Mjpeg,
                                });
                            } else if fmt.fourcc == FourCC::new(b"YUYV") {
                                info!("Found YUYV device: {} - {}", path, caps.card);
                                return Ok(FoundDevice {
                                    path,
                                    format: PixelFormat::Yuyv4,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    Err(eyre!("No suitable capture device found"))
}
