use std::future::Future;
use std::sync::Arc;

use winit::window::Window;

#[cfg(not(target_arch = "wasm32"))]
use crate::grid::{GRID_HEIGHT, GRID_WIDTH};
use crate::grid::{Grid, LETTERBOX, Size, letterbox};

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
    hold: bool,
    uniform: wgpu::Buffer,
    draw_groups: [wgpu::BindGroup; 2],
    pipeline: wgpu::RenderPipeline,
    sim_uniform: wgpu::Buffer,
    sim_groups: [wgpu::BindGroup; 2],
    sim_pipeline: wgpu::ComputePipeline,
    sim_chunks_pipeline: wgpu::ComputePipeline,
    brush_uniform: wgpu::Buffer,
    brush_groups: [wgpu::BindGroup; 2],
    brush_pipeline: wgpu::ComputePipeline,
    format: wgpu::TextureFormat,
    bloom: Option<BloomTargets>,
    egui: Option<egui_wgpu::Renderer>,
    // Bind groups borrow this buffer. The field keeps it alive.
    #[allow(dead_code)]
    materials: wgpu::Buffer,
    // Bind groups borrow this buffer. The field keeps it alive.
    #[allow(dead_code)]
    reactions: wgpu::Buffer,
    world_w: u32,
    world_h: u32,
    chunked: bool,
    awake: Vec<bool>,
    quiet: Vec<u8>,
    chunk_list: wgpu::Buffer,
    dirty: wgpu::Buffer,
    camera: Option<crate::grid::Camera>,
    // Bind groups borrow these buffers. The fields keep them alive.
    #[allow(dead_code)]
    particles: wgpu::Buffer,
    #[allow(dead_code)]
    claims: wgpu::Buffer,
    blast_groups: [wgpu::BindGroup; 2],
    blast_pipeline: wgpu::ComputePipeline,
    fly_pipeline: wgpu::ComputePipeline,
    clear_claims_pipeline: wgpu::ComputePipeline,
    claim_pipeline: wgpu::ComputePipeline,
    land_pipeline: wgpu::ComputePipeline,
    nudge_pipeline: wgpu::ComputePipeline,
}

struct Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Speck {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    word: u32,
    age: u32,
    _pad0: u32,
    _pad1: u32,
}

const _: () = assert!(std::mem::size_of::<Speck>() == 32);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    cell_x: f32,
    cell_y: f32,
    zoom: f32,
    grid_width: u32,
    grid_height: u32,
    frame_width: u32,
    frame_height: u32,
    _pad: u32,
}

