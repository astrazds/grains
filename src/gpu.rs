use std::future::Future;
use std::sync::Arc;

use winit::window::Window;

use crate::grid::{CELL_COUNT, GRID_HEIGHT, GRID_WIDTH, Grid, LETTERBOX, Size, letterbox};

const PASSES_PER_FRAME: u32 = 4;

#[derive(Debug)]
pub struct NoAdapter {
    names: Vec<String>,
}

impl std::fmt::Display for NoAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no adapter: {}", self.names.join(", "))
    }
}

pub(crate) struct Gpu {
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: Option<Surface>,
    window: Option<Arc<Window>>,
    size: Size,
    cells: [wgpu::Buffer; 2],
    front: usize,
    passes: u32,
    seeded: bool,
    uniform: wgpu::Buffer,
    draw_groups: [wgpu::BindGroup; 2],
    pipeline: wgpu::RenderPipeline,
    sim_uniform: wgpu::Buffer,
    sim_groups: [wgpu::BindGroup; 2],
    sim_pipeline: wgpu::ComputePipeline,
}

struct Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    origin_x: i32,
    origin_y: i32,
    scale: u32,
    grid_width: u32,
    grid_height: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

const _: () = assert!(std::mem::size_of::<Uniforms>() == 32);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SimUniform {
    offset: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

const _: () = assert!(std::mem::size_of::<SimUniform>() == 16);

pub(crate) fn wait(future: impl Future<Output = ()> + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    pollster::block_on(future);
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(future);
}

fn backends() -> wgpu::Backends {
    #[cfg(target_arch = "wasm32")]
    {
        wgpu::Backends::BROWSER_WEBGPU
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        wgpu::Backends::all()
    }
}

impl Gpu {
    pub(crate) async fn for_window(window: Arc<Window>) -> Result<Self, NoAdapter> {
        let size = window.inner_size();
        Self::open(
            Some(window),
            Size {
                width: size.width,
                height: size.height,
            },
        )
        .await
    }

    pub(crate) fn render_rgba(grid: &Grid, size: Size) -> Result<Vec<u8>, NoAdapter> {
        if size.width == 0 || size.height == 0 {
            return Ok(Vec::new());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let gpu = pollster::block_on(Self::open(None, size))?;
            Ok(gpu.read_rgba(grid, size))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = grid;
            Err(NoAdapter { names: Vec::new() })
        }
    }

    pub(crate) fn resize(&mut self, size: Size) {
        self.size = size;
        self.reconfigure();
    }

    pub(crate) fn draw(&mut self, window: &Window, grid: &Grid) {
        if self.size.width == 0 || self.size.height == 0 {
            return;
        }
        let current = {
            let Some(surface) = self.surface.as_ref() else {
                return;
            };
            surface.surface.get_current_texture()
        };
        match current {
            wgpu::CurrentSurfaceTexture::Success(frame) => {
                self.paint(&frame.texture, grid);
                window.pre_present_notify();
                self.queue.present(frame);
            }
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                drop(frame);
                self.reconfigure();
            }
            wgpu::CurrentSurfaceTexture::Outdated => self.reconfigure(),
            wgpu::CurrentSurfaceTexture::Lost => self.recreate_surface(),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {}
            wgpu::CurrentSurfaceTexture::Validation => panic!("surface validation"),
        }
    }

    async fn open(window: Option<Arc<Window>>, size: Size) -> Result<Self, NoAdapter> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = backends();
        let instance = wgpu::Instance::new(descriptor);
        let surface = window.as_ref().map(|window| {
            instance
                .create_surface(Arc::clone(window))
                .expect("surface")
        });
        let adapters = instance.enumerate_adapters(backends()).await;
        let mut names = Vec::new();
        let mut opened = None;
        for adapter in adapters {
            names.push(adapter.get_info().name);
            let flags = adapter.get_downlevel_capabilities().flags;
            if !flags.contains(
                wgpu::DownlevelFlags::FRAGMENT_STORAGE | wgpu::DownlevelFlags::COMPUTE_SHADERS,
            ) {
                continue;
            }
            let format = if let Some(surface) = &surface {
                if !adapter.is_surface_supported(surface) {
                    continue;
                }
                let caps = surface.get_capabilities(&adapter);
                let Some(format) = pick_format(&caps.formats) else {
                    continue;
                };
                format
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            };
            let descriptor = wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            };
            if let Ok((device, queue)) = adapter.request_device(&descriptor).await {
                opened = Some((device, queue, format));
                break;
            }
        }
        let Some((device, queue, format)) = opened else {
            return Err(NoAdapter { names });
        };

        let gpu_parts = resources(&device, format);
        let surface = surface.map(|surface| {
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                color_space: wgpu::SurfaceColorSpace::Auto,
                width: size.width,
                height: size.height,
                present_mode: wgpu::PresentMode::AutoVsync,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: Vec::new(),
            };
            if size.width > 0 && size.height > 0 {
                surface.configure(&device, &config);
            }
            Surface { surface, config }
        });

        Ok(Self {
            instance,
            device,
            queue,
            surface,
            window,
            size,
            cells: gpu_parts.cells,
            front: 0,
            passes: 0,
            seeded: false,
            uniform: gpu_parts.uniform,
            draw_groups: gpu_parts.draw_groups,
            pipeline: gpu_parts.pipeline,
            sim_uniform: gpu_parts.sim_uniform,
            sim_groups: gpu_parts.sim_groups,
            sim_pipeline: gpu_parts.sim_pipeline,
        })
    }

    fn reconfigure(&mut self) {
        if self.size.width == 0 || self.size.height == 0 {
            return;
        }
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        surface.config.width = self.size.width;
        surface.config.height = self.size.height;
        surface.surface.configure(&self.device, &surface.config);
    }

    fn recreate_surface(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let surface = self.instance.create_surface(window).expect("surface");
        if let Some(slot) = self.surface.as_mut() {
            slot.surface = surface;
        }
        self.reconfigure();
    }

    fn paint(&mut self, texture: &wgpu::Texture, grid: &Grid) {
        if !self.seeded {
            self.queue.write_buffer(&self.cells[0], 0, grid.as_bytes());
            self.front = 0;
            self.seeded = true;
        }
        for _ in 0..PASSES_PER_FRAME {
            let offset = self.passes % 2;
            self.step(offset);
            self.passes += 1;
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.draw_cells(&mut encoder, &view, self.size, self.front);
        self.queue.submit([encoder.finish()]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_rgba(&self, grid: &Grid, size: Size) -> Vec<u8> {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.queue.write_buffer(&self.cells[0], 0, grid.as_bytes());
        self.draw_cells(&mut encoder, &view, size, 0);
        let stride = padded_row_bytes(size.width);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(stride) * u64::from(size.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(size.height),
                },
            },
            wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        receiver.recv().expect("map callback").expect("map buffer");
        let mapped = slice.get_mapped_range().expect("mapped range");
        let row = (size.width * 4) as usize;
        let stride = stride as usize;
        let mut pixels = Vec::with_capacity(row * size.height as usize);
        for y in 0..size.height as usize {
            let start = y * stride;
            pixels.extend_from_slice(&mapped[start..start + row]);
        }
        drop(mapped);
        buffer.unmap();
        pixels
    }

    fn step(&mut self, offset: u32) {
        let src = self.front;
        let dst = src ^ 1;
        let sim = SimUniform {
            offset,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        };
        self.queue
            .write_buffer(&self.sim_uniform, 0, bytemuck::bytes_of(&sim));
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let bytes = (CELL_COUNT * 4) as u64;
        encoder.copy_buffer_to_buffer(&self.cells[src], 0, &self.cells[dst], 0, bytes);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.sim_pipeline);
            pass.set_bind_group(0, &self.sim_groups[src], &[]);
            let (groups_x, groups_y) = workgroups(offset);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }
        self.queue.submit([encoder.finish()]);
        self.front = dst;
    }

    #[cfg(test)]
    pub(crate) fn preview(grid: &Grid, passes: u32, size: Size) -> Result<Vec<u8>, NoAdapter> {
        let mut gpu = pollster::block_on(Self::open(None, size))?;
        gpu.queue.write_buffer(&gpu.cells[0], 0, grid.as_bytes());
        for pass in 0..passes {
            gpu.step(pass % 2);
        }
        Ok(gpu.read_rgba_from(gpu.front, size))
    }

    #[cfg(test)]
    fn read_rgba_from(&self, index: usize, size: Size) -> Vec<u8> {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.draw_cells(&mut encoder, &view, size, index);
        let stride = padded_row_bytes(size.width);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(stride) * u64::from(size.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(size.height),
                },
            },
            wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let mapped = map_buffer(&self.device, &buffer);
        let row = (size.width * 4) as usize;
        let stride = stride as usize;
        let mut pixels = Vec::with_capacity(row * size.height as usize);
        for y in 0..size.height as usize {
            let start = y * stride;
            pixels.extend_from_slice(&mapped[start..start + row]);
        }
        pixels
    }

    #[cfg(test)]
    pub(crate) fn simulate(grid: &Grid, passes: u32) -> Result<Vec<crate::grid::Cell>, NoAdapter> {
        let gpu = pollster::block_on(Self::open(
            None,
            Size {
                width: GRID_WIDTH as u32,
                height: GRID_HEIGHT as u32,
            },
        ))?;
        gpu.queue.write_buffer(&gpu.cells[0], 0, grid.as_bytes());
        let mut gpu = gpu;
        for pass in 0..passes {
            gpu.step(pass % 2);
        }
        Ok(gpu.read_cells(gpu.front))
    }

    #[cfg(test)]
    fn read_cells(&self, index: usize) -> Vec<crate::grid::Cell> {
        let bytes = (CELL_COUNT * 4) as u64;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_buffer(&self.cells[index], 0, &buffer, 0, bytes);
        self.queue.submit([encoder.finish()]);
        let mapped = map_buffer(&self.device, &buffer);
        bytemuck::cast_slice(&mapped).to_vec()
    }

    fn draw_cells(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        size: Size,
        index: usize,
    ) {
        let Some(fit) = letterbox(size) else {
            return;
        };
        let uniforms = Uniforms {
            origin_x: fit.origin_x,
            origin_y: fit.origin_y,
            scale: fit.scale,
            grid_width: GRID_WIDTH as u32,
            grid_height: GRID_HEIGHT as u32,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        };
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniforms));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(letterbox_color()),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let Some((x, y, width, height)) = viewport(fit, size) else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.draw_groups[index], &[]);
        pass.set_viewport(x, y, width, height, 0.0, 1.0);
        pass.draw(0..3, 0..1);
    }
}

