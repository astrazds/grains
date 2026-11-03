mod gpu;
mod grid;
mod ui;

#[cfg(target_arch = "wasm32")]
mod web;

pub use gpu::NoAdapter;
pub use grid::{
    CELL_COUNT, Cell, Fit, GRID_HEIGHT, GRID_WIDTH, Grid, LETTERBOX, Size, cell_at, letterbox,
    physical_pixel,
};

pub fn render_rgba(grid: &Grid, size: Size) -> Result<Vec<u8>, NoAdapter> {
    Gpu::render_rgba(grid, size)
}

use std::rc::Rc;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

use gpu::{EguiFrame, Gpu, Paint, Stroke};
use grid::material_name;
use ui::{Controls, PASSES_MAX, RADIUS_MAX};

struct App {
    window: Option<Arc<Window>>,
    gpu: Rc<std::cell::RefCell<Option<Gpu>>>,
    grid: Grid,
    controls: Controls,
    over_panel: bool,
    fps: f32,
    last_frame: web_time::Instant,
    egui: Option<egui_winit::State>,
    painting: bool,
    erasing: bool,
    cursor: Option<(f64, f64)>,
    last_cell: Option<(i32, i32)>,
}

pub fn run() {
    eprintln!("brush sand radius 2");
    eprintln!("passes 4");
    eprintln!("bloom on");
    let event_loop = EventLoop::new().expect("event loop");
    let app = App {
        window: None,
        gpu: Rc::new(std::cell::RefCell::new(None)),
        grid: Grid::demo(),
        controls: Controls::new(),
        over_panel: false,
        fps: 0.0,
        last_frame: web_time::Instant::now(),
        egui: None,
        painting: false,
        erasing: false,
        cursor: None,
        last_cell: None,
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut app = app;
        event_loop.run_app(&mut app).expect("event loop");
    }
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(app);
    }
}

fn window_attributes() -> WindowAttributes {
    #[cfg(target_arch = "wasm32")]
    {
        web::attributes()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        WindowAttributes::default()
            .with_title("grains")
            .with_inner_size(PhysicalSize::<u32>::new(1024, 600))
    }
}

fn digit(key: KeyCode) -> Option<char> {
    match key {
        KeyCode::Digit1 => Some('1'),
        KeyCode::Digit2 => Some('2'),
        KeyCode::Digit3 => Some('3'),
        KeyCode::Digit4 => Some('4'),
        KeyCode::Digit5 => Some('5'),
        KeyCode::Digit6 => Some('6'),
        KeyCode::Digit7 => Some('7'),
        KeyCode::Digit8 => Some('8'),
        KeyCode::Digit9 => Some('9'),
        _ => None,
    }
}

fn frame_size(window: &Window, reported: PhysicalSize<u32>) -> Size {
    #[cfg(target_arch = "wasm32")]
    {
        let measured = web::physical_size(window);
        if measured.width == 0 || measured.height == 0 {
            Size {
                width: reported.width,
                height: reported.height,
            }
        } else {
            measured
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = window;
        Size {
            width: reported.width,
            height: reported.height,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(window_attributes())
                .expect("window"),
        );
        let slot = Rc::clone(&self.gpu);
        let painted = Arc::clone(&window);
        gpu::wait(async move {
            match Gpu::for_window(Arc::clone(&painted)).await {
                Ok(mut gpu) => {
                    let reported = painted.inner_size();
                    gpu.resize(frame_size(&painted, reported));
                    *slot.borrow_mut() = Some(gpu);
                    painted.request_redraw();
                }
                Err(err) => {
                    #[cfg(target_arch = "wasm32")]
                    web::report_unavailable(&err.to_string());
                    #[cfg(not(target_arch = "wasm32"))]
                    panic!("{err}");
                }
            }
        });
        self.window = Some(window.clone());
        self.egui = Some(egui_winit::State::new(
            egui::Context::default(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            window.theme(),
            None,
        ));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let consumed = self
            .egui
            .as_mut()
            .is_some_and(|ui| ui.on_window_event(&window, &event).consumed);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
                    gpu.resize(frame_size(&window, size));
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
                    gpu.resize(frame_size(&window, window.inner_size()));
                }
            }
            WindowEvent::RedrawRequested => self.redraw(&window),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some((position.x, position.y));
            }
            WindowEvent::MouseInput { state, button, .. } if !consumed => {
                let down = state == ElementState::Pressed;
                match button {
                    MouseButton::Left => self.painting = down,
                    MouseButton::Right => self.erasing = down,
                    _ => {}
                }
                if !self.painting && !self.erasing {
                    self.last_cell = None;
                }
            }
            WindowEvent::MouseWheel { delta, .. } if !self.over_panel => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(offset) => offset.y as f32 / 40.0,
                };
                if steps > 0.0 {
                    self.controls.radius = (self.controls.radius + 1).min(RADIUS_MAX);
                } else if steps < 0.0 {
                    self.controls.radius = (self.controls.radius - 1).max(0);
                }
                self.log_brush();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed || event.repeat {
                    return;
                }
                if let PhysicalKey::Code(key) = event.physical_key {
                    self.key(key);
                }
            }
            _ => {}
        }
    }
}

