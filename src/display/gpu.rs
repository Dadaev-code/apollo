//! WebGPU-based display with zero-copy texture upload

use std::sync::Arc;
use std::time::Instant;

use color_eyre::{eyre::eyre, Result};
use tracing::{info, instrument};
use wgpu::*;
use winit::event_loop::EventLoop;
use winit::window::Window;

use crate::{DisplayConfig, Frame, PixelFormat};

/// GPU-accelerated display using WebGPU
pub struct GpuDisplay {
    _config: DisplayConfig,
    device: Device,
    queue: Queue,
    surface: Surface<'static>,
    texture: Option<Texture>,
    pipeline: RenderPipeline,
    pub window: Arc<Window>,
}

impl GpuDisplay {
    /// Initialize WebGPU display
    #[instrument(skip(event_loop, config))]
    pub async fn new(event_loop: &EventLoop<()>, config: DisplayConfig) -> Result<Self> {
        info!("Initializing WebGPU display");

        // Create window
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("Apollo")
                    .with_inner_size(winit::dpi::PhysicalSize::new(config.width, config.height))
                    .with_fullscreen(if config.fullscreen {
                        Some(winit::window::Fullscreen::Borderless(None))
                    } else {
                        None
                    }),
            )?,
        );

        // Initialize WebGPU
        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone())?;

        // Get adapter - prefer high-performance
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| eyre!("No suitable GPU adapter found"))?;

        info!("GPU: {}", adapter.get_info().name);

        // Create device and queue
        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    label: Some("Apollo GPU Device"),
                    required_features: Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
                    required_limits: Limits::default(),
                    memory_hints: Default::default(),
                },
                None,
            )
            .await?;

        // Configure surface
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: config.width,
            height: config.height,
            present_mode: if config.vsync {
                PresentMode::AutoVsync
            } else {
                PresentMode::Fifo
            },
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 1,
        };

        surface.configure(&device, &surface_config);

        // Create render pipeline
        let pipeline = Self::create_render_pipeline(&device, surface_format)?;

        Ok(Self {
            _config: config,
            device,
            queue,
            surface,
            texture: None,
            pipeline,
            window,
        })
    }

    /// Display frame with zero-copy texture upload
    #[instrument(skip(self, frame))]
    pub fn display_frame(&mut self, frame: &Frame) -> Result<()> {
        let render_start = Instant::now();

        // Create or update texture
        if self.texture.is_none() {
            self.texture = Some(self.create_texture(frame)?);
        }

        let texture = self.texture.as_ref().unwrap();

        // Upload frame data to GPU
        self.upload_frame_data(texture, frame)?;

        // Render
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color::BLACK),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            render_pass.set_pipeline(&self.pipeline);
            // Bind texture and render
            render_pass.draw(0..3, 0..1); // Fullscreen triangle
        }

        // Submit commands
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        let render_time = render_start.elapsed();
        metrics::histogram!("render_time_us").record(render_time.as_micros() as f64);

        Ok(())
    }

    fn create_texture(&self, frame: &Frame) -> Result<Texture> {
        let size = Extent3d {
            width: frame.meta.width,
            height: frame.meta.height,
            depth_or_array_layers: 1,
        };

        let texture = self.device.create_texture(&TextureDescriptor {
            label: Some("Frame Texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        Ok(texture)
    }

    fn upload_frame_data(&self, texture: &Texture, frame: &Frame) -> Result<()> {
        // Decode MJPEG if needed
        let rgba_data = match frame.meta.format {
            PixelFormat::Mjpeg => {
                // Use zune-jpeg for fastest JPEG decoding
                // Convert Bytes to slice for decoder
                let data_slice = &frame.data[..];
                let mut decoder = zune_jpeg::JpegDecoder::new(data_slice);
                let pixels = decoder.decode()?;
                // For now, assume JPEG is RGB and convert to RGBA
                let mut rgba = Vec::with_capacity(pixels.len() * 4 / 3);
                for chunk in pixels.chunks(3) {
                    if chunk.len() == 3 {
                        rgba.push(chunk[0]);
                        rgba.push(chunk[1]);
                        rgba.push(chunk[2]);
                        rgba.push(255);
                    }
                }
                rgba
            }
            PixelFormat::Rgb24 => {
                // Convert RGB to RGBA
                let mut rgba = Vec::with_capacity(frame.data.len() * 4 / 3);
                for chunk in frame.data.chunks(3) {
                    if chunk.len() == 3 {
                        rgba.push(chunk[0]);
                        rgba.push(chunk[1]);
                        rgba.push(chunk[2]);
                        rgba.push(255);
                    }
                }
                rgba
            }
            _ => return Err(eyre!("Unsupported pixel format")),
        };

        // Upload to GPU
        self.queue.write_texture(
            ImageCopyTexture {
                texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &rgba_data,
            ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * frame.meta.width),
                rows_per_image: Some(frame.meta.height),
            },
            Extent3d {
                width: frame.meta.width,
                height: frame.meta.height,
                depth_or_array_layers: 1,
            },
        );

        Ok(())
    }

    fn create_render_pipeline(device: &Device, format: TextureFormat) -> Result<RenderPipeline> {
        // Simple shader that renders a fullscreen triangle
        let shader_source = r#"
            @vertex
            fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
                // Fullscreen triangle trick
                let x = f32(i32(vertex_index) - 1);
                let y = f32(i32(vertex_index & 1u) * 2 - 1);
                return vec4<f32>(x, y, 0.0, 1.0);
            }
             
            @fragment
            fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
                // For now, just output a test pattern
                let r = position.x / 1920.0;
                let g = position.y / 1080.0;
                return vec4<f32>(r, g, 0.5, 1.0);
            }
        "#;

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Display Shader"),
            source: ShaderSource::Wgsl(shader_source.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Display Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Display Pipeline"),
            layout: Some(&pipeline_layout),
            cache: None,
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(BlendState::REPLACE),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
        });

        Ok(pipeline)
    }
}
