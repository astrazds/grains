mod gpu;
mod grid;

#[cfg(target_arch = "wasm32")]
mod web;

pub use gpu::NoAdapter;
pub use grid::{CELL_COUNT, Cell, Fit, GRID_HEIGHT, GRID_WIDTH, Grid, LETTERBOX, Size, letterbox};

pub fn render_rgba(grid: &Grid, size: Size) -> Result<Vec<u8>, NoAdapter> {
    Gpu::render_rgba(grid, size)
}

use std::rc::Rc;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use gpu::Gpu;

struct App {
    window: Option<Arc<Window>>,
    gpu: Rc<std::cell::RefCell<Option<Gpu>>>,
    grid: Grid,
}

pub fn run() {
    let event_loop = EventLoop::new().expect("event loop");
    let app = App {
        window: None,
        gpu: Rc::new(std::cell::RefCell::new(None)),
        grid: Grid::random(),
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
                if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
                    gpu.draw(&window, &self.grid);
                }
                window.request_redraw();
            }
            _ => {}
        }
    }
}
