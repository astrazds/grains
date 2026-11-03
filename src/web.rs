use wasm_bindgen::prelude::*;
use winit::platform::web::WindowAttributesExtWebSys;
use winit::window::WindowAttributes;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    crate::run();
}

pub fn attributes() -> WindowAttributes {
    let browser = web_sys::window().expect("window");
    let document = browser.document().expect("document");
    let canvas = document
        .get_element_by_id("grains")
        .expect("canvas")
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .expect("canvas");
    WindowAttributes::default()
        .with_title("grains")
        .with_canvas(Some(canvas))
}

pub fn physical_size(window: &winit::window::Window) -> crate::Size {
    use winit::platform::web::WindowExtWebSys;
    let canvas = window.canvas().expect("canvas");
    let ratio = web_sys::window().expect("window").device_pixel_ratio();
    crate::Size {
        width: device_pixels(canvas.client_width(), ratio),
        height: device_pixels(canvas.client_height(), ratio),
    }
}

fn device_pixels(css_px: i32, ratio: f64) -> u32 {
    (f64::from(css_px.max(0)) * ratio).round() as u32
}