struct Parts {
    cells: [wgpu::Buffer; 2],
    uniform: wgpu::Buffer,
    draw_groups: [wgpu::BindGroup; 2],
    pipeline: wgpu::RenderPipeline,
    sim_uniform: wgpu::Buffer,
    sim_groups: [wgpu::BindGroup; 2],
    sim_pipeline: wgpu::ComputePipeline,
}

fn resources(device: &wgpu::Device, format: wgpu::TextureFormat) -> Parts {
    let usage =
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
    let cells = [0, 1].map(|_| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (CELL_COUNT * 4) as u64,
            usage,
            mapped_at_creation: false,
        })
    });
    let uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 32,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let sim_uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let draw_groups = [0, 1].map(|index| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &cells[index],
                        offset: 0,
                        size: None,
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &uniform,
                        offset: 0,
                        size: None,
                    }),
                },
            ],
        })
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/draw.wgsl").into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let sim_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let sim_groups = [0, 1].map(|index| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &sim_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &cells[index],
                        offset: 0,
                        size: None,
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &cells[index ^ 1],
                        offset: 0,
                        size: None,
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &sim_uniform,
                        offset: 0,
                        size: None,
                    }),
                },
            ],
        })
    });
    let sim_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/sim.wgsl").into()),
    });
    let sim_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&sim_layout)],
        immediate_size: 0,
    });
    let sim_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&sim_pipeline_layout),
        module: &sim_shader,
        entry_point: Some("step"),
        compilation_options: Default::default(),
        cache: None,
    });
    Parts {
        cells,
        uniform,
        draw_groups,
        pipeline,
        sim_uniform,
        sim_groups,
        sim_pipeline,
    }
}

