use vello::{AaConfig, AaSupport, RenderParams, RendererOptions, Scene, peniko::ImageData, wgpu};

use crate::{CanvasGpu, Error, PhysicalSize, Result, state::Color};

/// The single GPU image rendered by Vello and presented by the desktop host.
struct RenderTarget {
    size: PhysicalSize,
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl RenderTarget {
    fn new(device: &wgpu::Device, requested: PhysicalSize) -> Self {
        // WGPU does not permit zero-sized textures. The public frame still
        // reports the requested zero size and Canvas skips all rendering.
        let size = PhysicalSize::new(requested.width.max(1), requested.height.max(1));
        let extent = wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("koharu canvas vello target"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            size,
            _texture: texture,
            view,
        }
    }
}

/// Owns all WGPU/Vello objects needed to turn CPU drawing descriptions into the
/// texture returned by `Canvas::render`.
///
/// Keeping this type separate means the canvas state machine does not need to
/// know about render passes, texture usage flags, or command submission.
pub(crate) struct GpuRenderer {
    gpu: CanvasGpu,
    vello: vello::Renderer,
    target: RenderTarget,
}

impl GpuRenderer {
    pub fn new(gpu: CanvasGpu, size: PhysicalSize) -> Result<Self> {
        let vello = vello::Renderer::new(
            &gpu.device,
            RendererOptions {
                antialiasing_support: AaSupport::area_only(),
                ..Default::default()
            },
        )
        .map_err(|error| Error::Gpu(error.to_string()))?;
        let target = RenderTarget::new(&gpu.device, size);
        Ok(Self { gpu, vello, target })
    }

    pub fn resize(&mut self, size: PhysicalSize) {
        self.target = RenderTarget::new(&self.gpu.device, size);
    }

    pub fn render_content(&mut self, scene: &Scene, background: Color) -> Result<()> {
        self.vello
            .render_to_texture(
                &self.gpu.device,
                &self.gpu.queue,
                scene,
                &self.target.view,
                &RenderParams {
                    base_color: vello::peniko::Color::from_rgba8(
                        background[0],
                        background[1],
                        background[2],
                        background[3],
                    ),
                    width: self.target.size.width,
                    height: self.target.size.height,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .map_err(|error| Error::Gpu(error.to_string()))
    }

    pub fn mark_image_dirty(&mut self, image: &ImageData) {
        self.vello.mark_override_image_dirty(image);
    }

    pub fn output(&self) -> &wgpu::TextureView {
        &self.target.view
    }

    pub fn read_pixel(&self, x: f64, y: f64) -> Result<Color> {
        if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
            return Err(Error::Invalid("sample point is outside the canvas".into()));
        }
        let x = x.floor() as u32;
        let y = y.floor() as u32;
        if x >= self.target.size.width || y >= self.target.size.height {
            return Err(Error::Invalid("sample point is outside the canvas".into()));
        }
        let row_bytes = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("koharu canvas color sample"),
            size: u64::from(row_bytes),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("koharu canvas color sample encoder"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target._texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let submission = self.gpu.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.gpu
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .map_err(|error| Error::Gpu(format!("failed to poll color sample: {error}")))?;
        receiver
            .recv()
            .map_err(|_| Error::Gpu("color sample channel closed".into()))?
            .map_err(|error| Error::Gpu(format!("failed to map color sample: {error}")))?;
        let mapped = slice.get_mapped_range();
        let color = [mapped[0], mapped[1], mapped[2], mapped[3]];
        drop(mapped);
        buffer.unmap();
        Ok(color)
    }

    #[cfg(test)]
    pub fn read_output(&self) -> Vec<u8> {
        let size = self.target.size;
        let row_bytes = size.width * 4;
        let padded_row_bytes = row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("koharu canvas visual-test readback"),
            size: u64::from(padded_row_bytes) * u64::from(size.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("koharu canvas visual-test encoder"),
            });
        encoder.copy_texture_to_buffer(
            self.target._texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row_bytes),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
        );
        let submission = self.gpu.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.gpu
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .expect("visual-test device polling failed");
        receiver
            .recv()
            .expect("visual-test readback channel closed")
            .expect("visual-test buffer mapping failed");

        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((row_bytes * size.height) as usize);
        for row in mapped
            .chunks_exact(padded_row_bytes as usize)
            .take(size.height as usize)
        {
            pixels.extend_from_slice(&row[..row_bytes as usize]);
        }
        drop(mapped);
        buffer.unmap();
        pixels
    }
}
