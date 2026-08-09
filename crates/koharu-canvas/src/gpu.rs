use std::{
    collections::VecDeque,
    sync::{Arc, mpsc},
};

use vello::{AaConfig, AaSupport, RenderParams, RendererOptions, Scene, wgpu};

use crate::{CanvasGpu, Error, PhysicalSize, Result, state::Color};

const SAMPLE_ROW_BYTES: u64 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64;
const SAMPLE_SLOTS: usize = 3;
const MAX_QUEUED_SAMPLES: usize = 8;

type SampleCallback = Box<dyn FnOnce(Result<Color>) + Send + 'static>;

/// The viewport-sized GPU image rendered by Vello and presented by the host.
struct RenderTarget {
    size: PhysicalSize,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl RenderTarget {
    fn new(device: &wgpu::Device, size: PhysicalSize) -> Self {
        debug_assert!(!size.is_empty());
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
            size,
            texture,
            view,
        }
    }
}

struct SampleRequest {
    x: u32,
    y: u32,
    complete: SampleCallback,
}

enum SampleState {
    Idle,
    Mapping {
        ticket: u64,
        complete: Option<SampleCallback>,
    },
    Cancelled {
        ticket: u64,
    },
}

struct SampleSlot {
    buffer: wgpu::Buffer,
    state: SampleState,
}

struct MapCompletion {
    slot: usize,
    ticket: u64,
    result: std::result::Result<(), wgpu::BufferAsyncError>,
}

struct SampleRing {
    slots: Vec<SampleSlot>,
    queued: VecDeque<SampleRequest>,
    sender: mpsc::Sender<MapCompletion>,
    receiver: mpsc::Receiver<MapCompletion>,
    wake: Arc<dyn Fn() + Send + Sync>,
    next_ticket: u64,
}

impl SampleRing {
    fn new(device: &wgpu::Device, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (sender, receiver) = mpsc::channel();
        let slots = (0..SAMPLE_SLOTS)
            .map(|index| SampleSlot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(match index {
                        0 => "koharu canvas color sample 0",
                        1 => "koharu canvas color sample 1",
                        _ => "koharu canvas color sample 2",
                    }),
                    size: SAMPLE_ROW_BYTES,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                state: SampleState::Idle,
            })
            .collect();
        Self {
            slots,
            queued: VecDeque::new(),
            sender,
            receiver,
            wake,
            next_ticket: 0,
        }
    }

    fn request(
        &mut self,
        gpu: &CanvasGpu,
        target: &RenderTarget,
        x: f64,
        y: f64,
        complete: SampleCallback,
    ) -> Result<()> {
        if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
            return Err(Error::Invalid("sample point is outside the canvas".into()));
        }
        let x = x.floor() as u32;
        let y = y.floor() as u32;
        if x >= target.size.width || y >= target.size.height {
            return Err(Error::Invalid("sample point is outside the canvas".into()));
        }
        if self.queued.len() >= MAX_QUEUED_SAMPLES
            && !self
                .slots
                .iter()
                .any(|slot| matches!(slot.state, SampleState::Idle))
        {
            return Err(Error::Invalid("color sample queue is full".into()));
        }
        self.queued.push_back(SampleRequest { x, y, complete });
        self.pump(gpu, target);
        (self.wake)();
        Ok(())
    }

    fn pump(&mut self, gpu: &CanvasGpu, target: &RenderTarget) {
        while let Some(slot_index) = self
            .slots
            .iter()
            .position(|slot| matches!(slot.state, SampleState::Idle))
        {
            let Some(request) = self.queued.pop_front() else {
                break;
            };
            self.next_ticket = self.next_ticket.wrapping_add(1).max(1);
            let ticket = self.next_ticket;
            let slot = &mut self.slots[slot_index];
            let mut encoder = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("koharu canvas color sample encoder"),
                });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &target.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: request.x,
                        y: request.y,
                        z: 0,
                    },
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
            gpu.queue.submit([encoder.finish()]);
            slot.state = SampleState::Mapping {
                ticket,
                complete: Some(request.complete),
            };
            let sender = self.sender.clone();
            let wake = Arc::clone(&self.wake);
            slot.buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = sender.send(MapCompletion {
                        slot: slot_index,
                        ticket,
                        result,
                    });
                    wake();
                });
        }
    }

    fn poll(&mut self, gpu: &CanvasGpu, target: Option<&RenderTarget>) {
        let _ = gpu.device.poll(wgpu::PollType::Poll);
        while let Ok(completion) = self.receiver.try_recv() {
            let Some(slot) = self.slots.get_mut(completion.slot) else {
                continue;
            };
            let state = std::mem::replace(&mut slot.state, SampleState::Idle);
            match state {
                SampleState::Mapping {
                    ticket,
                    mut complete,
                } if ticket == completion.ticket => {
                    let result = completion
                        .result
                        .map_err(|error| Error::Gpu(format!("failed to map color sample: {error}")))
                        .and_then(|()| {
                            let mapped = slot.buffer.slice(..).get_mapped_range();
                            if mapped.len() < 4 {
                                return Err(Error::Gpu(
                                    "color sample buffer returned too few bytes".into(),
                                ));
                            }
                            Ok([mapped[0], mapped[1], mapped[2], mapped[3]])
                        });
                    slot.buffer.unmap();
                    if let Some(complete) = complete.take() {
                        complete(result);
                    }
                }
                SampleState::Cancelled { ticket } if ticket == completion.ticket => {
                    slot.buffer.unmap();
                }
                current => {
                    slot.state = current;
                }
            }
        }
        if let Some(target) = target {
            self.pump(gpu, target);
        }
    }

    fn cancel(&mut self, message: &str) {
        for request in self.queued.drain(..) {
            (request.complete)(Err(Error::Gpu(message.into())));
        }
        for slot in &mut self.slots {
            let state = std::mem::replace(&mut slot.state, SampleState::Idle);
            match state {
                SampleState::Mapping {
                    ticket,
                    mut complete,
                } => {
                    if let Some(complete) = complete.take() {
                        complete(Err(Error::Gpu(message.into())));
                    }
                    slot.buffer.unmap();
                    slot.state = SampleState::Cancelled { ticket };
                }
                SampleState::Cancelled { ticket } => {
                    slot.state = SampleState::Cancelled { ticket };
                }
                SampleState::Idle => {}
            }
        }
    }

    fn pending(&self) -> bool {
        !self.queued.is_empty()
            || self
                .slots
                .iter()
                .any(|slot| !matches!(slot.state, SampleState::Idle))
    }
}