fn workgroups(offset: u32) -> (u32, u32) {
    let blocks_x = (GRID_WIDTH as u32 - offset) / 2;
    let blocks_y = (GRID_HEIGHT as u32 - offset) / 2;
    (blocks_x.div_ceil(8), blocks_y.div_ceil(8))
}

#[cfg(test)]
fn map_buffer(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
    let slice = buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    receiver.recv().expect("map callback").expect("map buffer");
    let mapped = slice.get_mapped_range().expect("mapped range");
    let bytes = mapped.to_vec();
    drop(mapped);
    buffer.unmap();
    bytes
}

fn pick_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    if formats.contains(&wgpu::TextureFormat::Rgba8Unorm) {
        Some(wgpu::TextureFormat::Rgba8Unorm)
    } else if formats.contains(&wgpu::TextureFormat::Bgra8Unorm) {
        Some(wgpu::TextureFormat::Bgra8Unorm)
    } else {
        formats.iter().copied().find(|format| !format.is_srgb())
    }
}

fn letterbox_color() -> wgpu::Color {
    wgpu::Color {
        r: f64::from(LETTERBOX[0]) / 255.0,
        g: f64::from(LETTERBOX[1]) / 255.0,
        b: f64::from(LETTERBOX[2]) / 255.0,
        a: f64::from(LETTERBOX[3]) / 255.0,
    }
}

