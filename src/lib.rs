mod gpu;
mod grid;
mod ui;
mod world;

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

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, Modifiers, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

use gpu::{EguiFrame, Gpu, Paint, Stroke};

type Inbox = Rc<RefCell<Option<Result<Vec<u8>, String>>>>;
use grid::{Camera, material_name};
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
    notice: Rc<RefCell<String>>,
    inbox: Inbox,
    link_checked: bool,
    camera: Camera,
    pan_button: Option<MouseButton>,
    pending_strokes: Vec<Stroke>,
    modifiers: Modifiers,
}

pub fn run() {
    eprintln!("brush sand radius 2");
    eprintln!("passes 4");
    eprintln!("bloom on");
    let event_loop = EventLoop::new().expect("event loop");
    let app = App::new();
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
        KeyCode::Digit0 => Some('0'),
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
        let size = frame_size(&window, window.inner_size());
        self.camera = Camera::centered(
            self.grid.width() as u32,
            self.grid.height() as u32,
            size.width,
            size.height,
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
                self.resize(frame_size(&window, size));
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                self.resize(frame_size(&window, window.inner_size()));
            }
            WindowEvent::RedrawRequested => self.redraw(&window),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers,
            WindowEvent::CursorMoved { position, .. } => {
                if self.pan_button.is_some()
                    && let Some((last_x, last_y)) = self.cursor
                {
                    let size = frame_size(&window, window.inner_size());
                    self.camera.pan(
                        (position.x - last_x) as f32,
                        (position.y - last_y) as f32,
                        self.grid.width() as u32,
                        self.grid.height() as u32,
                        size.width,
                        size.height,
                    );
                }
                self.cursor = Some((position.x, position.y));
                if let Some(state) = &self.egui {
                    let ctx = state.egui_ctx();
                    let point =
                        egui::pos2(position.x as f32, position.y as f32) / ctx.pixels_per_point();
                    self.over_panel = ctx
                        .layer_id_at(point)
                        .is_some_and(|layer| layer.order != egui::Order::Background);
                    if self.over_panel {
                        self.last_cell = None;
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.mouse_button(button, state, self.modifiers.state().shift_key(), consumed)
            }
            WindowEvent::Focused(false) => {
                self.painting = false;
                self.erasing = false;
                self.pan_button = None;
                self.cursor = None;
                self.last_cell = None;
                self.modifiers = Modifiers::default();
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                self.last_cell = None;
            }
            WindowEvent::MouseWheel { delta, .. } if !consumed && !self.over_panel => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(offset) => offset.y as f32 / 40.0,
                };
                if steps == 0.0 {
                    return;
                }
                let flags = self.modifiers.state();
                if flags.control_key() || flags.alt_key() || flags.super_key() {
                    let factor = if steps > 0.0 { 1.1 } else { 0.9 };
                    let (cursor_x, cursor_y) = self.cursor.unwrap_or((0.0, 0.0));
                    self.camera
                        .zoom_at(cursor_x as f32, cursor_y as f32, factor);
                    let size = frame_size(&window, window.inner_size());
                    self.camera.constrain(
                        self.grid.width() as u32,
                        self.grid.height() as u32,
                        size.width,
                        size.height,
                    );
                } else if steps > 0.0 {
                    self.controls.radius = (self.controls.radius + 1).min(RADIUS_MAX);
                    self.log_brush();
                } else if steps < 0.0 {
                    self.controls.radius = (self.controls.radius - 1).max(0);
                    self.log_brush();
                }
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if !consumed => {
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
    fn new() -> Self {
        Self {
            window: None,
            gpu: Rc::new(std::cell::RefCell::new(None)),
            grid: Grid::demo_world(),
            controls: Controls::new(),
            over_panel: false,
            fps: 0.0,
            last_frame: web_time::Instant::now(),
            egui: None,
            painting: false,
            erasing: false,
            cursor: None,
            last_cell: None,
            notice: Rc::new(RefCell::new(String::new())),
            inbox: Rc::new(RefCell::new(None)),
            link_checked: false,
            camera: {
                let (width, height) = grid::world_size();
                Camera::centered(width as u32, height as u32, 1024, 600)
            },
            pan_button: None,
            pending_strokes: Vec::new(),
            modifiers: Modifiers::default(),
        }
    }

    fn mouse_button(
        &mut self,
        button: MouseButton,
        state: ElementState,
        shift: bool,
        consumed: bool,
    ) {
        let down = state == ElementState::Pressed;
        if down && (consumed || self.over_panel) {
            return;
        }
        if !down
            && !consumed
            && !self.over_panel
            && ((button == MouseButton::Left && self.painting)
                || (button == MouseButton::Right && self.erasing))
            && let Some(stroke) = self.stroke_for()
            && (stroke.from_x, stroke.from_y) != (stroke.to_x, stroke.to_y)
        {
            self.pending_strokes.push(stroke);
        }
        match button {
            MouseButton::Left => self.painting = down,
            MouseButton::Right if down && shift => self.erasing = true,
            MouseButton::Middle | MouseButton::Right if down => self.pan_button = Some(button),
            MouseButton::Middle | MouseButton::Right => {
                if button == MouseButton::Right {
                    self.erasing = false;
                }
                if self.pan_button == Some(button) {
                    self.pan_button = None;
                }
            }
            _ => return,
        }
        if down && (button == MouseButton::Left || (button == MouseButton::Right && self.erasing)) {
            if let Some(stroke) = self.stroke_for() {
                self.pending_strokes.push(stroke);
            }
        } else if !self.painting && !self.erasing {
            self.last_cell = None;
        }
    }

    fn resize(&mut self, size: Size) {
        if size.width > 0 && size.height > 0 {
            self.camera.constrain(
                self.grid.width() as u32,
                self.grid.height() as u32,
                size.width,
                size.height,
            );
        }
        if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
            gpu.resize(size);
        }
    }

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
            KeyCode::KeyC => {
                self.pending_strokes.clear();
                self.last_cell = None;
                self.controls.clear = true;
            }
            KeyCode::KeyS => self.controls.save = true,
            KeyCode::KeyL => self.controls.load = true,
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
        // Only run egui when its texture updates have a frame to render into.
        let frame = {
            let mut slot = self.gpu.borrow_mut();
            let Some(gpu) = slot.as_mut() else { return };
            let Some(frame) = gpu.acquire_frame() else {
                window.request_redraw();
                return;
            };
            frame
        };
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
        self.take_inbox();
        self.open_link();
        let keyboard_clear = std::mem::take(&mut self.controls.clear);
        let ui = self.run_panel(window);
        if self.controls.clear {
            self.pending_strokes.clear();
            self.last_cell = None;
        }
        if self.controls.save {
            self.controls.save = false;
            self.save_world();
        }
        if self.controls.load {
            self.controls.load = false;
            self.load_world();
        }
        if self.controls.share {
            self.controls.share = false;
            self.share_world();
        }
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
        let mut strokes = std::mem::take(&mut self.pending_strokes);
        strokes.extend(self.stroke_for());
        let clear = std::mem::take(&mut self.controls.clear) || keyboard_clear;
        if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
            gpu.set_camera(self.camera);
            gpu.draw(
                frame,
                window,
                &self.grid,
                Paint {
                    paused: self.controls.paused,
                    clear,
                    bloom: self.controls.bloom,
                    passes: self.controls.passes,
                    strokes,
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
            let notice = self.notice.borrow().clone();
            let (awake, chunks) = self
                .gpu
                .borrow()
                .as_ref()
                .map(|gpu| gpu.chunk_stats())
                .unwrap_or((0, 0));
            ui::show(ctx, &mut self.controls, fps, &notice, awake, chunks);
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

    fn take_inbox(&mut self) {
        let Some(result) = self.inbox.borrow_mut().take() else {
            return;
        };
        match result {
            Ok(bytes) => self.apply_file(&bytes),
            Err(err) => self.set_notice(err),
        }
    }

    fn open_link(&mut self) {
        if self.link_checked {
            return;
        }
        self.link_checked = true;
        #[cfg(target_arch = "wasm32")]
        {
            let hash = web::hash();
            if hash.trim_start_matches('#').is_empty() {
                return;
            }
            match world::bytes_from_hash(&hash) {
                Ok(bytes) => self.apply_file(&bytes),
                Err(err) => self.set_notice(err),
            }
        }
    }

    fn save_world(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let words = match self.snapshot() {
                Ok(words) => words,
                Err(err) => {
                    self.set_notice(err);
                    return;
                }
            };
            self.set_notice(write_file(&words));
        }
        #[cfg(target_arch = "wasm32")]
        self.with_grid(|words| {
            match words.and_then(|words| {
                let (width, height) = grid::world_size();
                world::encode(&words, width as u32, height as u32)
            }) {
                Ok(bytes) => match web::download(&bytes) {
                    Ok(()) => format!("saved {}", world::FILE_NAME),
                    Err(err) => err,
                },
                Err(err) => err,
            }
        });
    }

    fn load_world(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        match std::fs::read(world::FILE_NAME) {
            Ok(bytes) => self.apply_file(&bytes),
            Err(err) => self.set_notice(format!("could not load {}: {err}", world::FILE_NAME)),
        }
        #[cfg(target_arch = "wasm32")]
        web::pick_file(Rc::clone(&self.inbox));
    }

    fn share_world(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let words = match self.snapshot() {
                Ok(words) => words,
                Err(err) => {
                    self.set_notice(err);
                    return;
                }
            };
            self.set_notice(native_link(&words));
        }
        #[cfg(target_arch = "wasm32")]
        self.with_grid(|words| {
            match words.and_then(|words| {
                let (width, height) = grid::world_size();
                world::encode(&words, width as u32, height as u32)
            }) {
                Ok(bytes) => match world::hash_fragment(&bytes) {
                    Ok(fragment) => match web::set_hash(&fragment) {
                        Ok(()) => "updated the link".to_string(),
                        Err(err) => err,
                    },
                    Err(err) => err,
                },
                Err(err) => err,
            }
        });
    }

    fn apply_file(&mut self, bytes: &[u8]) {
        let saved = match world::decode(bytes) {
            Ok(saved) => saved,
            Err(err) => {
                self.set_notice(err);
                return;
            }
        };
        if let Err(err) = self.grid.place(saved.width, saved.height, &saved.words) {
            self.set_notice(err);
            return;
        }
        if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
            gpu.store(&self.grid);
            gpu.wake_all();
        }
        self.set_notice("loaded the world");
    }

    fn set_notice(&self, text: impl Into<String>) {
        *self.notice.borrow_mut() = text.into();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn snapshot(&self) -> Result<Vec<u32>, String> {
        let gpu = self.gpu.borrow();
        let Some(gpu) = gpu.as_ref() else {
            return Err("the grid is not ready to save".to_string());
        };
        Ok(gpu.snapshot())
    }

    #[cfg(target_arch = "wasm32")]
    fn with_grid(&self, done: impl FnOnce(Result<Vec<u32>, String>) -> String + 'static) {
        let notice = Rc::clone(&self.notice);
        let gpu = self.gpu.borrow();
        let Some(gpu) = gpu.as_ref() else {
            drop(gpu);
            self.set_notice("the grid is not ready to save");
            return;
        };
        gpu.snapshot_then(move |words| {
            *notice.borrow_mut() = done(words);
        });
    }

    fn stroke_for(&mut self) -> Option<Stroke> {
        if self.over_panel || (!self.painting && !self.erasing) {
            self.last_cell = None;
            return None;
        }
        let (cursor_x, cursor_y) = self.cursor?;
        let Some((x, y)) = grid::cell_at_camera(
            cursor_x as f32,
            cursor_y as f32,
            self.camera,
            self.grid.width() as u32,
            self.grid.height() as u32,
        ) else {
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

#[cfg(not(target_arch = "wasm32"))]
fn write_file(words: &[u32]) -> String {
    match world::encode(
        words,
        crate::grid::world_size().0 as u32,
        crate::grid::world_size().1 as u32,
    ) {
        Ok(bytes) => match std::fs::write(world::FILE_NAME, bytes) {
            Ok(()) => format!("saved {}", world::FILE_NAME),
            Err(err) => format!("could not save {}: {err}", world::FILE_NAME),
        },
        Err(err) => err,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_link(words: &[u32]) -> String {
    match world::encode(
        words,
        crate::grid::world_size().0 as u32,
        crate::grid::world_size().1 as u32,
    )
    .and_then(|bytes| world::hash_fragment(&bytes))
    {
        Ok(fragment) => {
            eprintln!("link #{fragment}");
            "printed a link for this world".to_string()
        }
        Err(err) => err,
    }
}

#[cfg(test)]
mod input_tests {
    use super::*;
    use ElementState::{Pressed, Released};

    #[test]
    fn clear_between_redraws_preserves_only_later_strokes() {
        let mut app = App::new();
        app.cursor = Some((100.0, 40.0));
        app.mouse_button(MouseButton::Left, Pressed, false, false);
        app.mouse_button(MouseButton::Left, Released, false, false);
        app.key(KeyCode::KeyC);
        assert!(app.controls.clear);
        assert!(app.pending_strokes.is_empty());
        app.cursor = Some((140.0, 40.0));
        app.mouse_button(MouseButton::Left, Pressed, false, false);
        assert_eq!(app.pending_strokes.len(), 1);
    }

    #[test]
    fn drag_between_redraws_reaches_both_endpoints() {
        let mut app = App::new();
        app.cursor = Some((100.0, 40.0));
        app.mouse_button(MouseButton::Left, Pressed, false, false);
        app.cursor = Some((140.0, 40.0));
        app.mouse_button(MouseButton::Left, Released, false, false);
        let stroke = app.pending_strokes.last().expect("queued stroke");
        assert_eq!(stroke.to_x - stroke.from_x, 20);
        assert_eq!(stroke.to_y, stroke.from_y);
    }

    #[test]
    fn releasing_shift_before_right_button_ends_erase() {
        let mut app = App::new();
        app.mouse_button(MouseButton::Right, Pressed, true, false);
        assert!(app.erasing);
        app.mouse_button(MouseButton::Right, Released, false, false);
        assert!(!app.erasing);
    }

    #[test]
    fn releasing_over_panel_ends_world_stroke() {
        let mut app = App::new();
        app.mouse_button(MouseButton::Left, Pressed, false, false);
        assert!(app.painting);
        app.mouse_button(MouseButton::Left, Released, false, true);
        assert!(!app.painting);
    }
}