/// Owns all WGPU/Vello state for the offscreen viewport target.
pub(crate) struct GpuRenderer {
    gpu: CanvasGpu,
    vello: vello::Renderer,
    target: Option<RenderTarget>,
    samples: SampleRing,
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
        let target = (!size.is_empty()).then(|| RenderTarget::new(&gpu.device, size));
        let samples = SampleRing::new(&gpu.device, wake);
        Ok(Self {
            gpu,
            vello,
            target,
            samples,
        })
    }

    pub fn resize(&mut self, size: PhysicalSize) {
        self.cancel_samples();
        self.target = (!size.is_empty()).then(|| RenderTarget::new(&self.gpu.device, size));
    }

    pub fn render_content(&mut self, scene: &Scene, background: Color) -> Result<()> {
        let Some(target) = self.target.as_ref() else {
            return Ok(());
        };
        self.vello
            .render_to_texture(
                &self.gpu.device,
                &self.gpu.queue,
                scene,
                &target.view,
                &RenderParams {
                    base_color: vello::peniko::Color::from_rgba8(
                        background[0],
                        background[1],
                        background[2],
                        background[3],
                    ),
                    width: target.size.width,
                    height: target.size.height,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .map_err(|error| Error::Gpu(error.to_string()))
    }

    pub fn output(&self) -> Option<&wgpu::TextureView> {
        self.target.as_ref().map(|target| &target.view)
    }

    pub fn request_pixel(
        &mut self,
        x: f64,
        y: f64,
        complete: impl FnOnce(Result<Color>) + Send + 'static,
    ) -> Result<()> {
        let target = self
            .target
            .as_ref()
            .ok_or_else(|| Error::Invalid("cannot sample an empty canvas".into()))?;
        self.samples
            .request(&self.gpu, target, x, y, Box::new(complete))
    }

    pub fn poll_samples(&mut self) {
        self.samples.poll(&self.gpu, self.target.as_ref());
    }

    pub fn samples_pending(&self) -> bool {
        self.samples.pending()
    }

    pub fn cancel_samples(&mut self) {
        self.samples.cancel("color sample was cancelled");
    }
}

impl Drop for GpuRenderer {
    fn drop(&mut self) {
        self.samples.cancel("canvas was dropped");
    }
}
