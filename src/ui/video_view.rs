//! Video surface: one persistent GPU texture that is overwritten in place.
//!
//! The stock `image` widget uploads every new handle asynchronously and draws
//! nothing until that finishes, which makes video flicker. Here the texture
//! stays alive and each frame is written into it synchronously before drawing.

use crate::player::VideoFrame;
use crate::state::app_state::VideoAdjust;
use iced::mouse;
use iced::wgpu;
use iced::widget::shader::{self, Viewport};
use iced::Rectangle;
use std::sync::Arc;

pub struct VideoProgram {
    pub frame: Arc<VideoFrame>,
    pub adjust: VideoAdjust,
}

impl<Message> shader::Program<Message> for VideoProgram {
    type State = ();
    type Primitive = VideoPrimitive;

    fn draw(&self, _state: &(), _cursor: mouse::Cursor, _bounds: Rectangle) -> VideoPrimitive {
        VideoPrimitive {
            frame: self.frame.clone(),
            adjust: self.adjust,
        }
    }
}

#[derive(Debug)]
pub struct VideoPrimitive {
    frame: Arc<VideoFrame>,
    adjust: VideoAdjust,
}

struct Target {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

pub struct VideoPipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    texture_format: wgpu::TextureFormat,
    /// The texture decodes sRGB on sampling, so the shader sees linear light.
    linear_sampling: bool,
    target: Option<Target>,
    uploaded: Option<u64>,
}

/// Byte size of the WGSL `Uniforms` struct.
const UNIFORM_SIZE: usize = 32;

const SHADER: &str = r#"
struct Uniforms {
    scale: vec2<f32>,
    brightness: f32,
    contrast: f32,
    monochrome: f32,
    linear_sampling: f32,
    saturation: f32,
    _pad: f32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var frame: texture_2d<f32>;
@group(0) @binding(2) var frame_sampler: sampler;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, 1.0),
    );
    let c = corners[index];

    var out: VsOut;
    out.position = vec4<f32>(c * u.scale, 0.0, 1.0);
    out.uv = vec2<f32>((c.x + 1.0) * 0.5, (1.0 - c.y) * 0.5);
    return out;
}

fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3<f32>(0.0031308));
}

fn srgb_decode(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var color = textureSample(frame, frame_sampler, in.uv).rgb;

    // Adjust the display-encoded values, as video picture controls do.
    let encoded = u.linear_sampling > 0.5;
    if encoded {
        color = srgb_encode(color);
    }
    if u.monochrome > 0.5 {
        color = vec3<f32>(dot(color, vec3<f32>(0.2126, 0.7152, 0.0722)));
    }
    let luma = vec3<f32>(dot(color, vec3<f32>(0.2126, 0.7152, 0.0722)));
    color = mix(luma, color, 1.0 + u.saturation);
    color = (color - 0.5) * (1.0 + u.contrast) + 0.5 + u.brightness * 0.5;
    color = clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
    if encoded {
        color = srgb_decode(color);
    }

    return vec4<f32>(color, 1.0);
}
"#;

impl shader::Pipeline for VideoPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..wgpu::PrimitiveState::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video uniforms"),
            size: UNIFORM_SIZE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Frames are already display-encoded; match the surface so the bytes
        // reach the screen unchanged.
        let texture_format = if format.is_srgb() {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };

        Self {
            pipeline,
            layout,
            sampler,
            uniforms,
            texture_format,
            linear_sampling: format.is_srgb(),
            target: None,
            uploaded: None,
        }
    }
}

impl VideoPipeline {
    fn target(&mut self, device: &wgpu::Device, width: u32, height: u32) -> &Target {
        let stale = self
            .target
            .as_ref()
            .is_none_or(|t| t.width != width || t.height != height);

        if stale {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("video frame"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.texture_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("video bind group"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.uploaded = None;
            self.target = Some(Target {
                texture,
                bind_group,
                width,
                height,
            });
        }

        self.target.as_ref().expect("target was just created")
    }
}

impl shader::Primitive for VideoPrimitive {
    type Pipeline = VideoPipeline;

    fn prepare(
        &self,
        pipeline: &mut VideoPipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        let frame = &self.frame;
        if frame.width == 0 || frame.height == 0 {
            return;
        }

        let needs_upload = pipeline.uploaded != Some(frame.id)
            || pipeline
                .target
                .as_ref()
                .is_none_or(|t| t.width != frame.width || t.height != frame.height);

        let target = pipeline.target(device, frame.width, frame.height);
        if needs_upload {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &target.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &frame.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(frame.width * 4),
                    rows_per_image: Some(frame.height),
                },
                wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
            );
            pipeline.uploaded = Some(frame.id);
        }

        // Letterbox: shrink the quad on one axis to keep the aspect ratio.
        let frame_aspect = frame.width as f32 / frame.height as f32;
        let bounds_aspect = bounds.width / bounds.height.max(1.0);
        let scale = if bounds_aspect > frame_aspect {
            [frame_aspect / bounds_aspect, 1.0]
        } else {
            [1.0, bounds_aspect / frame_aspect]
        };
        let flag = |on: bool| if on { 1.0f32 } else { 0.0 };
        let fields = [
            scale[0],
            scale[1],
            self.adjust.brightness,
            self.adjust.contrast,
            flag(self.adjust.monochrome),
            flag(pipeline.linear_sampling),
            self.adjust.saturation,
        ];
        let mut bytes = [0u8; UNIFORM_SIZE];
        for (chunk, value) in bytes.chunks_exact_mut(4).zip(fields) {
            chunk.copy_from_slice(&value.to_ne_bytes());
        }
        queue.write_buffer(&pipeline.uniforms, 0, &bytes);
    }

    fn draw(&self, pipeline: &VideoPipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        let Some(target) = &pipeline.target else {
            return true;
        };
        render_pass.set_pipeline(&pipeline.pipeline);
        render_pass.set_bind_group(0, &target.bind_group, &[]);
        render_pass.draw(0..4, 0..1);
        true
    }
}