impl App {
    fn key(&mut self, key: KeyCode) {
        if let Some(material) = digit(key).and_then(ui::material_for_key) {
            self.set_material(material);
            return;
        }
        match key {
            KeyCode::Minus | KeyCode::NumpadSubtract => {
                self.controls.passes = self.controls.passes.saturating_sub(1).max(1);
                eprintln!("passes {}", self.controls.passes);
            }
            KeyCode::Equal | KeyCode::NumpadAdd => {
                self.controls.passes = (self.controls.passes + 1).min(PASSES_MAX);
                eprintln!("passes {}", self.controls.passes);
            }
            KeyCode::KeyC => self.controls.clear = true,
            KeyCode::KeyB => {
                self.controls.bloom = !self.controls.bloom;
                if self.controls.bloom {
                    eprintln!("bloom on");
                } else {
                    eprintln!("bloom off");
                }
            }
            KeyCode::Space => {
                self.controls.paused = !self.controls.paused;
                if self.controls.paused {
                    eprintln!("paused");
                } else {
                    eprintln!("running");
                }
            }
            _ => {}
        }
    }

    fn set_material(&mut self, material: u8) {
        self.controls.material = material;
        self.log_brush();
    }

    fn log_brush(&self) {
        eprintln!(
            "brush {} radius {}",
            material_name(self.controls.material),
            self.controls.radius
        );
    }

    fn redraw(&mut self, window: &Window) {
        let now = web_time::Instant::now();
        let dt = now.saturating_duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;
        if dt > 0.0 && dt < 0.5 {
            let sample = 1.0 / dt;
            self.fps = if self.fps == 0.0 {
                sample
            } else {
                self.fps * 0.9 + sample * 0.1
            };
        }
        let before = (
            self.controls.material,
            self.controls.radius,
            self.controls.passes,
            self.controls.paused,
            self.controls.bloom,
        );
        let ui = self.run_panel(window);
        if self.controls.material != before.0 || self.controls.radius != before.1 {
            self.log_brush();
        }
        if self.controls.passes != before.2 {
            eprintln!("passes {}", self.controls.passes);
        }
        if self.controls.paused != before.3 {
            if self.controls.paused {
                eprintln!("paused");
            } else {
                eprintln!("running");
            }
        }
        if self.controls.bloom != before.4 {
            if self.controls.bloom {
                eprintln!("bloom on");
            } else {
                eprintln!("bloom off");
            }
        }
        let stroke = self.stroke_for(window);
        let clear = self.controls.clear;
        self.controls.clear = false;
        if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
            gpu.draw(
                window,
                &self.grid,
                Paint {
                    paused: self.controls.paused,
                    clear,
                    bloom: self.controls.bloom,
                    passes: self.controls.passes,
                    stroke,
                },
                ui,
            );
        }
        window.request_redraw();
    }

    fn run_panel(&mut self, window: &Window) -> Option<EguiFrame> {
        let (ctx, input) = {
            let ui = self.egui.as_mut()?;
            (ui.egui_ctx().clone(), ui.take_egui_input(window))
        };
        let fps = self.fps;
        let full = ctx.run_pass(input, |ctx| {
            ui::show(ctx, &mut self.controls, fps);
            self.over_panel = ctx.is_pointer_over_egui();
        });
        let ui = self.egui.as_mut()?;
        ui.handle_platform_output(window, full.platform_output);
        let primitives = ctx.tessellate(full.shapes, full.pixels_per_point);
        Some(EguiFrame {
            primitives,
            textures: full.textures_delta,
            pixels_per_point: full.pixels_per_point,
        })
    }

    fn stroke_for(&mut self, window: &Window) -> Option<Stroke> {
        if self.over_panel || (!self.painting && !self.erasing) {
            self.last_cell = None;
            return None;
        }
        let size = frame_size(window, window.inner_size());
        let fit = letterbox(size)?;
        let (cursor_x, cursor_y) = self.cursor?;
        let Some((x, y)) = cell_at(cursor_x.round() as i32, cursor_y.round() as i32, fit) else {
            self.last_cell = None;
            return None;
        };
        let cell = (x as i32, y as i32);
        let from = self.last_cell.unwrap_or(cell);
        self.last_cell = Some(cell);
        Some(Stroke {
            from_x: from.0,
            from_y: from.1,
            to_x: cell.0,
            to_y: cell.1,
            radius: self.controls.radius as u32,
            material: grid::spawn_word(self.controls.material),
            erase: u32::from(self.erasing),
            clear: 0,
        })
    }
}
