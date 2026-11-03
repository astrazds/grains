mod gpu;
mod grid;

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

use gpu::{Gpu, Paint, Stroke};
use grid::{EMPTY, FIRE, OIL, SAND, SMOKE, STEAM, STONE, WATER, WOOD, material_name, spawn_word};

const RADIUS_MAX: i32 = 48;
const PASSES_MAX: u32 = 16;

struct App {
    window: Option<Arc<Window>>,
    gpu: Rc<std::cell::RefCell<Option<Gpu>>>,
    grid: Grid,
    material: u8,
    radius: i32,
    passes: u32,
    painting: bool,
    erasing: bool,
    paused: bool,
    clear: bool,
    cursor: Option<(f64, f64)>,
    last_cell: Option<(i32, i32)>,
}

pub fn run() {
    eprintln!("brush sand radius 2");
    eprintln!("passes 4");
    let event_loop = EventLoop::new().expect("event loop");
    let app = App {
        window: None,
        gpu: Rc::new(std::cell::RefCell::new(None)),
        grid: Grid::demo(),
        material: SAND,
        radius: 2,
        passes: 4,
        painting: false,
        erasing: false,
        paused: false,
        clear: false,
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
            let mut gpu = Gpu::for_window(Arc::clone(&painted))
                .await
                .unwrap_or_else(|err| panic!("{err}"));
            let reported = painted.inner_size();
            gpu.resize(frame_size(&painted, reported));
            *slot.borrow_mut() = Some(gpu);
            painted.request_redraw();
        });
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.clone() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
                    gpu.resize(frame_size(&window, size));
                }
            }
            WindowEvent::RedrawRequested => {
                let stroke = self.stroke_for(&window);
                let clear = self.clear;
                self.clear = false;
                if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
                    gpu.draw(
                        &window,
                        &self.grid,
                        Paint {
                            paused: self.paused,
                            clear,
                            passes: self.passes,
                            stroke,
                        },
                    );
                }
                window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some((position.x, position.y));
            }
            WindowEvent::MouseInput { state, button, .. } => {
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
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(offset) => offset.y as f32 / 40.0,
                };
                if steps > 0.0 {
                    self.radius = (self.radius + 1).min(RADIUS_MAX);
                } else if steps < 0.0 {
                    self.radius = (self.radius - 1).max(0);
                }
                self.log_brush();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed || event.repeat {
                    return;
                }
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Digit1) => self.set_material(EMPTY),
                    PhysicalKey::Code(KeyCode::Digit2) => self.set_material(STONE),
                    PhysicalKey::Code(KeyCode::Digit3) => self.set_material(SAND),
                    PhysicalKey::Code(KeyCode::Digit4) => self.set_material(WATER),
                    PhysicalKey::Code(KeyCode::Digit5) => self.set_material(OIL),
                    PhysicalKey::Code(KeyCode::Digit6) => self.set_material(SMOKE),
                    PhysicalKey::Code(KeyCode::Digit7) => self.set_material(WOOD),
                    PhysicalKey::Code(KeyCode::Digit8) => self.set_material(FIRE),
                    PhysicalKey::Code(KeyCode::Digit9) => self.set_material(STEAM),
                    PhysicalKey::Code(KeyCode::Minus)
                    | PhysicalKey::Code(KeyCode::NumpadSubtract) => {
                        self.passes = self.passes.saturating_sub(1).max(1);
                        eprintln!("passes {}", self.passes);
                    }
                    PhysicalKey::Code(KeyCode::Equal) | PhysicalKey::Code(KeyCode::NumpadAdd) => {
                        self.passes = (self.passes + 1).min(PASSES_MAX);
                        eprintln!("passes {}", self.passes);
                    }
                    PhysicalKey::Code(KeyCode::KeyC) => self.clear = true,
                    PhysicalKey::Code(KeyCode::Space) => {
                        self.paused = !self.paused;
                        if self.paused {
                            eprintln!("paused");
                        } else {
                            eprintln!("running");
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

impl App {
    fn set_material(&mut self, material: u8) {
        self.material = material;
        self.log_brush();
    }

    fn log_brush(&self) {
        eprintln!(
            "brush {} radius {}",
            material_name(self.material),
            self.radius
        );
    }

    fn stroke_for(&mut self, window: &Window) -> Option<Stroke> {
        if !self.painting && !self.erasing {
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
            radius: self.radius as u32,
            material: spawn_word(self.material),
            erase: u32::from(self.erasing),
            clear: 0,
        })
    }
}