const _: () = assert!(std::mem::size_of::<Uniforms>() == 32);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SimUniform {
    offset: u32,
    width: u32,
    height: u32,
    awake: u32,
    chunk_cols: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

const _: () = assert!(std::mem::size_of::<SimUniform>() == 32);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Stroke {
    pub from_x: i32,
    pub from_y: i32,
    pub to_x: i32,
    pub to_y: i32,
    pub radius: u32,
    pub material: u32,
    pub erase: u32,
    pub clear: u32,
}

const _: () = assert!(std::mem::size_of::<Stroke>() == 32);

pub(crate) struct EguiFrame {
    pub primitives: Vec<egui::ClippedPrimitive>,
    pub textures: egui::TexturesDelta,
    pub pixels_per_point: f32,
}

pub(crate) struct Scene {
    pub rgba: Vec<u8>,
    // The bloom test reads the linear frame. The window does not.
    #[allow(dead_code)]
    pub linear: Vec<u8>,
}

pub(crate) struct Paint {
    pub paused: bool,
    pub clear: bool,
    pub bloom: bool,
    pub passes: u32,
    pub stroke: Option<Stroke>,
}

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
        let (width, height) = crate::grid::world_size();
        Self::open(
            Some(window),
            Size {
                width: size.width,
                height: size.height,
            },
            (width as u32, height as u32),
        )
        .await
    }

    pub(crate) fn render_rgba(grid: &Grid, size: Size) -> Result<Vec<u8>, NoAdapter> {
        Ok(Self::render_scene(grid, size, false)?.rgba)
    }

    pub(crate) fn render_scene(grid: &Grid, size: Size, bloom: bool) -> Result<Scene, NoAdapter> {
        if size.width == 0 || size.height == 0 {
            return Ok(Scene {
                rgba: Vec::new(),
                linear: Vec::new(),
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut gpu = pollster::block_on(Self::open(
                None,
                size,
                (GRID_WIDTH as u32, GRID_HEIGHT as u32),
            ))?;
            Ok(gpu.read_scene(grid, size, bloom))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (grid, bloom);
            Err(NoAdapter { names: Vec::new() })
        }
    }

    pub(crate) fn resize(&mut self, size: Size) {
        self.size = size;
        self.reconfigure();
    }

    pub(crate) fn draw(
        &mut self,
        window: &Window,
        grid: &Grid,
        paint: Paint,
        ui: Option<EguiFrame>,
    ) {
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
                self.paint(&frame.texture, grid, &paint, ui);
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

    async fn open(
        window: Option<Arc<Window>>,
        size: Size,
        world: (u32, u32),
    ) -> Result<Self, NoAdapter> {
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

        let gpu_parts = resources(&device, format, world.0, world.1);
        let chunks = chunk_count(world.0, world.1);
        queue.write_buffer(
            &gpu_parts.materials,
            0,
            bytemuck::cast_slice(&crate::grid::MATERIALS),
        );
        queue.write_buffer(
            &gpu_parts.reactions,
            0,
            bytemuck::cast_slice(&crate::grid::REACTIONS),
        );
        let surface = surface.map(|surface| {
            let config = wgpu::SurfaceConfiguration {
                usage: surface_usage(),
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
            hold: false,
            uniform: gpu_parts.uniform,
            draw_groups: gpu_parts.draw_groups,
            pipeline: gpu_parts.pipeline,
            sim_uniform: gpu_parts.sim_uniform,
            sim_groups: gpu_parts.sim_groups,
            sim_pipeline: gpu_parts.sim_pipeline,
            sim_chunks_pipeline: gpu_parts.sim_chunks_pipeline,
            brush_uniform: gpu_parts.brush_uniform,
            brush_groups: gpu_parts.brush_groups,
            brush_pipeline: gpu_parts.brush_pipeline,
            format,
            bloom: None,
            egui: None,
            materials: gpu_parts.materials,
            reactions: gpu_parts.reactions,
            world_w: world.0,
            world_h: world.1,
            chunked: true,
            awake: vec![true; chunks],
            quiet: vec![0; chunks],
            chunk_list: gpu_parts.chunk_list,
            dirty: gpu_parts.dirty,
            camera: None,
            particles: gpu_parts.particles,
            claims: gpu_parts.claims,
            blast_groups: gpu_parts.blast_groups,
            blast_pipeline: gpu_parts.blast_pipeline,
            fly_pipeline: gpu_parts.fly_pipeline,
            clear_claims_pipeline: gpu_parts.clear_claims_pipeline,
            claim_pipeline: gpu_parts.claim_pipeline,
            land_pipeline: gpu_parts.land_pipeline,
            nudge_pipeline: gpu_parts.nudge_pipeline,
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

    fn paint(
        &mut self,
        texture: &wgpu::Texture,
        grid: &Grid,
        input: &Paint,
        ui: Option<EguiFrame>,
    ) {
        if !self.seeded {
            self.queue.write_buffer(&self.cells[0], 0, grid.as_bytes());
            self.front = 0;
            self.seeded = true;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = self.device.poll(wgpu::PollType::Poll);
        }
        if self.hold {
            self.hold = false;
            self.present(texture, self.front, input.bloom, ui);
            return;
        }
        if input.clear {
            self.apply_brush(Stroke {
                from_x: 0,
                from_y: 0,
                to_x: 0,
                to_y: 0,
                radius: 0,
                material: 0,
                erase: 0,
                clear: 1,
            });
        }
        if let Some(stroke) = input.stroke {
            self.apply_brush(stroke);
        }
        if input.paused {
            self.present(texture, self.front, input.bloom, ui);
            return;
        }
        for _ in 0..input.passes {
            let offset = self.passes % 2;
            self.step(offset);
            self.passes += 1;
        }
        self.present(texture, self.front, input.bloom, ui);
    }

    fn present(
        &mut self,
        texture: &wgpu::Texture,
        index: usize,
        bloom: bool,
        ui: Option<EguiFrame>,
    ) {
        if let Some(ui) = &ui {
            let mut uploads = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            self.prepare_egui(&mut uploads, ui);
            self.queue.submit([uploads.finish()]);
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.draw_bloom(&mut encoder, texture, index, bloom);
        if let Some(ui) = ui {
            self.paint_egui(&mut encoder, texture, &ui);
        }
        #[cfg(target_arch = "wasm32")]
        let probe = self.record_probe(&mut encoder, texture);
        self.queue.submit([encoder.finish()]);
        #[cfg(target_arch = "wasm32")]
        self.finish_probe(probe, texture.clone());
    }

    // `?probe=1` copies three canvas pixels into `window.grainsPixels`.
    // Screenshots of a WebGPU canvas stay empty, so the smoke test reads these.
    #[cfg(target_arch = "wasm32")]
    fn record_probe(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> Vec<(&'static str, wgpu::Buffer)> {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let frame = N.fetch_add(1, Ordering::Relaxed);
        if !crate::web::probe() || frame < 2 || PROBE_BUSY.swap(true, Ordering::Relaxed) {
            return Vec::new();
        }
        web_sys::console::log_1(&format!("probe copy {frame}").into());
        let spots = [
            (8u32, 8u32, "corner"),
            (120, 180, "panel"),
            (748, 348, "stroke"),
        ];
        let mut buffers = Vec::new();
        for (x, y, name) in spots {
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("probe"),
                size: 256,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(1),
                    },
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            buffers.push((name, buffer));
        }
        buffers
    }

    #[cfg(target_arch = "wasm32")]
    fn finish_probe(&self, buffers: Vec<(&'static str, wgpu::Buffer)>, texture: wgpu::Texture) {
        if buffers.is_empty() {
            return;
        }
        let report = std::rc::Rc::new(std::cell::RefCell::new(js_sys::Object::new()));
        let left = std::rc::Rc::new(std::cell::Cell::new(buffers.len()));
        for (name, buffer) in buffers {
            let mapped = buffer.clone();
            let report = std::rc::Rc::clone(&report);
            let left = std::rc::Rc::clone(&left);
            let keep = texture.clone();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    drop(keep);
                    if let Ok(()) = result
                        && let Ok(view) = mapped.slice(..).get_mapped_range()
                    {
                        let array = js_sys::Array::new();
                        for byte in &view[..4] {
                            array.push(&wasm_bindgen::JsValue::from(u32::from(*byte)));
                        }
                        let _ = js_sys::Reflect::set(&report.borrow(), &name.into(), &array);
                    }
                    if left.get() == 1 {
                        if let Some(window) = web_sys::window() {
                            let _ = js_sys::Reflect::set(
                                &window,
                                &"grainsPixels".into(),
                                &report.borrow(),
                            );
                        }
                        PROBE_BUSY.store(false, std::sync::atomic::Ordering::Relaxed);
                    }
                    left.set(left.get().saturating_sub(1));
                });
        }
    }

    fn prepare_egui(&mut self, encoder: &mut wgpu::CommandEncoder, ui: &EguiFrame) {
        if self.egui.is_none() {
            self.egui = Some(egui_wgpu::Renderer::new(
                &self.device,
                self.format,
                egui_wgpu::RendererOptions::default(),
            ));
        }
        let device = self.device.clone();
        let queue = self.queue.clone();
        let screen = self.egui_screen(ui);
        let renderer = self.egui.as_mut().unwrap();
        for (id, images) in &ui.textures.set {
            for image in images {
                renderer.update_texture(&device, &queue, *id, image);
            }
        }
        let _ = renderer.update_buffers(&device, &queue, encoder, &ui.primitives, &screen);
    }

    fn paint_egui(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        ui: &EguiFrame,
    ) {
        let screen = self.egui_screen(ui);
        // Solid rects in the panel sample egui's font atlas at uv 0. On web that
        // sample stays empty, so the window never covers the sky. Draw the vertex
        // colors first; the textured pass then adds glyphs when the atlas is ready.
        #[cfg(target_arch = "wasm32")]
        self.draw_widget_colors(encoder, texture, ui);
        let renderer = self.egui.as_mut().unwrap();
        {
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            renderer.render(&mut pass, &ui.primitives, &screen);
            drop(pass);
        }
        for id in &ui.textures.free {
            renderer.free_texture(id);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn draw_widget_colors(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        ui: &EguiFrame,
    ) {
        let mut vertices: Vec<u8> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut base = 0u32;
        for prim in &ui.primitives {
            let egui::epaint::Primitive::Mesh(mesh) = &prim.primitive else {
                continue;
            };
            vertices.extend_from_slice(bytemuck::cast_slice(&mesh.vertices));
            indices.extend(mesh.indices.iter().map(|index| index + base));
            base += mesh.vertices.len() as u32;
        }
        if indices.is_empty() {
            return;
        }
        let pipeline = widget_color_pipeline(&self.device, self.format);
        let uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("widget-screen"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let screen = [self.size.width as f32, self.size.height as f32, 0.0, 0.0];
        self.queue
            .write_buffer(&uniform, 0, bytemuck::cast_slice(&screen));
        let layout = pipeline.get_bind_group_layout(0);
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("widget-screen"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("widget-vertices"),
            size: vertices.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("widget-indices"),
            size: (indices.len() * 4) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&vertex_buffer, 0, &vertices);
        self.queue
            .write_buffer(&index_buffer, 0, bytemuck::cast_slice(&indices));
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("widgets"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
    }

    fn egui_screen(&self, ui: &EguiFrame) -> egui_wgpu::ScreenDescriptor {
        egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.size.width, self.size.height],
            pixels_per_point: ui.pixels_per_point.max(0.1),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_scene(&mut self, grid: &Grid, size: Size, bloom: bool) -> Scene {
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
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.queue.write_buffer(&self.cells[0], 0, grid.as_bytes());
        self.draw_bloom(&mut encoder, &texture, 0, bloom);
        let rgba = stage_copy(&self.device, &mut encoder, &texture, size, 4);
        let linear = bloom.then(|| {
            let lit = &self.bloom.as_ref().unwrap().lit;
            stage_copy(&self.device, &mut encoder, lit, size, 8)
        });
        self.queue.submit([encoder.finish()]);
        Scene {
            rgba: take_pixels(&self.device, &rgba, size, 4),
            linear: linear
                .map(|buffer| take_pixels(&self.device, &buffer, size, 8))
                .unwrap_or_default(),
        }
    }

    fn step(&mut self, offset: u32) {
        if self.chunked && self.awake.iter().all(|on| !on) {
            return;
        }
        let src = self.front;
        let dst = src ^ 1;
        let list = if self.chunked {
            self.live_chunks()
        } else {
            Vec::new()
        };
        let awake = if self.chunked {
            list.len() as u32
        } else {
            u32::MAX
        };
        let sim = SimUniform {
            offset,
            width: self.world_w,
            height: self.world_h,
            awake,
            chunk_cols: chunk_cols(self.world_w) as u32,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        };
        self.queue
            .write_buffer(&self.sim_uniform, 0, bytemuck::bytes_of(&sim));
        if self.chunked && !list.is_empty() {
            let bytes: Vec<u8> = list.iter().flat_map(|word| word.to_le_bytes()).collect();
            self.queue.write_buffer(&self.chunk_list, 0, &bytes);
            self.queue
                .write_buffer(&self.dirty, 0, &vec![0u8; self.awake.len() * 4]);
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let bytes = (self.world_w as u64) * (self.world_h as u64) * 4;
        encoder.copy_buffer_to_buffer(&self.cells[src], 0, &self.cells[dst], 0, bytes);
        if self.chunked {
            let count = list.len() as u32;
            self.dispatch_cells(&mut encoder, &self.blast_pipeline, src, count);
            self.dispatch_blocks(&mut encoder, src, count);
            self.dispatch_cells(&mut encoder, &self.fly_pipeline, src, count);
            self.dispatch_cells(&mut encoder, &self.clear_claims_pipeline, src, count);
            self.dispatch_cells(&mut encoder, &self.claim_pipeline, src, count);
            self.dispatch_cells(&mut encoder, &self.land_pipeline, src, count);
            self.dispatch_cells(&mut encoder, &self.nudge_pipeline, src, count);
        } else {
            self.dispatch(&mut encoder, &self.blast_pipeline, src);
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.sim_pipeline);
                pass.set_bind_group(0, &self.sim_groups[src], &[]);
                let (groups_x, groups_y) = workgroups(self.world_w, self.world_h, offset);
                pass.dispatch_workgroups(groups_x, groups_y, 1);
            }
            self.dispatch(&mut encoder, &self.fly_pipeline, src);
            self.dispatch(&mut encoder, &self.clear_claims_pipeline, src);
            self.dispatch(&mut encoder, &self.claim_pipeline, src);
            self.dispatch(&mut encoder, &self.land_pipeline, src);
            self.dispatch(&mut encoder, &self.nudge_pipeline, src);
        }
        self.queue.submit([encoder.finish()]);
        self.front = dst;
        if self.chunked {
            self.note_changes();
        }
    }

    fn dispatch_blocks(&self, encoder: &mut wgpu::CommandEncoder, src: usize, count: u32) {
        if count == 0 {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: None,
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.sim_chunks_pipeline);
        pass.set_bind_group(0, &self.sim_groups[src], &[]);
        pass.dispatch_workgroups(4, 4, count);
    }

    fn dispatch_cells(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::ComputePipeline,
        src: usize,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: None,
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.blast_groups[src], &[]);
        pass.dispatch_workgroups(8, 8, count);
    }

    fn live_chunks(&self) -> Vec<u32> {
        let cols = chunk_cols(self.world_w);
        let rows = chunk_rows(self.world_h);
        let mut live = vec![false; cols * rows];
        for y in 0..rows {
            for x in 0..cols {
                if !self.awake[y * cols + x] {
                    continue;
                }
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        if nx >= 0 && ny >= 0 && (nx as usize) < cols && (ny as usize) < rows {
                            live[ny as usize * cols + nx as usize] = true;
                        }
                    }
                }
            }
        }
        live.iter()
            .enumerate()
            .filter(|(_, on)| **on)
            .map(|(index, _)| {
                let cx = (index % cols) as u32;
                let cy = (index / cols) as u32;
                cx | (cy << 16)
            })
            .collect()
    }

    fn wake_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        let cols = chunk_cols(self.world_w) as i32;
        let rows = chunk_rows(self.world_h) as i32;
        let c0x = (x0.div_euclid(crate::grid::CHUNK as i32)).clamp(0, cols - 1);
        let c1x = (x1.div_euclid(crate::grid::CHUNK as i32)).clamp(0, cols - 1);
        let c0y = (y0.div_euclid(crate::grid::CHUNK as i32)).clamp(0, rows - 1);
        let c1y = (y1.div_euclid(crate::grid::CHUNK as i32)).clamp(0, rows - 1);
        for cy in (c0y - 1)..=(c1y + 1) {
            for cx in (c0x - 1)..=(c1x + 1) {
                if cx < 0 || cy < 0 || cx >= cols || cy >= rows {
                    continue;
                }
                let index = cy as usize * cols as usize + cx as usize;
                self.awake[index] = true;
                self.quiet[index] = 0;
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn note_changes(&mut self) {
        let bytes = (self.awake.len() * 4) as u64;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_buffer(&self.dirty, 0, &buffer, 0, bytes);
        self.queue.submit([encoder.finish()]);
        let mapped = map_buffer(&self.device, &buffer);
        let dirty: Vec<u32> = bytemuck::cast_slice(&mapped).to_vec();
        let cols = chunk_cols(self.world_w);
        let rows = chunk_rows(self.world_h);
        let mut wake = vec![false; dirty.len()];
        for (index, flag) in dirty.iter().enumerate() {
            if *flag == 0 {
                continue;
            }
            let x = index % cols;
            let y = index / cols;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx >= 0 && ny >= 0 && (nx as usize) < cols && (ny as usize) < rows {
                        wake[ny as usize * cols + nx as usize] = true;
                    }
                }
            }
        }
        for (index, wake_it) in wake.iter().enumerate() {
            if *wake_it {
                self.awake[index] = true;
                self.quiet[index] = 0;
            } else if self.awake[index] {
                self.quiet[index] = self.quiet[index].saturating_add(1);
                if self.quiet[index] >= 4 {
                    self.awake[index] = false;
                }
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn note_changes(&mut self) {}

    fn dispatch(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::ComputePipeline,
        src: usize,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: None,
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.blast_groups[src], &[]);
        let (groups_x, groups_y) = cell_groups(self.world_w, self.world_h);
        pass.dispatch_workgroups(groups_x, groups_y, 1);
    }

    fn apply_brush(&mut self, stroke: Stroke) {
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Upload {
            stroke: Stroke,
            width: u32,
            height: u32,
            pad0: u32,
            pad1: u32,
        }
        let upload = Upload {
            stroke,
            width: self.world_w,
            height: self.world_h,
            pad0: 0,
            pad1: 0,
        };
        self.queue
            .write_buffer(&self.brush_uniform, 0, bytemuck::bytes_of(&upload));
        if stroke.clear != 0 {
            self.awake.fill(true);
            self.quiet.fill(0);
        } else {
            let reach = stroke.radius as i32;
            self.wake_rect(
                stroke.from_x.min(stroke.to_x) - reach,
                stroke.from_y.min(stroke.to_y) - reach,
                stroke.from_x.max(stroke.to_x) + reach,
                stroke.from_y.max(stroke.to_y) + reach,
            );
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.brush_pipeline);
            pass.set_bind_group(0, &self.brush_groups[self.front], &[]);
            let (groups_x, groups_y) = cell_groups(self.world_w, self.world_h);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }
        self.queue.submit([encoder.finish()]);
    }

    #[cfg(test)]
    pub(crate) fn preview(grid: &Grid, passes: u32, size: Size) -> Result<Vec<u8>, NoAdapter> {
        let mut gpu = pollster::block_on(Self::open(
            None,
            size,
            (GRID_WIDTH as u32, GRID_HEIGHT as u32),
        ))?;
        gpu.queue.write_buffer(&gpu.cells[0], 0, grid.as_bytes());
        for pass in 0..passes {
            gpu.step(pass % 2);
        }
        Ok(gpu.read_rgba_from(gpu.front, size))
    }

    #[cfg(test)]
    fn read_rgba_from(&mut self, index: usize, size: Size) -> Vec<u8> {
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
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.draw_bloom(&mut encoder, &texture, index, true);
        let stride = padded_row_bytes(size.width, 4);
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
    pub(crate) fn stamp(grid: &Grid, stroke: Stroke) -> Result<Vec<crate::grid::Cell>, NoAdapter> {
        let mut gpu = pollster::block_on(Self::open(
            None,
            Size {
                width: GRID_WIDTH as u32,
                height: GRID_HEIGHT as u32,
            },
            (GRID_WIDTH as u32, GRID_HEIGHT as u32),
        ))?;
        gpu.queue.write_buffer(&gpu.cells[0], 0, grid.as_bytes());
        gpu.front = 0;
        gpu.apply_brush(stroke);
        Ok(gpu.read_cells(0))
    }

    #[cfg(test)]
    pub(crate) fn simulate(grid: &Grid, passes: u32) -> Result<Vec<crate::grid::Cell>, NoAdapter> {
        let gpu = pollster::block_on(Self::open(
            None,
            Size {
                width: GRID_WIDTH as u32,
                height: GRID_HEIGHT as u32,
            },
            (GRID_WIDTH as u32, GRID_HEIGHT as u32),
        ))?;
        gpu.queue.write_buffer(&gpu.cells[0], 0, grid.as_bytes());
        let mut gpu = gpu;
        for pass in 0..passes {
            gpu.step(pass % 2);
        }
        Ok(gpu.read_cells(gpu.front))
    }

    pub(crate) fn store(&mut self, grid: &Grid) {
        self.queue
            .write_buffer(&self.cells[self.front], 0, grid.as_bytes());
        self.seeded = true;
        self.hold = true;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn snapshot(&self) -> Vec<u32> {
        let cells = self.read_cells(self.front);
        bytemuck::cast_slice(&cells).to_vec()
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn snapshot_then(&self, done: impl FnOnce(Result<Vec<u32>, String>) + 'static) {
        let bytes = (self.world_w as u64) * (self.world_h as u64) * 4;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_buffer(&self.cells[self.front], 0, &buffer, 0, bytes);
        self.queue.submit([encoder.finish()]);
        let mapped = buffer.clone();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let outcome = match result {
                    Ok(()) => match mapped.slice(..).get_mapped_range() {
                        Ok(view) => {
                            let words = bytemuck::cast_slice(&view).to_vec();
                            drop(view);
                            mapped.unmap();
                            Ok(words)
                        }
                        Err(err) => Err(format!("could not read the grid: {err}")),
                    },
                    Err(err) => Err(format!("could not read the grid: {err}")),
                };
                done(outcome);
            });
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_cells(&self, index: usize) -> Vec<crate::grid::Cell> {
        let bytes = (self.world_w as u64) * (self.world_h as u64) * 4;
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

    #[cfg(test)]
    fn read_specks(&self) -> Vec<Speck> {
        let bytes =
            (self.world_w as u64) * (self.world_h as u64) * std::mem::size_of::<Speck>() as u64;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_buffer(&self.particles, 0, &buffer, 0, bytes);
        self.queue.submit([encoder.finish()]);
        let mapped = map_buffer(&self.device, &buffer);
        bytemuck::cast_slice(&mapped).to_vec()
    }

    fn draw_bloom(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        index: usize,
        bloom: bool,
    ) {
        self.ensure_bloom();
        let Some(_) = self.prepare_frame(self.size) else {
            return;
        };
        let hdr_view = self
            .bloom
            .as_ref()
            .unwrap()
            .hdr
            .create_view(&Default::default());
        let glow_view = self
            .bloom
            .as_ref()
            .unwrap()
            .glow
            .create_view(&Default::default());
        {
            let bloom = self.bloom.as_ref().unwrap();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[
                    Some(color_attachment(&hdr_view, letterbox_color())),
                    Some(color_attachment(&glow_view, wgpu::Color::TRANSPARENT)),
                ],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&bloom.grid);
            pass.set_bind_group(0, &self.draw_groups[index], &[]);
            pass.draw(0..3, 0..1);
        }
        if bloom {
            self.bloom_pass(
                encoder,
                Stage::Bright,
                &self.bloom.as_ref().unwrap().low_view[0],
            );
            self.bloom_pass(
                encoder,
                Stage::BlurX,
                &self.bloom.as_ref().unwrap().low_view[1],
            );
            self.bloom_pass(
                encoder,
                Stage::BlurY,
                &self.bloom.as_ref().unwrap().low_view[0],
            );
            self.bloom_pass(
                encoder,
                Stage::Composite,
                &self.bloom.as_ref().unwrap().lit_view,
            );
        } else {
            self.bloom_pass(
                encoder,
                Stage::Plain,
                &self.bloom.as_ref().unwrap().lit_view,
            );
        }
        let target_view = target.create_view(&Default::default());
        self.bloom_pass(encoder, Stage::Blit, &target_view);
    }

    fn bloom_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        stage: Stage,
        view: &wgpu::TextureView,
    ) {
        let bloom = self.bloom.as_ref().unwrap();
        let (pipeline, group) = match stage {
            Stage::Bright => (&bloom.bright, &bloom.bright_group),
            Stage::BlurX => (&bloom.blur_x, &bloom.blur_from[0]),
            Stage::BlurY => (&bloom.blur_y, &bloom.blur_from[1]),
            Stage::Composite => (&bloom.composite, &bloom.composite_group),
            Stage::Plain => (&bloom.composite, &bloom.plain_group),
            Stage::Blit => (&bloom.blit, &bloom.blit_group),
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(color_attachment(view, wgpu::Color::TRANSPARENT))],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }

    pub(crate) fn set_camera(&mut self, camera: crate::grid::Camera) {
        self.camera = Some(camera);
    }

    pub(crate) fn wake_all(&mut self) {
        self.awake.fill(true);
        self.quiet.fill(0);
    }

    #[cfg(test)]
    fn chunk_awake(&self, x: usize, y: usize) -> bool {
        let cols = chunk_cols(self.world_w);
        self.awake[y * cols + x]
    }

    pub(crate) fn chunk_stats(&self) -> (u32, u32) {
        let awake = self.awake.iter().filter(|on| **on).count() as u32;
        (awake, self.awake.len() as u32)
    }

    #[cfg(test)]
    pub(crate) fn set_chunked(&mut self, chunked: bool) {
        self.chunked = chunked;
    }

    fn prepare_frame(&self, size: Size) -> Option<(f32, f32, f32, f32)> {
        if size.width == 0 || size.height == 0 {
            return None;
        }
        let (cell_x, cell_y, zoom) = if let Some(camera) = self.camera {
            (camera.cell_x, camera.cell_y, camera.zoom)
        } else {
            let fit = letterbox(size)?;
            let zoom = fit.scale as f32;
            (
                -(fit.origin_x as f32) / zoom,
                -(fit.origin_y as f32) / zoom,
                zoom,
            )
        };
        let uniforms = Uniforms {
            cell_x,
            cell_y,
            zoom,
            grid_width: self.world_w,
            grid_height: self.world_h,
            frame_width: size.width,
            frame_height: size.height,
            _pad: 0,
        };
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniforms));
        Some((0.0, 0.0, size.width as f32, size.height as f32))
    }

    fn ensure_bloom(&mut self) {
        if self
            .bloom
            .as_ref()
            .is_some_and(|bloom| bloom.frame == self.size)
        {
            return;
        }
        let layout = self.pipeline.get_bind_group_layout(0);
        self.bloom = Some(bloom_targets(&self.device, self.format, &layout, self.size));
        self.clear_black();
    }

    fn clear_black(&self) {
        let Some(bloom) = &self.bloom else {
            return;
        };
        let view = bloom.black.create_view(&Default::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(color_attachment(&view, wgpu::Color::TRANSPARENT))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.queue.submit([encoder.finish()]);
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
    sim_chunks_pipeline: wgpu::ComputePipeline,
    brush_uniform: wgpu::Buffer,
    brush_groups: [wgpu::BindGroup; 2],
    brush_pipeline: wgpu::ComputePipeline,
    materials: wgpu::Buffer,
    reactions: wgpu::Buffer,
    particles: wgpu::Buffer,
    claims: wgpu::Buffer,
    chunk_list: wgpu::Buffer,
    dirty: wgpu::Buffer,
    blast_groups: [wgpu::BindGroup; 2],
    blast_pipeline: wgpu::ComputePipeline,
    fly_pipeline: wgpu::ComputePipeline,
    clear_claims_pipeline: wgpu::ComputePipeline,
    claim_pipeline: wgpu::ComputePipeline,
    land_pipeline: wgpu::ComputePipeline,
    nudge_pipeline: wgpu::ComputePipeline,
}

fn resources(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    world_w: u32,
    world_h: u32,
) -> Parts {
    let cells_len = world_w as usize * world_h as usize;
    let chunks = chunk_count(world_w, world_h);
    let usage =
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
    let cells = [0, 1].map(|_| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (cells_len * 4) as u64,
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
        size: 32,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let chunk_list = zeroed_buffer(device, (chunks.max(1) * 4) as u64);
    let dirty = zeroed_buffer(device, (chunks.max(1) * 4) as u64);
    let materials = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (crate::grid::MATERIAL_COUNT * std::mem::size_of::<crate::grid::MaterialInfo>())
            as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let reactions = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (crate::grid::REACTIONS.len() * std::mem::size_of::<crate::grid::Reaction>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
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
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
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
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &materials,
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
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            storage_entry(5),
            storage_entry(6),
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
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &materials,
                        offset: 0,
                        size: None,
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &reactions,
                        offset: 0,
                        size: None,
                    }),
                },
                buffer_entry(5, &chunk_list),
                buffer_entry(6, &dirty),
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
    let sim_chunks_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&sim_pipeline_layout),
        module: &sim_shader,
        entry_point: Some("step_chunks"),
        compilation_options: Default::default(),
        cache: None,
    });
    let brush_uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 48,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let brush_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
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
    let brush_groups = [0, 1].map(|index| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &brush_layout,
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
                        buffer: &brush_uniform,
                        offset: 0,
                        size: None,
                    }),
                },
            ],
        })
    });
    let brush_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/brush.wgsl").into()),
    });
    let brush_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&brush_layout)],
        immediate_size: 0,
    });
    let brush_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&brush_pipeline_layout),
        module: &brush_shader,
        entry_point: Some("paint"),
        compilation_options: Default::default(),
        cache: None,
    });
    let particles = zeroed_buffer(device, (cells_len * 32) as u64);
    let claims = zeroed_buffer(device, (cells_len * 4) as u64);
    let blast_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/blast.wgsl").into()),
    });
    let blast_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            storage_entry(0),
            storage_entry(1),
            storage_entry(2),
            storage_entry(3),
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            storage_entry(5),
        ],
    });
    let blast_groups = [0, 1].map(|index| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &blast_layout,
            entries: &[
                buffer_entry(0, &cells[index]),
                buffer_entry(1, &cells[index ^ 1]),
                buffer_entry(2, &particles),
                buffer_entry(3, &claims),
                buffer_entry(4, &sim_uniform),
                buffer_entry(5, &chunk_list),
            ],
        })
    });
    let blast_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&blast_layout)],
        immediate_size: 0,
    });
    let blast_pipe = |entry: &str| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&blast_pipeline_layout),
            module: &blast_shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    Parts {
        cells,
        uniform,
        draw_groups,
        pipeline,
        sim_uniform,
        sim_groups,
        sim_pipeline,
        sim_chunks_pipeline,
        brush_uniform,
        brush_groups,
        brush_pipeline,
        materials,
        reactions,
        particles,
        claims,
        chunk_list,
        dirty,
        blast_groups,
        blast_pipeline: blast_pipe("blast"),
        fly_pipeline: blast_pipe("fly"),
        clear_claims_pipeline: blast_pipe("clear_claims"),
        claim_pipeline: blast_pipe("claim"),
        land_pipeline: blast_pipe("land"),
        nudge_pipeline: blast_pipe("nudge"),
    }
}

fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: false },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn buffer_entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer,
            offset: 0,
            size: None,
        }),
    }
}

fn zeroed_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: true,
    });
    let zeros = vec![0u8; usize::try_from(size).expect("buffer size")];
    buffer
        .slice(..)
        .get_mapped_range_mut()
        .expect("zero buffer")
        .copy_from_slice(&zeros);
    buffer.unmap();
    buffer
}

fn cell_groups(width: u32, height: u32) -> (u32, u32) {
    (width.div_ceil(8), height.div_ceil(8))
}

fn workgroups(width: u32, height: u32, offset: u32) -> (u32, u32) {
    let blocks_x = (width - offset) / 2;
    let blocks_y = (height - offset) / 2;
    (blocks_x.div_ceil(8), blocks_y.div_ceil(8))
}

fn chunk_cols(width: u32) -> usize {
    (width as usize).div_ceil(crate::grid::CHUNK)
}

fn chunk_rows(height: u32) -> usize {
    (height as usize).div_ceil(crate::grid::CHUNK)
}

fn chunk_count(width: u32, height: u32) -> usize {
    chunk_cols(width) * chunk_rows(height)
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(target_arch = "wasm32")]
static PROBE_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_arch = "wasm32")]
fn widget_color_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    use std::cell::RefCell;
    thread_local! {
        static PIPE: RefCell<Option<wgpu::RenderPipeline>> = const { RefCell::new(None) };
    }
    PIPE.with(|slot| {
        if slot.borrow().is_none() {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("widgets"),
                source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/widgets.wgsl").into()),
            });
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("widget-screen"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("widgets"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let vertex_layout = wgpu::VertexBufferLayout {
                array_stride: 20,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 8,
                        shader_location: 1,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Unorm8x4,
                        offset: 16,
                        shader_location: 2,
                    },
                ],
            };
            *slot.borrow_mut() = Some(device.create_render_pipeline(
                &wgpu::RenderPipelineDescriptor {
                    label: Some("widgets"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[Some(vertex_layout)],
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
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                },
            ));
        }
        slot.borrow().clone().unwrap()
    })
}