fn viewport(fit: crate::grid::Fit, size: Size) -> Option<(f32, f32, f32, f32)> {
    let x0 = fit.origin_x;
    let y0 = fit.origin_y;
    let x1 = fit.origin_x + GRID_WIDTH as i32 * fit.scale as i32;
    let y1 = fit.origin_y + GRID_HEIGHT as i32 * fit.scale as i32;
    let left = x0.max(0).min(size.width as i32);
    let top = y0.max(0).min(size.height as i32);
    let right = x1.max(0).min(size.width as i32);
    let bottom = y1.max(0).min(size.height as i32);
    if right <= left || bottom <= top {
        return None;
    }
    Some((
        left as f32,
        top as f32,
        (right - left) as f32,
        (bottom - top) as f32,
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn padded_row_bytes(width: u32) -> u32 {
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    (width * 4).div_ceil(align) * align
}

#[cfg(test)]
mod tests {
    use crate::grid::{Cell, FLAGS_SHIFT, LIFE_SHIFT, MATERIAL_SHIFT, Size, VARIATION_SHIFT};

    #[test]
    fn shader_shifts_match_the_word() {
        let shader = include_str!("../shaders/draw.wgsl");
        for (name, shift) in [
            ("MATERIAL_SHIFT", MATERIAL_SHIFT),
            ("VARIATION_SHIFT", VARIATION_SHIFT),
            ("LIFE_SHIFT", LIFE_SHIFT),
            ("FLAGS_SHIFT", FLAGS_SHIFT),
        ] {
            let line = format!("const {name}: u32 = {shift}u;");
            assert!(shader.contains(&line), "{line}");
        }
    }

    #[test]
    fn offscreen_pixels() {
        let grid = crate::grid::Grid::from_fn(|x, y| match (x, y) {
            (0, 0) => Cell::pack(0x10, 0x20, 0x30, 0x40),
            (1, 0) => Cell::pack(0, 0, 0, 0),
            (56, 44) => Cell::pack(0x07, 0x02, 0x03, 0x04),
            (56, 45) => Cell::pack(0x10, 0x20, 0x30, 0x00),
            _ => Cell::pack(0, 0, 0, 0),
        });
        assert!(
            crate::render_rgba(
                &grid,
                Size {
                    width: 0,
                    height: 288
                }
            )
            .unwrap()
            .is_empty()
        );
        assert!(
            crate::render_rgba(
                &grid,
                Size {
                    width: 512,
                    height: 0
                }
            )
            .unwrap()
            .is_empty()
        );

        let Some(frame) = colours(crate::render_rgba(
            &grid,
            Size {
                width: 512,
                height: 288,
            },
        )) else {
            return;
        };
        asserted(512, 288, &frame, 0, 0, [0x10, 0x20, 0x30, 0xFF]);
        asserted(512, 288, &frame, 1, 0, [0x00, 0x00, 0x00, 0xFF]);

        let Some(frame) = colours(crate::render_rgba(
            &grid,
            Size {
                width: 1024,
                height: 600,
            },
        )) else {
            return;
        };
        asserted(1024, 600, &frame, 0, 0, [20, 20, 23, 255]);
        asserted(1024, 600, &frame, 0, 11, [20, 20, 23, 255]);
        asserted(1024, 600, &frame, 0, 12, [0x10, 0x20, 0x30, 0xFF]);

        let Some(frame) = colours(crate::render_rgba(
            &grid,
            Size {
                width: 400,
                height: 200,
            },
        )) else {
            return;
        };
        assert_eq!(frame.len(), 400 * 200 * 4);
        eprintln!("asserted 400x200 byte length = {}", frame.len());
        asserted(400, 200, &frame, 0, 0, [0x07, 0x02, 0x03, 0xFF]);
        asserted(400, 200, &frame, 0, 1, [0x10, 0x20, 0x30, 0xFF]);
    }

    fn colours<T>(result: Result<T, crate::NoAdapter>) -> Option<T> {
        match result {
            Ok(frame) => Some(frame),
            Err(err) if gpu_required(std::env::var("GRAINS_REQUIRE_GPU").ok().as_deref()) => {
                panic!("GRAINS_REQUIRE_GPU=1 and no adapter was found: {err}")
            }
            Err(err) => {
                eprintln!("offscreen colour asserts skipped: {err}");
                None
            }
        }
    }

    fn asserted(width: u32, height: u32, frame: &[u8], x: u32, y: u32, expect: [u8; 4]) {
        let got = pixel(frame, width, x, y);
        assert_eq!(got, expect);
        eprintln!("asserted {width}x{height} pixel ({x},{y}) = {got:?}");
    }

    #[test]
    #[ignore = "writes frames under target/sand-frames"]
    fn capture_sand_frames() {
        let grid = crate::grid::Grid::demo();
        let size = Size {
            width: 512,
            height: 288,
        };
        let dir = std::path::Path::new("target/sand-frames");
        std::fs::create_dir_all(dir).unwrap();
        for (frame, passes) in [0, 4, 8, 12, 16, 24, 32, 48].into_iter().enumerate() {
            let pixels = super::Gpu::preview(&grid, passes, size).expect("preview");
            let path = dir.join(format!("frame-{frame:02}.ppm"));
            let mut ppm = format!("P6\n{} {}\n255\n", size.width, size.height).into_bytes();
            for px in pixels.chunks(4) {
                ppm.extend_from_slice(&px[..3]);
            }
            std::fs::write(path, ppm).unwrap();
        }
    }

    #[test]
    fn one_grain_falls_to_the_floor() {
        use crate::grid::{EMPTY, SAND};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        grid.set(8, 280, Cell::pack(SAND, 0, 0, 0));
        let Some(cells) = colours(super::Gpu::simulate(&grid, 7)) else {
            return;
        };
        assert_eq!(material_at(&cells, 8, 287), SAND);
        assert_eq!(material_at(&cells, 8, 280), EMPTY);
        assert_eq!(sand_count(&cells), 1);
        eprintln!("asserted one sand grain at (8,287) after 7 passes");
    }

    #[test]
    fn sand_does_not_pass_through_stone() {
        use crate::grid::{EMPTY, SAND, STONE};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        for x in 0..crate::grid::GRID_WIDTH {
            grid.set(x, 284, Cell::pack(STONE, 0, 0, 0));
        }
        for x in 8..12 {
            grid.set(x, 280, Cell::pack(SAND, 0, 0, 0));
        }
        let Some(cells) = colours(super::Gpu::simulate(&grid, 24)) else {
            return;
        };
        assert_eq!(sand_count(&cells), 4);
        for x in 0..crate::grid::GRID_WIDTH {
            assert_eq!(material_at(&cells, x, 284), STONE, "stone at {x}");
            for y in 285..crate::grid::GRID_HEIGHT {
                assert_ne!(material_at(&cells, x, y), SAND, "sand at ({x},{y})");
            }
        }
        eprintln!("asserted 4 sand grains stayed above the stone row");
    }

    #[test]
    fn sand_count_is_conserved() {
        let grid = crate::grid::Grid::demo();
        let before = grid.sand_count();
        let Some(cells) = colours(super::Gpu::simulate(&grid, 16)) else {
            return;
        };
        assert_eq!(sand_count(&cells), before);
        eprintln!("asserted sand count {before} after 16 passes");
    }

    #[test]
    fn sand_slides_beside_stone() {
        use crate::grid::{EMPTY, SAND, STONE};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        grid.set(8, 10, Cell::pack(SAND, 0, 0, 0));
        grid.set(8, 11, Cell::pack(STONE, 0, 0, 0));
        let Some(cells) = colours(super::Gpu::simulate(&grid, 1)) else {
            return;
        };
        assert_eq!(material_at(&cells, 8, 11), STONE);
        assert_eq!(material_at(&cells, 8, 10), EMPTY);
        assert_eq!(material_at(&cells, 9, 11), SAND);
        assert_eq!(sand_count(&cells), 1);
        eprintln!("asserted sand slid from (8,10) to (9,11)");
    }

    fn material_at(cells: &[Cell], x: usize, y: usize) -> u8 {
        cells[y * crate::grid::GRID_WIDTH + x].material()
    }

    fn sand_count(cells: &[Cell]) -> usize {
        cells
            .iter()
            .filter(|cell| cell.material() == crate::grid::SAND)
            .count()
    }

    #[test]
    fn gpu_required_is_only_the_digit_one() {
        assert!(!gpu_required(None));
        assert!(!gpu_required(Some("")));
        assert!(!gpu_required(Some("0")));
        assert!(gpu_required(Some("1")));
    }

    fn gpu_required(value: Option<&str>) -> bool {
        value == Some("1")
    }

    fn pixel(frame: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * width + x) * 4) as usize;
        [
            frame[index],
            frame[index + 1],
            frame[index + 2],
            frame[index + 3],
        ]
    }
}
