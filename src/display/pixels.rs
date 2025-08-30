//! Pixels-based display for simple and reliable rendering

use std::sync::Arc;
use std::time::Instant;

use color_eyre::{eyre::eyre, Result};
use pixels::{Pixels, SurfaceTexture};
use tracing::{info, instrument};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::{DisplayConfig, Frame, PixelFormat};

/// Simple pixels-based display
pub struct PixelsDisplay {
    _config: DisplayConfig,
    pub window: Arc<Window>,
    pixels: Box<Pixels>,
}

impl PixelsDisplay {
    /// Initialize pixels display
    #[instrument(skip(window, config))]
    pub fn new(window: Arc<Window>, config: DisplayConfig) -> Result<Self> {
        info!("Initializing pixels display");

        let window_size = window.inner_size();
        let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, &*window);
        let pixels = Box::new(Pixels::new(config.width, config.height, surface_texture)?);

        Ok(Self {
            _config: config,
            window,
            pixels,
        })
    }

    /// Display frame by copying to pixels buffer
    #[instrument(skip(self, frame))]
    pub fn display_frame(&mut self, frame: &Frame) -> Result<()> {
        let render_start = Instant::now();

        // Get pixels buffer
        let buffer = self.pixels.frame_mut();

        // Convert frame data to RGBA format
        let rgba_data = match frame.meta.format {
            PixelFormat::Mjpeg => {
                // Decode MJPEG
                let data_slice = &frame.data[..];
                let mut decoder = zune_jpeg::JpegDecoder::new(data_slice);
                let pixels = decoder.decode()?;

                // Convert RGB to RGBA
                let mut rgba = Vec::with_capacity(pixels.len() * 4 / 3);
                for chunk in pixels.chunks(3) {
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
            _ => return Err(eyre!("Unsupported pixel format for pixels display")),
        };

        // Copy to pixels buffer (assuming the decoded image fits)
        let copy_len = buffer.len().min(rgba_data.len());
        buffer[..copy_len].copy_from_slice(&rgba_data[..copy_len]);

        // Render to screen
        if let Err(e) = self.pixels.render() {
            return Err(eyre!("Pixels render error: {}", e));
        }

        let render_time = render_start.elapsed();
        metrics::histogram!("render_time_us").record(render_time.as_micros() as f64);

        Ok(())
    }

    /// Resize the display
    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        if let Err(e) = self.pixels.resize_surface(width, height) {
            return Err(eyre!("Failed to resize pixels surface: {}", e));
        }
        Ok(())
    }
}

/// Application handler for winit event loop
pub struct PixelsApp {
    display: Option<PixelsDisplay>,
    config: DisplayConfig,
    frame_receiver: flume::Receiver<Frame>,
}

impl PixelsApp {
    pub fn new(config: DisplayConfig, frame_receiver: flume::Receiver<Frame>) -> Self {
        Self {
            display: None,
            config,
            frame_receiver,
        }
    }
}

impl ApplicationHandler for PixelsApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.display.is_none() {
            let window_attrs = Window::default_attributes()
                .with_title("Apollo Video Pipeline")
                .with_inner_size(LogicalSize::new(self.config.width, self.config.height))
                .with_fullscreen(if self.config.fullscreen {
                    Some(winit::window::Fullscreen::Borderless(None))
                } else {
                    None
                });

            let window = Arc::new(event_loop.create_window(window_attrs).unwrap());
            let display = PixelsDisplay::new(window, self.config.clone()).unwrap();
            self.display = Some(display);
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
                if let Some(ref mut display) = self.display {
                    // Get latest frame
                    if let Ok(frame) = self.frame_receiver.try_recv() {
                        let latency = frame.timestamp.elapsed();
                        metrics::histogram!("frame_latency_ms").record(latency.as_millis() as f64);

                        if let Err(e) = display.display_frame(&frame) {
                            tracing::error!("Display error: {}", e);
                        }
                    }
                }
            }
            WindowEvent::Resized(new_size) => {
                if let Some(ref mut display) = self.display {
                    if let Err(e) = display.resize(new_size.width, new_size.height) {
                        tracing::error!("Resize error: {}", e);
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(ref display) = self.display {
            display.window.request_redraw();
        }
    }
}

/// Run the pixels display event loop
pub fn run_pixels_display(
    config: DisplayConfig,
    frame_receiver: flume::Receiver<Frame>,
) -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = PixelsApp::new(config, frame_receiver);

    event_loop.run_app(&mut app)?;
    Ok(())
}