fn surface_usage() -> wgpu::TextureUsages {
    let usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
    #[cfg(target_arch = "wasm32")]
    if crate::web::probe() {
        return usage | wgpu::TextureUsages::COPY_SRC;
    }
    usage
}

fn color_attachment(
    view: &wgpu::TextureView,
    clear: wgpu::Color,
) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(clear),
            store: wgpu::StoreOp::Store,
        },
    }
}

enum Stage {
    Bright,
    BlurX,
    BlurY,
    Composite,
    Plain,
    Blit,
}

struct BloomTargets {
    frame: Size,
    grid: wgpu::RenderPipeline,
    bright: wgpu::RenderPipeline,
    blur_x: wgpu::RenderPipeline,
    blur_y: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    bright_group: wgpu::BindGroup,
    blur_from: [wgpu::BindGroup; 2],
    composite_group: wgpu::BindGroup,
    plain_group: wgpu::BindGroup,
    blit_group: wgpu::BindGroup,
    // Groups borrow the views, and the views borrow the textures.
    #[allow(dead_code)]
    hdr_view: wgpu::TextureView,
    #[allow(dead_code)]
    glow_view: wgpu::TextureView,
    low_view: [wgpu::TextureView; 2],
    lit_view: wgpu::TextureView,
    #[allow(dead_code)]
    hdr: wgpu::Texture,
    #[allow(dead_code)]
    glow: wgpu::Texture,
    #[allow(dead_code)]
    low: [wgpu::Texture; 2],
    #[allow(dead_code)]
    sampler: wgpu::Sampler,
    // Sampled when bloom is off so the tonemap matches the bloom path.
    #[allow(dead_code)]
    black: wgpu::Texture,
    // Native frame readback copies this. The window only samples the view.
    #[allow(dead_code)]
    lit: wgpu::Texture,
}

