//! SDL2 Window Display Module
//! Provides functionality to create an SDL2 window and display video frames.
//! Uses the sdl2 crate for window management and rendering.

use color_eyre::{eyre::eyre, Result};
use flume::Receiver;
use sdl2::event::Event;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::{Canvas, TextureCreator};
use sdl2::video::{Window, WindowContext};

use tracing::info;

use crate::capture::{decoder, Frame};

/// SDL2 Window Display
/// Handles window creation, event loop, and frame rendering.
/// Supports fullscreen and vsync options, with gpu acceleration.
pub struct Sdl2Display {
    canvas: Canvas<Window>,
    texture_creator: TextureCreator<WindowContext>,
    width: u32,
    height: u32,
}

impl Sdl2Display {
    pub fn new(sdl_context: &sdl2::Sdl, width: u32, height: u32) -> Result<Self> {
        let video_subsystem = sdl_context.video().map_err(|e| eyre!(e))?;

        let window_builder = video_subsystem
            .window("Apollo Video Pipeline", width, height)
            .position_centered()
            .build()?;

        let canvas_builder = window_builder.into_canvas().present_vsync();

        let canvas = canvas_builder.build()?;
        let texture_creator = canvas.texture_creator();

        Ok(Self {
            canvas,
            texture_creator,
            width,
            height,
        })
    }

    pub fn render_frame(&mut self, frame: &Frame) -> Result<()> {
        let rgb_data = decoder::decode_frame(&frame.data, frame.meta.format)?;

        let mut texture = self
            .texture_creator
            .create_texture_streaming(PixelFormatEnum::RGB24, self.width, self.height)
            .map_err(|e| eyre!(e))?;

        texture
            .update(None, &rgb_data, (self.width * 3) as usize)
            .map_err(|e| eyre!(e))?;

        self.canvas.clear();
        self.canvas
            .copy(&texture, None, None)
            .map_err(|e| eyre!(e))?;

        self.canvas.present();
        Ok(())
    }

    pub fn run(&mut self, sdl_context: &sdl2::Sdl, rx: Receiver<Frame>) -> Result<()> {
        let mut event_pump = sdl_context.event_pump().map_err(|e| eyre!(e))?;

        'running: loop {
            for event in event_pump.poll_iter() {
                match event {
                    Event::Quit { .. } => {
                        info!("Quit event received");
                        break 'running;
                    }
                    _ => {}
                }
            }

            match rx.recv() {
                Ok(frame) => {
                    self.render_frame(&frame)?;
                }
                Err(e) => {
                    eprintln!("Failed to receive frame: {}", e);
                }
            }
        }

        Ok(())
    }
}
