//! Apollo Video Pipeline with Pixels and Camera Integration

use std::sync::Arc;
use std::time::{Duration, Instant};

use apollo::{capture, Config, Frame, PixelFormat};
use color_eyre::Result;
use flume::bounded;
use pixels::{Pixels, SurfaceTexture};
use tracing::{error, info, warn};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

struct DisplayConfig {
    width: u32,
    height: u32,
}

struct VideoApp {
    window: Option<Arc<Window>>,
    config: DisplayConfig,
    frame_receiver: flume::Receiver<Frame>,
}

impl VideoApp {
    fn new(config: DisplayConfig, frame_receiver: flume::Receiver<Frame>) -> Self {
        Self {
            window: None,
            config,
            frame_receiver,
        }
    }
}

impl ApplicationHandler for VideoApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window_attrs = Window::default_attributes()
                .with_title("Apollo Video Pipeline")
                .with_inner_size(LogicalSize::new(self.config.width, self.config.height));

            let window = Arc::new(event_loop.create_window(window_attrs).unwrap());
            self.window = Some(window);
            info!("Window created successfully");
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                info!("Window close requested");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if let Some(ref window) = self.window {
                    // Try to get a frame from the camera
                    match self.frame_receiver.try_recv() {
                        Ok(frame) => {
                            let latency = frame.timestamp.elapsed();
                            metrics::histogram!("frame_latency_ms")
                                .record(latency.as_millis() as f64);

                            if let Err(e) = self.display_frame(window, &frame) {
                                error!("Display error: {}", e);
                            }
                        }
                        Err(_) => {
                            // No frame available, show test pattern or previous frame
                            if let Err(e) = self.display_test_pattern(window) {
                                error!("Test pattern display error: {}", e);
                            }
                        }
                    }
                }
            }
            WindowEvent::Resized(_new_size) => {
                // Window resizing will be handled by creating new pixels instance
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }
}

impl VideoApp {
    fn display_frame(&self, window: &Window, frame: &Frame) -> Result<()> {
        let window_size = window.inner_size();
        let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, window);

        let mut pixels = Pixels::new(self.config.width, self.config.height, surface_texture)?;
        let buffer = pixels.frame_mut();

        // Convert frame data to RGBA format
        let rgba_data = match frame.meta.format {
            PixelFormat::Mjpeg => {
                // Decode MJPEG
                let data_slice = &frame.data[..];
                let mut decoder = zune_jpeg::JpegDecoder::new(data_slice);
                let decoded_pixels = decoder.decode()?;

                // Convert RGB to RGBA
                let mut rgba = Vec::with_capacity(decoded_pixels.len() * 4 / 3);
                for chunk in decoded_pixels.chunks(3) {
                    if chunk.len() == 3 {
                        rgba.push(chunk[0]); // R
                        rgba.push(chunk[1]); // G
                        rgba.push(chunk[2]); // B
                        rgba.push(255); // A
                    }
                }
                rgba
            }
            PixelFormat::Rgb24 => {
                // Convert RGB to RGBA
                let mut rgba = Vec::with_capacity(frame.data.len() * 4 / 3);
                for chunk in frame.data.chunks(3) {
                    if chunk.len() == 3 {
                        rgba.push(chunk[0]); // R
                        rgba.push(chunk[1]); // G
                        rgba.push(chunk[2]); // B
                        rgba.push(255); // A
                    }
                }
                rgba
            }
            _ => {
                warn!(
                    "Unsupported pixel format: {:?}, showing test pattern",
                    frame.meta.format
                );
                return self.display_test_pattern(window);
            }
        };

        // Copy to pixels buffer
        let copy_len = buffer.len().min(rgba_data.len());
        buffer[..copy_len].copy_from_slice(&rgba_data[..copy_len]);

        // Render to screen
        pixels.render()?;

        Ok(())
    }

    fn display_test_pattern(&self, window: &Window) -> Result<()> {
        let window_size = window.inner_size();
        let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, window);

        let mut pixels = Pixels::new(self.config.width, self.config.height, surface_texture)?;

        // Create simple test pattern
        let frame = pixels.frame_mut();
        for (i, pixel) in frame.chunks_exact_mut(4).enumerate() {
            let x = (i % self.config.width as usize) as u8;
            let y = (i / self.config.width as usize) as u8;

            pixel[0] = x; // R
            pixel[1] = y; // G
            pixel[2] = 128; // B
            pixel[3] = 255; // A
        }

        pixels.render()?;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize error handling and logging
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter("apollo=debug,wgpu=warn")
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .init();

    info!("🚀 Apollo Video Pipeline Starting");

    // Load configuration
    let config = Config::default();
    apollo::CONFIG.store(Arc::new(config.clone()));

    // Auto-detect capture device if needed
    let device = if config.capture.device == "auto" {
        capture::v4l2::auto_detect_device().await?
    } else {
        config.capture.device.clone()
    };

    info!("Using capture device: {}", device);

    // Create channels for pipeline
    let (capture_tx, capture_rx) = bounded::<Frame>(config.pipeline.ring_buffer_size);
    let (decode_tx, decode_rx) = bounded::<Frame>(config.pipeline.ring_buffer_size);

    // Initialize capture
    let mut capture_config = config.capture.clone();
    capture_config.device = device;
    let mut capture = capture::v4l2::V4l2Capture::new(capture_config)?;
    capture.start_stream()?;

    // Spawn capture task
    let _capture_handle = tokio::spawn(async move {
        let mut frame_count = 0u64;
        let mut last_report = Instant::now();

        loop {
            match capture.capture_frame().await {
                Ok(frame) => {
                    frame_count += 1;

                    // Report FPS every second
                    if last_report.elapsed() >= Duration::from_secs(1) {
                        let fps = frame_count as f64 / last_report.elapsed().as_secs_f64();
                        info!("Capture FPS: {:.1}", fps);
                        metrics::gauge!("capture_fps").set(fps);

                        frame_count = 0;
                        last_report = Instant::now();
                    }

                    // Send frame to decode pipeline
                    if capture_tx.send(frame).is_err() {
                        warn!("Decode pipeline full, dropping frame");
                        metrics::counter!("dropped_frames").increment(1);
                    }
                }
                Err(e) => {
                    error!("Capture error: {}", e);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
    });

    // Spawn decode task (MJPEG passthrough for now)
    let _decode_handle = tokio::spawn(async move {
        while let Ok(frame) = capture_rx.recv_async().await {
            // Pass through frames directly to display
            if decode_tx.send(frame).is_err() {
                warn!("Display pipeline full");
            }
        }
    });

    // Set up display configuration
    let display_config = DisplayConfig {
        width: config.display.width,
        height: config.display.height,
    };

    // Run video display in main thread (required for winit)
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = VideoApp::new(display_config, decode_rx);
    event_loop.run_app(&mut app)?;

    info!("Apollo shutting down");
    Ok(())
}