fn bloom_targets(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    draw_layout: &wgpu::BindGroupLayout,
    frame: Size,
) -> BloomTargets {
    let low_w = frame.width.div_ceil(4).max(1);
    let low_h = frame.height.div_ceil(4).max(1);
    let hdr = float_target(device, frame.width, frame.height, false);
    let glow = float_target(device, frame.width, frame.height, false);
    let low = [
        float_target(device, low_w, low_h, false),
        float_target(device, low_w, low_h, false),
    ];
    let lit = float_target(device, frame.width, frame.height, true);
    let black = float_target(device, 1, 1, false);
    let hdr_view = hdr.create_view(&Default::default());
    let glow_view = glow.create_view(&Default::default());
    let low_view = [
        low[0].create_view(&Default::default()),
        low[1].create_view(&Default::default()),
    ];
    let lit_view = lit.create_view(&Default::default());
    let black_view = black.create_view(&Default::default());
    let draw_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/draw.wgsl").into()),
    });
    let bloom_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/bloom.wgsl").into()),
    });
    let grid_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(draw_layout)],
        immediate_size: 0,
    });
    let grid = hdr_pipeline(device, &draw_shader, &grid_layout);
    let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[texture_entry(0)],
    });
    let tex_pipeline = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&tex_layout)],
        immediate_size: 0,
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: None,
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            texture_entry(0),
            texture_entry(1),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let composite_pipeline_layout =
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&composite_layout)],
            immediate_size: 0,
        });
    let float_format = wgpu::TextureFormat::Rgba16Float;
    BloomTargets {
        frame,
        bright_group: texture_group(device, &tex_layout, &glow_view),
        blur_from: [
            texture_group(device, &tex_layout, &low_view[0]),
            texture_group(device, &tex_layout, &low_view[1]),
        ],
        composite_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &composite_layout,
            entries: &[
                texture_binding(0, &hdr_view),
                texture_binding(1, &low_view[0]),
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        }),
        plain_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &composite_layout,
            entries: &[
                texture_binding(0, &hdr_view),
                texture_binding(1, &black_view),
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        }),
        blit_group: texture_group(device, &tex_layout, &lit_view),
        grid,
        bright: fullscreen_pipeline(device, &bloom_shader, "bright", &tex_pipeline, float_format),
        blur_x: fullscreen_pipeline(device, &bloom_shader, "blur_x", &tex_pipeline, float_format),
        blur_y: fullscreen_pipeline(device, &bloom_shader, "blur_y", &tex_pipeline, float_format),
        composite: fullscreen_pipeline(
            device,
            &bloom_shader,
            "composite",
            &composite_pipeline_layout,
            float_format,
        ),
        blit: fullscreen_pipeline(device, &bloom_shader, "blit", &tex_pipeline, format),
        hdr_view,
        glow_view,
        low_view,
        lit_view,
        hdr,
        glow,
        low,
        sampler,
        black,
        lit,
    }
}

fn float_target(device: &wgpu::Device, width: u32, height: u32, copy: bool) -> wgpu::Texture {
    let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    if copy {
        usage |= wgpu::TextureUsages::COPY_SRC;
    }
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage,
        view_formats: &[],
    })
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn texture_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[texture_binding(0, view)],
    })
}

