use std::{mem::size_of, sync::Arc};

use bytemuck::{Pod, Zeroable};
use cef::osr_texture_import::{SharedTextureHandle, TextureImporter as _};
use thiserror::Error;

use super::AcceleratedFrame;

#[derive(Clone)]
pub(crate) struct BrowserGpu {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub backend: wgpu::Backend,
}

impl BrowserGpu {
    pub(crate) const fn supports_accelerated_osr(&self) -> bool {
        #[cfg(target_os = "windows")]
        return matches!(self.backend, wgpu::Backend::Dx12 | wgpu::Backend::Vulkan);
        #[cfg(target_os = "linux")]
        return matches!(self.backend, wgpu::Backend::Vulkan);
        #[cfg(target_os = "macos")]
        return matches!(self.backend, wgpu::Backend::Metal);
        #[allow(unreachable_code)]
        false
    }
}

pub(crate) struct AcceleratedFrameImporter {
    gpu: BrowserGpu,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MappingImmediates {
    origin: [f32; 2],
    scale: [f32; 2],
}

impl AcceleratedFrameImporter {
    pub(crate) fn new(gpu: BrowserGpu) -> Self {
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("koharu CEF accelerated frame layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("koharu CEF accelerated frame pipeline layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: size_of::<MappingImmediates>() as u32,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("koharu CEF accelerated frame shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("accelerated.wgsl").into()),
            });
        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("koharu CEF accelerated frame pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fragment"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("koharu CEF accelerated frame sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            gpu,
            layout,
            sampler,
            pipeline,
        }
    }

    pub(crate) fn import(
        &self,
        sequence: u64,
        info: &cef::AcceleratedPaintInfo,
    ) -> Result<AcceleratedFrame, AcceleratedFrameError> {
        let mapping = FrameMapping::from_info(info)?;
        let shared = SharedTextureHandle::new(info);
        if !supports_hardware_acceleration(&shared, &self.gpu.device) {
            return Err(AcceleratedFrameError::UnsupportedBackend(self.gpu.backend));
        }
        let imported = shared
            .import_texture(&self.gpu.device)
            .map_err(|error| AcceleratedFrameError::Import(error.to_string()))?;
        if imported.usage().contains(wgpu::TextureUsages::COPY_DST) {
            return Err(AcceleratedFrameError::Import(
                "cef-rs could not import the shared texture".into(),
            ));
        }
        if imported.width() != mapping.coded_width || imported.height() != mapping.coded_height {
            return Err(AcceleratedFrameError::UnexpectedTextureSize {
                expected: (mapping.coded_width, mapping.coded_height),
                actual: (imported.width(), imported.height()),
            });
        }

        let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("koharu owned CEF accelerated frame"),
            size: wgpu::Extent3d {
                width: mapping.output_width,
                height: mapping.output_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let source = imported.create_view(&wgpu::TextureViewDescriptor::default());
        let target = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = self
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("koharu CEF accelerated frame bind group"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("koharu CEF accelerated frame copy encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("koharu CEF accelerated frame copy"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_immediates(0, bytemuck::bytes_of(&mapping.immediates()));
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        let submission = self.gpu.queue.submit([encoder.finish()]);
        self.gpu
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .map_err(|error| AcceleratedFrameError::Copy(error.to_string()))?;

        Ok(AcceleratedFrame {
            sequence,
            width: mapping.output_width,
            height: mapping.output_height,
            texture,
        })
    }
}

fn supports_hardware_acceleration(handle: &SharedTextureHandle, device: &wgpu::Device) -> bool {
    match handle {
        #[cfg(target_os = "windows")]
        SharedTextureHandle::D3D11(importer) => importer.supports_hardware_acceleration(device),
        #[cfg(target_os = "linux")]
        SharedTextureHandle::DmaBuf(importer) => importer.supports_hardware_acceleration(device),
        #[cfg(target_os = "macos")]
        SharedTextureHandle::IOSurface(importer) => importer.supports_hardware_acceleration(device),
        SharedTextureHandle::Unsupported => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FrameMapping {
    coded_width: u32,
    coded_height: u32,
    content_x: u32,
    content_y: u32,
    content_width: u32,
    content_height: u32,
    output_width: u32,
    output_height: u32,
}

impl FrameMapping {
    fn from_info(info: &cef::AcceleratedPaintInfo) -> Result<Self, AcceleratedFrameError> {
        let coded = &info.extra.coded_size;
        let content = &info.extra.content_rect;
        let source = (info.extra.has_source_size != 0)
            .then_some((info.extra.source_size.width, info.extra.source_size.height));
        Self::new(
            (coded.width, coded.height),
            (content.x, content.y, content.width, content.height),
            source,
        )
    }

    fn new(
        coded: (i32, i32),
        content: (i32, i32, i32, i32),
        source: Option<(i32, i32)>,
    ) -> Result<Self, AcceleratedFrameError> {
        let positive = |value: i32| u32::try_from(value).ok().filter(|value| *value > 0);
        let nonnegative = |value: i32| u32::try_from(value).ok();
        let (Some(coded_width), Some(coded_height)) = (positive(coded.0), positive(coded.1)) else {
            return Err(AcceleratedFrameError::InvalidMapping);
        };
        let (Some(content_x), Some(content_y), Some(content_width), Some(content_height)) = (
            nonnegative(content.0),
            nonnegative(content.1),
            positive(content.2),
            positive(content.3),
        ) else {
            return Err(AcceleratedFrameError::InvalidMapping);
        };
        if content_x
            .checked_add(content_width)
            .is_none_or(|right| right > coded_width)
            || content_y
                .checked_add(content_height)
                .is_none_or(|bottom| bottom > coded_height)
        {
            return Err(AcceleratedFrameError::InvalidMapping);
        }
        let (output_width, output_height) = source
            .and_then(|(width, height)| Some((positive(width)?, positive(height)?)))
            .unwrap_or((content_width, content_height));
        Ok(Self {
            coded_width,
            coded_height,
            content_x,
            content_y,
            content_width,
            content_height,
            output_width,
            output_height,
        })
    }

    fn immediates(self) -> MappingImmediates {
        MappingImmediates {
            origin: [
                self.content_x as f32 / self.coded_width as f32,
                self.content_y as f32 / self.coded_height as f32,
            ],
            scale: [
                self.content_width as f32 / self.coded_width as f32,
                self.content_height as f32 / self.coded_height as f32,
            ],
        }
    }
}

#[derive(Debug, Error)]
pub(crate) enum AcceleratedFrameError {
    #[error("CEF accelerated paint metadata is invalid")]
    InvalidMapping,
    #[error("WGPU backend {0:?} cannot import CEF accelerated frames on this platform")]
    UnsupportedBackend(wgpu::Backend),
    #[error("failed to import the CEF shared texture: {0}")]
    Import(String),
    #[error("CEF imported texture size is {actual:?}, expected {expected:?}")]
    UnexpectedTextureSize {
        expected: (u32, u32),
        actual: (u32, u32),
    },
    #[error("failed to copy the CEF shared texture: {0}")]
    Copy(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_mapping_crops_padding_and_restores_source_size() {
        let mapping = FrameMapping::new((1024, 1024), (4, 8, 1000, 700), Some((800, 560))).unwrap();
        assert_eq!((mapping.output_width, mapping.output_height), (800, 560));
        assert_eq!(mapping.immediates().origin, [4.0 / 1024.0, 8.0 / 1024.0]);
        assert_eq!(
            mapping.immediates().scale,
            [1000.0 / 1024.0, 700.0 / 1024.0]
        );
    }

    #[test]
    fn content_mapping_rejects_regions_outside_the_shared_texture() {
        assert!(matches!(
            FrameMapping::new((800, 600), (10, 10, 800, 600), None),
            Err(AcceleratedFrameError::InvalidMapping)
        ));
    }
}
