use std::sync::{Arc, Mutex};

use vello::{AaConfig, AaSupport, RenderParams, RendererOptions, Scene, wgpu};

use crate::{CanvasGpu, Color, Error, PhysicalSize, Result};

const SAMPLE_RING_SIZE: usize = 3;
const SAMPLE_ROW_BYTES: u64 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64;

struct RenderTarget {
    requested: PhysicalSize,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl RenderTarget {
    fn new(device: &wgpu::Device, requested: PhysicalSize) -> Self {
        let size = PhysicalSize::new(requested.width.max(1), requested.height.max(1));
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("koharu canvas viewport target"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
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
            requested,
            texture,
            view,
        }
    }
}

enum SampleStatus {
    Idle,
    Pending,
    Ready(std::result::Result<(), String>),
}

struct SampleMapState {
    generation: u64,
    status: SampleStatus,
}

type SampleCompletion = Box<dyn FnOnce(Result<Color>) + Send + 'static>;

struct SampleSlot {
    buffer: wgpu::Buffer,
    state: Arc<Mutex<SampleMapState>>,
    completion: Option<SampleCompletion>,
}

pub(crate) struct GpuRenderer {
    gpu: CanvasGpu,
    vello: vello::Renderer,
    target: RenderTarget,
    samples: Vec<SampleSlot>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl GpuRenderer {
    pub fn new(
        gpu: CanvasGpu,
        size: PhysicalSize,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let vello = vello::Renderer::new(
            &gpu.device,
            RendererOptions {
                antialiasing_support: AaSupport::area_only(),
                ..Default::default()
            },
        )
        .map_err(|error| Error::Gpu(error.to_string()))?;
        let target = RenderTarget::new(&gpu.device, size);
        let samples = (0..SAMPLE_RING_SIZE)
            .map(|_| SampleSlot {
                buffer: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("koharu canvas color sample"),
                    size: SAMPLE_ROW_BYTES,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                state: Arc::new(Mutex::new(SampleMapState {
                    generation: 0,
                    status: SampleStatus::Idle,
                })),
                completion: None,
            })
            .collect();
        Ok(Self {
            gpu,
            vello,
            target,
            samples,
            wake,
        })
    }

    pub fn resize(&mut self, size: PhysicalSize) {
        self.cancel_samples();
        self.target = RenderTarget::new(&self.gpu.device, size);
    }

    pub fn render_content(&mut self, scene: &Scene, background: Color) -> Result<()> {
        if self.target.requested.is_empty() {
            return Ok(());
        }
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
                    width: self.target.requested.width,
                    height: self.target.requested.height,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .map_err(|error| Error::Gpu(error.to_string()))
    }

    pub fn output(&self) -> Option<&wgpu::TextureView> {
        (!self.target.requested.is_empty()).then_some(&self.target.view)
    }

    pub fn request_pixel(
        &mut self,
        x: f64,
        y: f64,
        complete: impl FnOnce(Result<Color>) + Send + 'static,
    ) -> Result<()> {
        if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
            return Err(Error::Invalid("sample point is outside the canvas".into()));
        }
        let x = x.floor() as u32;
        let y = y.floor() as u32;
        if x >= self.target.requested.width || y >= self.target.requested.height {
            return Err(Error::Invalid("sample point is outside the canvas".into()));
        }
        self.poll_samples();
        let slot = self
            .samples
            .iter_mut()
            .find(|slot| {
                matches!(
                    slot.state.lock().expect("sample state poisoned").status,
                    SampleStatus::Idle
                )
            })
            .ok_or_else(|| Error::Invalid("color sample queue is full".into()))?;

        let generation = {
            let mut state = slot.state.lock().expect("sample state poisoned");
            state.generation = state.generation.wrapping_add(1).max(1);
            state.status = SampleStatus::Pending;
            state.generation
        };
        slot.completion = Some(Box::new(complete));
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("koharu canvas color sample encoder"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &slot.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        self.gpu.queue.submit([encoder.finish()]);
        let state = Arc::clone(&slot.state);
        let wake = Arc::clone(&self.wake);
        slot.buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let mut state = state.lock().expect("sample state poisoned");
                if state.generation == generation && matches!(state.status, SampleStatus::Pending) {
                    state.status = SampleStatus::Ready(
                        result.map_err(|error| format!("failed to map color sample: {error}")),
                    );
                    drop(state);
                    wake();
                }
            });
        Ok(())
    }

    pub fn poll_samples(&mut self) {
        let _ = self.gpu.device.poll(wgpu::PollType::Poll);
        let mut completed = Vec::new();
        for slot in &mut self.samples {
            let ready = {
                let mut state = slot.state.lock().expect("sample state poisoned");
                let SampleStatus::Ready(_) = &state.status else {
                    continue;
                };
                let status = std::mem::replace(&mut state.status, SampleStatus::Idle);
                let SampleStatus::Ready(result) = status else {
                    unreachable!();
                };
                result
            };
            let result = match ready {
                Ok(()) => {
                    let color = slot
                        .buffer
                        .slice(..)
                        .get_mapped_range()
                        .map_err(|error| format!("failed to access mapped color sample: {error}"))
                        .and_then(|mapped| {
                            let color = (mapped.len() >= 4)
                                .then(|| [mapped[0], mapped[1], mapped[2], mapped[3]])
                                .ok_or_else(|| "mapped color sample is truncated".to_owned());
                            drop(mapped);
                            color
                        });
                    slot.buffer.unmap();
                    color
                }
                Err(error) => {
                    slot.buffer.unmap();
                    Err(error)
                }
            };
            if let Some(complete) = slot.completion.take() {
                completed.push((complete, result.map_err(Error::Gpu)));
            }
        }
        for (complete, result) in completed {
            complete(result);
        }
    }

    pub fn cancel_samples(&mut self) {
        let mut cancelled = Vec::new();
        for slot in &mut self.samples {
            let mut state = slot.state.lock().expect("sample state poisoned");
            if matches!(state.status, SampleStatus::Idle) {
                continue;
            }
            state.generation = state.generation.wrapping_add(1).max(1);
            state.status = SampleStatus::Idle;
            drop(state);
            slot.buffer.unmap();
            if let Some(complete) = slot.completion.take() {
                cancelled.push(complete);
            }
        }
        for complete in cancelled {
            complete(Err(Error::Invalid("color sample was cancelled".into())));
        }
    }

    pub fn samples_pending(&self) -> bool {
        self.samples.iter().any(|slot| {
            !matches!(
                slot.state.lock().expect("sample state poisoned").status,
                SampleStatus::Idle
            )
        })
    }
}