fn texture_binding(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

fn fullscreen_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    entry: &str,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
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
            module: shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn hdr_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
) -> wgpu::RenderPipeline {
    let target = wgpu::ColorTargetState {
        format: wgpu::TextureFormat::Rgba16Float,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
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
            module: shader,
            entry_point: Some("fs_hdr"),
            compilation_options: Default::default(),
            targets: &[Some(target.clone()), Some(target)],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn padded_row_bytes(width: u32, bytes_per_pixel: u32) -> u32 {
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    (width * bytes_per_pixel).div_ceil(align) * align
}

#[cfg(not(target_arch = "wasm32"))]
fn stage_copy(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    size: Size,
    bytes_per_pixel: u32,
) -> wgpu::Buffer {
    let stride = padded_row_bytes(size.width, bytes_per_pixel);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(stride) * u64::from(size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
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
    buffer
}

#[cfg(not(target_arch = "wasm32"))]
fn take_pixels(
    device: &wgpu::Device,
    buffer: &wgpu::Buffer,
    size: Size,
    bytes_per_pixel: u32,
) -> Vec<u8> {
    let stride = padded_row_bytes(size.width, bytes_per_pixel);
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
    let row = (size.width * bytes_per_pixel) as usize;
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
            (56, 44) => Cell::pack(0x0b, 0x02, 0x03, 0x04),
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
        assert_ne!(
            pixel(&frame, 512, 1, 0),
            pixel(&frame, 512, 400, 200),
            "empty background is flat"
        );

        let Some(frame) = colours(crate::render_rgba(
            &grid,
            Size {
                width: 1024,
                height: 600,
            },
        )) else {
            return;
        };
        assert_ne!(
            pixel(&frame, 1024, 0, 0),
            pixel(&frame, 1024, 1023, 599),
            "letterbox is flat"
        );
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
        asserted(400, 200, &frame, 0, 0, [0x0b, 0x02, 0x03, 0xFF]);
        asserted(400, 200, &frame, 0, 1, [0x10, 0x20, 0x30, 0xFF]);
    }

    #[test]
    fn a_saved_world_round_trips_through_the_gpu() {
        use crate::grid::{SAND, STONE};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(0, 0, 0, 0));
        grid.set(3, 4, Cell::pack(SAND, 0xab, 0x11, 0x04));
        grid.set(8, 9, Cell::pack(STONE, 0x3c, 0, 0));
        let mut gpu = match pollster::block_on(super::Gpu::open(
            None,
            Size {
                width: 512,
                height: 288,
            },
            (512, 288),
        )) {
            Ok(gpu) => gpu,
            Err(err) if gpu_required(std::env::var("GRAINS_REQUIRE_GPU").ok().as_deref()) => {
                panic!("GRAINS_REQUIRE_GPU=1 and no adapter was found: {err}")
            }
            Err(err) => {
                eprintln!("world round trip skipped: {err}");
                return;
            }
        };
        gpu.store(&grid);
        let before = gpu.snapshot();
        let file = crate::world::encode(&before, 512, 288).expect("encode");
        gpu.apply_brush(super::Stroke {
            from_x: 0,
            from_y: 0,
            to_x: 0,
            to_y: 0,
            radius: 0,
            material: 0,
            erase: 0,
            clear: 1,
        });
        let cleared = gpu.snapshot();
        assert_eq!(cleared[4 * 512 + 3], 0, "clear left the sand cell");
        let saved = crate::world::decode(&file).expect("decode");
        let loaded = crate::grid::Grid::from_words(&saved.words).expect("grid");
        gpu.store(&loaded);
        let after = gpu.snapshot();
        assert_eq!(after, before);
        assert_eq!(
            after[4 * 512 + 3],
            Cell::pack(SAND, 0xab, 0x11, 0x04).to_bits()
        );
        eprintln!("asserted the gpu buffer matched after save, clear, and load");
    }

    #[test]
    fn tnt_falls_like_sand() {
        use crate::grid::{EMPTY, STONE, TNT};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        for x in 0..16 {
            grid.set(x, 40, Cell::pack(STONE, 0, 0, 0));
        }
        grid.set(8, 36, Cell::pack(TNT, 0x11, 0, 0));
        let Some((cells, _)) = play(&grid, 16) else {
            return;
        };
        let landed = (0..crate::grid::GRID_HEIGHT)
            .find(|y| material_at(&cells, 8, *y) == TNT)
            .expect("tnt vanished");
        assert!(landed > 36, "tnt stayed at {landed}");
        eprintln!("asserted tnt fell to {landed}");
    }

    #[test]
    fn a_blast_clears_a_crater_and_the_stone_holds() {
        use crate::grid::{FIRE, STONE, TNT};
        let grid = blast_scene();
        let Some((cells, _)) = play(&grid, 2) else {
            return;
        };
        assert_eq!(count_material(&cells, TNT), 0, "tnt survived the blast");
        assert_eq!(count_material(&cells, STONE), 5);
        assert_eq!(material_at(&cells, 28, 120), STONE);
        let crater = material_at(&cells, 32, 120);
        assert!(
            crater == FIRE || crater == 0,
            "the crater at the tnt was {crater}"
        );
        eprintln!("asserted the crater cleared and the stone wall held");
    }

    #[test]
    fn a_chain_of_tnt_all_detonates() {
        use crate::grid::{EMPTY, FIRE, TNT};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        grid.set(10, 80, Cell::pack(FIRE, 0, 24, 0));
        for x in 11..=18 {
            grid.set(x, 80, Cell::pack(TNT, 0, 0, 0));
        }
        let Some((cells, _)) = play(&grid, 4) else {
            return;
        };
        assert_eq!(count_material(&cells, TNT), 0);
        eprintln!("asserted the tnt chain detonated");
    }

    #[test]
    fn flung_material_lands_and_the_blast_stays_finite() {
        use crate::grid::{SAND, STONE, TNT, WATER};
        let grid = blast_scene();
        let Some((cells, specks)) = play(&grid, 80) else {
            return;
        };
        assert_eq!(count_material(&cells, SAND), 2);
        assert_eq!(count_material(&cells, WATER), 2);
        assert_eq!(count_material(&cells, STONE), 5);
        assert_eq!(count_material(&cells, TNT), 0);
        assert!(
            cells
                .iter()
                .any(|cell| cell.material() == SAND && cell.variation() == 0x21)
        );
        assert!(
            cells
                .iter()
                .any(|cell| cell.material() == SAND && cell.variation() == 0x22)
        );
        assert!(specks.iter().all(|speck| speck.word == 0));
        assert!(specks.iter().all(|speck| {
            speck.x.is_finite()
                && speck.y.is_finite()
                && speck.vx.is_finite()
                && speck.vy.is_finite()
        }));
        let words: Vec<u32> = cells.iter().map(|cell| cell.to_bits()).collect();
        let settled = crate::grid::Grid::from_words(&words).expect("grid");
        let Some(frame) = colours(super::Gpu::render_scene(
            &settled,
            Size {
                width: 512,
                height: 288,
            },
            true,
        )) else {
            return;
        };
        assert_finite(&frame.linear);
        eprintln!("asserted flung sand and water landed, with no non-finite values");
    }

    fn blast_scene() -> crate::grid::Grid {
        use crate::grid::{EMPTY, FIRE, SAND, STONE, TNT, WATER};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        for y in 118..=122 {
            grid.set(28, y, Cell::pack(STONE, 0, 0, 0));
        }
        grid.set(30, 120, Cell::pack(FIRE, 0, 24, 0));
        for x in 31..=34 {
            grid.set(x, 120, Cell::pack(TNT, 0, 0, 0));
        }
        grid.set(35, 120, Cell::pack(SAND, 0x21, 0, 0));
        grid.set(35, 119, Cell::pack(SAND, 0x22, 0, 0));
        grid.set(33, 118, Cell::pack(WATER, 0, 0, 0));
        grid.set(34, 118, Cell::pack(WATER, 0, 0, 0));
        grid
    }

    #[test]
    fn chunking_matches_a_full_simulation() {
        use crate::grid::{EMPTY, SAND, STONE};
        let grid = crate::grid::Grid::filled(128, 128, |x, y| {
            if y == 120 && (16..48).contains(&x) {
                Cell::pack(STONE, 0, 0, 0)
            } else if (20..36).contains(&x) && (8..24).contains(&y) {
                Cell::pack(SAND, (x * 3) as u8, 0, 0)
            } else {
                Cell::pack(EMPTY, 0, 0, 0)
            }
        });
        let Some(chunked) = run_world(&grid, 40, true) else {
            return;
        };
        let Some(full) = run_world(&grid, 40, false) else {
            return;
        };
        assert_eq!(chunked, full);
        eprintln!("asserted chunked and full simulations matched");
    }

    #[test]
    fn a_settled_scene_sleeps() {
        use crate::grid::{EMPTY, SAND, STONE};
        let grid = crate::grid::Grid::filled(128, 128, |x, y| {
            if y == 40 {
                Cell::pack(STONE, 0, 0, 0)
            } else if y == 39 && (10..20).contains(&x) {
                Cell::pack(SAND, 1, 0, 0)
            } else {
                Cell::pack(EMPTY, 0, 0, 0)
            }
        });
        let Some(gpu) = open_world(&grid, 6, true) else {
            return;
        };
        let (awake, total) = gpu.chunk_stats();
        assert_eq!(awake, 0, "{awake} chunks stayed awake out of {total}");
        eprintln!("asserted a settled scene slept all {total} chunks");
    }

    #[test]
    fn painting_wakes_only_nearby_chunks() {
        use crate::grid::{EMPTY, SAND, STONE};
        let grid = crate::grid::Grid::filled(256, 128, |x, y| {
            if y == 40 {
                Cell::pack(STONE, 0, 0, 0)
            } else if y == 39 && x == 10 {
                Cell::pack(SAND, 1, 0, 0)
            } else {
                Cell::pack(EMPTY, 0, 0, 0)
            }
        });
        let Some(mut gpu) = open_world(&grid, 6, true) else {
            return;
        };
        assert_eq!(gpu.chunk_stats().0, 0);
        gpu.apply_brush(super::Stroke {
            from_x: 10,
            from_y: 10,
            to_x: 10,
            to_y: 10,
            radius: 2,
            material: crate::grid::spawn_word(SAND),
            erase: 0,
            clear: 0,
        });
        assert!(gpu.chunk_awake(0, 0));
        assert!(!gpu.chunk_awake(3, 0), "a far chunk woke up");
        eprintln!("asserted painting woke the nearby chunks only");
    }

    #[test]
    fn time_a_settled_world_against_a_full_dispatch() {
        use crate::grid::EMPTY;
        use std::time::Instant;
        let grid = crate::grid::Grid::filled(2048, 1152, |_, _| Cell::pack(EMPTY, 0, 0, 0));
        let Some(mut asleep) = open_world(&grid, 6, true) else {
            return;
        };
        assert_eq!(asleep.chunk_stats().0, 0);
        let started = Instant::now();
        for pass in 0..20 {
            asleep.step(pass % 2);
        }
        asleep
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        let slept = started.elapsed();
        let Some(mut full) = open_world(&grid, 0, false) else {
            return;
        };
        let started = Instant::now();
        for pass in 0..2 {
            full.step(pass % 2);
        }
        full.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        let busy = started.elapsed();
        eprintln!(
            "timing settled 20 steps {slept:?}, full dispatch 2 steps {busy:?} ({:.1} ms/step vs {:.1} ms/step)",
            slept.as_secs_f64() * 1000.0 / 20.0,
            busy.as_secs_f64() * 1000.0 / 2.0
        );
    }

    fn open_world(grid: &crate::grid::Grid, passes: u32, chunked: bool) -> Option<super::Gpu> {
        let mut gpu = match pollster::block_on(super::Gpu::open(
            None,
            Size {
                width: grid.width() as u32,
                height: grid.height() as u32,
            },
            (grid.width() as u32, grid.height() as u32),
        )) {
            Ok(gpu) => gpu,
            Err(err) if gpu_required(std::env::var("GRAINS_REQUIRE_GPU").ok().as_deref()) => {
                panic!("GRAINS_REQUIRE_GPU=1 and no adapter was found: {err}")
            }
            Err(err) => {
                eprintln!("chunk asserts skipped: {err}");
                return None;
            }
        };
        gpu.set_chunked(chunked);
        gpu.queue.write_buffer(&gpu.cells[0], 0, grid.as_bytes());
        gpu.front = 0;
        for pass in 0..passes {
            gpu.step(pass % 2);
        }
        Some(gpu)
    }

    fn run_world(grid: &crate::grid::Grid, passes: u32, chunked: bool) -> Option<Vec<Cell>> {
        open_world(grid, passes, chunked).map(|gpu| gpu.read_cells(gpu.front))
    }

    fn play(grid: &crate::grid::Grid, passes: u32) -> Option<(Vec<Cell>, Vec<super::Speck>)> {
        let mut gpu = match pollster::block_on(super::Gpu::open(
            None,
            Size {
                width: 512,
                height: 288,
            },
            (512, 288),
        )) {
            Ok(gpu) => gpu,
            Err(err) if gpu_required(std::env::var("GRAINS_REQUIRE_GPU").ok().as_deref()) => {
                panic!("GRAINS_REQUIRE_GPU=1 and no adapter was found: {err}")
            }
            Err(err) => {
                eprintln!("blast asserts skipped: {err}");
                return None;
            }
        };
        gpu.queue.write_buffer(&gpu.cells[0], 0, grid.as_bytes());
        gpu.front = 0;
        for pass in 0..passes {
            gpu.step(pass % 2);
        }
        Some((gpu.read_cells(gpu.front), gpu.read_specks()))
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
    fn stroke_fills_the_cells_between_the_ends() {
        use crate::grid::{EMPTY, SAND};
        let grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        let Some(cells) = colours(super::Gpu::stamp(
            &grid,
            super::Stroke {
                from_x: 2,
                from_y: 10,
                to_x: 20,
                to_y: 10,
                radius: 0,
                material: u32::from(SAND),
                erase: 0,
                clear: 0,
            },
        )) else {
            return;
        };
        for x in 2..=20 {
            assert_eq!(material_at(&cells, x, 10), SAND, "gap at {x}");
            assert_eq!(material_at(&cells, x, 9), EMPTY);
            assert_eq!(material_at(&cells, x, 11), EMPTY);
        }
        let Some(diagonal) = colours(super::Gpu::stamp(
            &grid,
            super::Stroke {
                from_x: 0,
                from_y: 30,
                to_x: 10,
                to_y: 40,
                radius: 0,
                material: u32::from(SAND),
                erase: 0,
                clear: 0,
            },
        )) else {
            return;
        };
        for i in 0..=10 {
            assert_eq!(material_at(&diagonal, i, 30 + i), SAND, "gap at {i}");
        }
        eprintln!("asserted continuous strokes (2,10)-(20,10) and (0,30)-(10,40)");
    }

    #[test]
    fn erase_clears_and_paint_leaves_stone() {
        use crate::grid::{EMPTY, SAND, STONE};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        grid.set(10, 10, Cell::pack(STONE, 0, 0, 0));
        let Some(painted) = colours(super::Gpu::stamp(
            &grid,
            super::Stroke {
                from_x: 2,
                from_y: 10,
                to_x: 20,
                to_y: 10,
                radius: 0,
                material: u32::from(SAND),
                erase: 0,
                clear: 0,
            },
        )) else {
            return;
        };
        assert_eq!(material_at(&painted, 10, 10), STONE);
        assert_eq!(material_at(&painted, 9, 10), SAND);
        assert_eq!(material_at(&painted, 11, 10), SAND);
        let held = crate::grid::Grid::from_fn(|x, y| painted[y * crate::grid::GRID_WIDTH + x]);
        let Some(erased) = colours(super::Gpu::stamp(
            &held,
            super::Stroke {
                from_x: 8,
                from_y: 10,
                to_x: 12,
                to_y: 10,
                radius: 0,
                material: 0,
                erase: 1,
                clear: 0,
            },
        )) else {
            return;
        };
        assert_eq!(material_at(&erased, 10, 10), EMPTY);
        assert_eq!(material_at(&erased, 9, 10), EMPTY);
        assert_eq!(material_at(&erased, 11, 10), EMPTY);
        assert_eq!(material_at(&erased, 2, 10), SAND);
        assert_eq!(material_at(&erased, 20, 10), SAND);
        eprintln!("asserted stone survived paint and erase cleared (8,10)-(12,10)");
    }

    #[test]
    fn water_levels_in_a_container() {
        use crate::grid::{EMPTY, STONE, WATER};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        let x0 = 10usize;
        let x1 = 26usize;
        let floor = 40usize;
        for y in 20..=floor {
            grid.set(x0, y, Cell::pack(STONE, 0, 0, 0));
            grid.set(x1, y, Cell::pack(STONE, 0, 0, 0));
        }
        for x in x0..=x1 {
            grid.set(x, floor, Cell::pack(STONE, 0, 0, 0));
        }
        for x in 11..15 {
            for y in 28..floor {
                grid.set(x, y, Cell::pack(WATER, 0, 0, 0));
            }
        }
        let before = count_material(&grid_cells(&grid), WATER);
        let Some(cells) = colours(super::Gpu::simulate(&grid, 200)) else {
            return;
        };
        assert_eq!(count_material(&cells, WATER), before);
        let mut heights = Vec::new();
        for x in (x0 + 1)..x1 {
            let height = (20..floor)
                .filter(|y| material_at(&cells, x, *y) == WATER)
                .count();
            heights.push(height);
            assert_eq!(material_at(&cells, x, floor), STONE);
        }
        assert_eq!(material_at(&cells, x0, 30), STONE);
        assert_eq!(material_at(&cells, x1, 30), STONE);
        let min = *heights.iter().min().unwrap();
        let max = *heights.iter().max().unwrap();
        assert!(max - min <= 1, "heights {heights:?}");
        assert!(max >= 1, "water vanished");
        eprintln!("asserted water levelled to heights {min}..={max}");
    }

    #[test]
    fn oil_ends_above_water_and_sand_ends_below() {
        use crate::grid::{EMPTY, OIL, SAND, STONE, WATER};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        for y in 10..=24 {
            grid.set(6, y, Cell::pack(STONE, 0, 0, 0));
            grid.set(9, y, Cell::pack(STONE, 0, 0, 0));
        }
        for x in 6..=9 {
            grid.set(x, 24, Cell::pack(STONE, 0, 0, 0));
        }
        for x in 7..=8 {
            for y in 14..=18 {
                grid.set(x, y, Cell::pack(WATER, 0, 0, 0));
            }
            for y in 19..=23 {
                grid.set(x, y, Cell::pack(OIL, 0, 0, 0));
            }
        }
        let Some(cells) = colours(super::Gpu::simulate(&grid, 40)) else {
            return;
        };
        assert_eq!(count_material(&cells, WATER), 10);
        assert_eq!(count_material(&cells, OIL), 10);
        for x in 7..=8 {
            let oil_bottom = (10..24).filter(|y| material_at(&cells, x, *y) == OIL).max();
            let water_top = (10..24)
                .filter(|y| material_at(&cells, x, *y) == WATER)
                .min();
            assert!(oil_bottom.unwrap() < water_top.unwrap(), "column {x}");
        }
        eprintln!("asserted oil above water");

        let mut sunk = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        for y in 10..=24 {
            sunk.set(6, y, Cell::pack(STONE, 0, 0, 0));
            sunk.set(9, y, Cell::pack(STONE, 0, 0, 0));
        }
        for x in 6..=9 {
            sunk.set(x, 24, Cell::pack(STONE, 0, 0, 0));
        }
        for x in 7..=8 {
            for y in 16..=23 {
                sunk.set(x, y, Cell::pack(WATER, 0, 0, 0));
            }
            sunk.set(x, 14, Cell::pack(SAND, 0, 0, 0));
            sunk.set(x, 15, Cell::pack(SAND, 0, 0, 0));
        }
        let Some(cells) = colours(super::Gpu::simulate(&sunk, 40)) else {
            return;
        };
        assert_eq!(count_material(&cells, SAND), 4);
        assert_eq!(count_material(&cells, WATER), 16);
        for x in 7..=8 {
            let sand_top = (10..24)
                .filter(|y| material_at(&cells, x, *y) == SAND)
                .min()
                .unwrap();
            let water_bottom = (10..24)
                .filter(|y| material_at(&cells, x, *y) == WATER)
                .max()
                .unwrap();
            assert!(sand_top > water_bottom, "column {x}");
        }
        eprintln!("asserted sand below water");
    }

    #[test]
    fn a_falling_water_column_stays_narrow() {
        use crate::grid::{EMPTY, OIL, WATER};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        for y in 10..22 {
            grid.set(20, y, Cell::pack(WATER, 0, 0, 0));
            grid.set(21, y, Cell::pack(WATER, 0, 0, 0));
            grid.set(40, y, Cell::pack(OIL, 0, 0, 0));
            grid.set(41, y, Cell::pack(OIL, 0, 0, 0));
        }
        let Some(cells) = colours(super::Gpu::simulate(&grid, 8)) else {
            return;
        };
        assert_narrow(&cells, WATER, 20, 21);
        assert_narrow(&cells, OIL, 40, 41);
        let water_bottom = (0..crate::grid::GRID_HEIGHT)
            .rev()
            .find(|y| material_at(&cells, 20, *y) == WATER || material_at(&cells, 21, *y) == WATER)
            .unwrap();
        let oil_bottom = (0..crate::grid::GRID_HEIGHT)
            .rev()
            .find(|y| material_at(&cells, 40, *y) == OIL || material_at(&cells, 41, *y) == OIL)
            .unwrap();
        assert!(
            water_bottom > 21,
            "water did not fall, bottom {water_bottom}"
        );
        assert!(oil_bottom > 21, "oil did not fall, bottom {oil_bottom}");
        eprintln!("asserted falling water and oil stayed in a column");
    }

    fn assert_narrow(cells: &[Cell], material: u8, x0: usize, x1: usize) {
        let mut seen = 0usize;
        for y in 0..crate::grid::GRID_HEIGHT {
            for x in 0..crate::grid::GRID_WIDTH {
                if material_at(cells, x, y) != material {
                    continue;
                }
                seen += 1;
                assert!(
                    (x0..=x1).contains(&x),
                    "{material} sprayed to x={x} from {x0}..={x1}"
                );
            }
        }
        assert_eq!(seen, 24, "lost {material}");
    }

    #[test]
    fn smoke_rises_slower_than_sand_falls() {
        use crate::grid::{EMPTY, SAND, SMOKE};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        grid.set(8, 30, Cell::pack(SAND, 0, 0, 0));
        grid.set(12, 30, Cell::pack(SMOKE, 0, 40, 0));
        let Some(cells) = colours(super::Gpu::simulate(&grid, 8)) else {
            return;
        };
        let sand_y = (0..crate::grid::GRID_HEIGHT)
            .filter(|y| material_at(&cells, 8, *y) == SAND)
            .max()
            .unwrap();
        let smoke_y = (0..crate::grid::GRID_HEIGHT)
            .filter(|y| (0..crate::grid::GRID_WIDTH).any(|x| material_at(&cells, x, *y) == SMOKE))
            .min()
            .unwrap();
        let sand_drop = sand_y - 30;
        let smoke_rise = 30 - smoke_y;
        assert!(
            sand_drop > smoke_rise,
            "sand {sand_drop} smoke {smoke_rise}"
        );
        assert!(smoke_rise >= 1, "smoke did not rise");
        eprintln!("asserted sand fell {sand_drop} and smoke rose {smoke_rise}");
    }

    #[test]
    fn smoke_rises_and_then_disappears() {
        use crate::grid::{EMPTY, SMOKE};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        grid.set(20, 40, Cell::pack(SMOKE, 0, 8, 0));
        let Some(risen) = colours(super::Gpu::simulate(&grid, 4)) else {
            return;
        };
        assert_eq!(count_material(&risen, SMOKE), 1);
        let highest = (0..crate::grid::GRID_HEIGHT)
            .filter(|y| (0..crate::grid::GRID_WIDTH).any(|x| material_at(&risen, x, *y) == SMOKE))
            .min()
            .unwrap();
        assert!(highest < 40, "smoke stayed at {highest}");
        let Some(gone) = colours(super::Gpu::simulate(&grid, 16)) else {
            return;
        };
        assert_eq!(count_material(&gone, SMOKE), 0);
        eprintln!("asserted smoke rose above 40 and then vanished");
    }

    #[test]
    fn non_gas_mass_is_conserved() {
        use crate::grid::{EMPTY, OIL, SAND, SMOKE, STONE, WATER};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        for i in 0..8 {
            grid.set(30 + i, 20, Cell::pack(SAND, 0, 0, 0));
            grid.set(30 + i, 21, Cell::pack(WATER, 0, 0, 0));
            grid.set(30 + i, 22, Cell::pack(OIL, 0, 0, 0));
            grid.set(30 + i, 23, Cell::pack(STONE, 0, 0, 0));
            grid.set(30 + i, 24, Cell::pack(SMOKE, 0, 20, 0));
        }
        let before = grid_cells(&grid);
        let Some(cells) = colours(super::Gpu::simulate(&grid, 30)) else {
            return;
        };
        assert_eq!(count_material(&cells, SAND), count_material(&before, SAND));
        assert_eq!(
            count_material(&cells, WATER),
            count_material(&before, WATER)
        );
        assert_eq!(count_material(&cells, OIL), count_material(&before, OIL));
        assert_eq!(
            count_material(&cells, STONE),
            count_material(&before, STONE)
        );
        assert!(count_material(&cells, SMOKE) < count_material(&before, SMOKE));
        eprintln!("asserted sand water oil and stone counts held");
    }

    #[test]
    fn fire_burns_through_wood_and_leaves_smoke() {
        use crate::grid::{EMPTY, FIRE, SMOKE, WOOD};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        for x in 10..=13 {
            grid.set(x, 30, Cell::pack(WOOD, 0, 0, 0));
        }
        grid.set(9, 30, Cell::pack(FIRE, 0, 24, 0));
        let Some(cells) = colours(super::Gpu::simulate(&grid, 60)) else {
            return;
        };
        assert_eq!(count_material(&cells, WOOD), 0);
        assert_eq!(count_material(&cells, FIRE), 0);
        assert!(count_material(&cells, SMOKE) > 0);
        eprintln!("asserted wood burned out and left smoke");
    }

    #[test]
    fn water_puts_out_fire_and_makes_steam() {
        use crate::grid::{EMPTY, FIRE, STEAM, STONE, WATER};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        grid.set(15, 21, Cell::pack(STONE, 0, 0, 0));
        grid.set(16, 21, Cell::pack(STONE, 0, 0, 0));
        grid.set(15, 20, Cell::pack(FIRE, 0, 24, 0));
        grid.set(16, 20, Cell::pack(WATER, 0, 0, 0));
        let Some(cells) = colours(super::Gpu::simulate(&grid, 8)) else {
            return;
        };
        assert_eq!(count_material(&cells, FIRE), 0);
        assert_eq!(count_material(&cells, WATER), 0);
        assert!(count_material(&cells, STEAM) > 0);
        let steam_y = (0..crate::grid::GRID_HEIGHT)
            .find(|y| (0..crate::grid::GRID_WIDTH).any(|x| material_at(&cells, x, *y) == STEAM))
            .unwrap();
        assert!(steam_y < 20, "steam stayed at {steam_y}");
        eprintln!("asserted water became steam and the fire went out");
    }

    #[test]
    fn stone_and_sand_do_not_burn() {
        use crate::grid::{EMPTY, FIRE, SAND, STONE};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        grid.set(9, 31, Cell::pack(STONE, 0, 0, 0));
        grid.set(10, 31, Cell::pack(STONE, 0, 0, 0));
        grid.set(11, 31, Cell::pack(STONE, 0, 0, 0));
        grid.set(9, 30, Cell::pack(STONE, 0, 0, 0));
        grid.set(10, 30, Cell::pack(SAND, 0, 0, 0));
        grid.set(11, 30, Cell::pack(FIRE, 0, 24, 0));
        grid.set(12, 30, Cell::pack(STONE, 0, 0, 0));
        let Some(cells) = colours(super::Gpu::simulate(&grid, 40)) else {
            return;
        };
        assert_eq!(material_at(&cells, 10, 30), SAND);
        assert_eq!(material_at(&cells, 9, 30), STONE);
        assert_eq!(material_at(&cells, 10, 31), STONE);
        assert_eq!(material_at(&cells, 12, 30), STONE);
        assert_eq!(count_material(&cells, SAND), 1);
        assert_eq!(count_material(&cells, STONE), 5);
        eprintln!("asserted sand and stone stayed next to fire");
    }

    fn count_material(cells: &[Cell], material: u8) -> usize {
        cells
            .iter()
            .filter(|cell| cell.material() == material)
            .count()
    }

    fn grid_cells(grid: &crate::grid::Grid) -> Vec<Cell> {
        (0..crate::grid::GRID_HEIGHT)
            .flat_map(|y| (0..crate::grid::GRID_WIDTH).map(move |x| grid.cell(x, y)))
            .collect()
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
    fn bloom_brightens_the_pixels_next_to_fire() {
        use crate::grid::{EMPTY, FIRE, SAND, STONE};
        let mut fire = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        fire.set(8, 8, Cell::pack(FIRE, 0, 255, 0));
        let size = Size {
            width: crate::grid::GRID_WIDTH as u32,
            height: crate::grid::GRID_HEIGHT as u32,
        };
        let Some(off) = colours(super::Gpu::render_scene(&fire, size, false)) else {
            return;
        };
        let Some(on) = colours(super::Gpu::render_scene(&fire, size, true)) else {
            return;
        };
        let dark = pixel(&off.rgba, size.width, 9, 8);
        let lit = pixel(&on.rgba, size.width, 9, 8);
        assert!(
            lit[0] >= dark[0] + 12,
            "fire did not glow over the background, off {dark:?} on {lit:?}"
        );
        assert!(
            lit[0] < dark[0] + 36,
            "glow washed out the neighbour, off {dark:?} on {lit:?}"
        );
        assert_eq!(
            pixel(&on.rgba, size.width, 200, 200),
            pixel(&off.rgba, size.width, 200, 200),
            "bloom reached a distant empty cell"
        );
        assert_finite(&on.linear);
        eprintln!("asserted the pixel beside fire rose from {dark:?} to {lit:?}");

        let mut plain = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        plain.set(4, 4, Cell::pack(SAND, 0, 0, 0));
        plain.set(6, 4, Cell::pack(STONE, 0, 0, 0));
        let Some(plain_off) = colours(super::Gpu::render_scene(&plain, size, false)) else {
            return;
        };
        let Some(plain_on) = colours(super::Gpu::render_scene(&plain, size, true)) else {
            return;
        };
        assert_eq!(plain_on.rgba, plain_off.rgba);
        assert_finite(&plain_on.linear);
        eprintln!("asserted a scene without emissive pixels was unchanged");
    }

    fn assert_finite(linear: &[u8]) {
        let (texels, rest) = linear.as_chunks::<2>();
        assert!(rest.is_empty());
        for (index, bytes) in texels.iter().enumerate() {
            let bits = u16::from_le_bytes(*bytes);
            let exponent = (bits >> 10) & 0x1f;
            assert_ne!(exponent, 0x1f, "non-finite texel {index}");
        }
    }

    #[test]
    fn sand_varies_fire_ages_and_the_background_is_graded() {
        use crate::grid::{EMPTY, FIRE, SAND};
        let mut grid = crate::grid::Grid::from_fn(|_, _| Cell::pack(EMPTY, 0, 0, 0));
        grid.set(8, 8, Cell::pack(SAND, 0, 0, 0));
        grid.set(12, 8, Cell::pack(SAND, 255, 0, 0));
        grid.set(30, 40, Cell::pack(SAND, 40, 0, 0));
        grid.set(80, 90, Cell::pack(SAND, 40, 0, 0));
        grid.set(20, 8, Cell::pack(FIRE, 0, 255, 0));
        grid.set(28, 8, Cell::pack(FIRE, 0, 12, 0));
        let size = Size {
            width: crate::grid::GRID_WIDTH as u32,
            height: crate::grid::GRID_HEIGHT as u32,
        };
        let Some(frame) = colours(super::Gpu::render_scene(&grid, size, false)) else {
            return;
        };
        let dull = pixel(&frame.rgba, size.width, 8, 8);
        let bright = pixel(&frame.rgba, size.width, 12, 8);
        assert_ne!(dull, bright, "sand variation did not change the colour");
        assert!(
            bright[0] > dull[0],
            "brighter grain was not lighter {dull:?} {bright:?}"
        );
        assert_eq!(
            pixel(&frame.rgba, size.width, 30, 40),
            pixel(&frame.rgba, size.width, 80, 90),
            "the same variation byte painted two different sands"
        );
        let young = pixel(&frame.rgba, size.width, 20, 8);
        let old = pixel(&frame.rgba, size.width, 28, 8);
        assert!(
            young[1] > old[1] + 15,
            "young fire {young:?} was not yellower than old fire {old:?}"
        );
        assert!(
            young[2] > old[2],
            "young fire {young:?} lost its yellow before the old ember {old:?}"
        );
        assert!(old[0] > old[1], "old fire {old:?} was not red");
        let sky_a = pixel(&frame.rgba, size.width, 8, 40);
        let sky_b = pixel(&frame.rgba, size.width, 400, 200);
        assert_ne!(sky_a, sky_b, "empty background is flat {sky_a:?}");
        let Some(lit) = colours(super::Gpu::render_scene(&grid, size, true)) else {
            return;
        };
        assert_finite(&lit.linear);
        eprintln!(
            "asserted sand {dull:?}/{bright:?}, fire {young:?} to {old:?}, sky {sky_a:?} to {sky_b:?}"
        );
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
