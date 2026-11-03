use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
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

pub fn report_unavailable(detail: &str) {
    let Some(browser) = web_sys::window() else {
        return;
    };
    let Some(document) = browser.document() else {
        return;
    };
    if let Some(canvas) = document.get_element_by_id("grains") {
        let _ = canvas.set_attribute("hidden", "hidden");
    }
    let Ok(node) = document.create_element("p") else {
        return;
    };
    node.set_id("grains-webgpu");
    node.set_text_content(Some(
        "grains needs WebGPU, and this browser does not have it. Try a current Chrome, Edge, or Firefox.",
    ));
    let _ = node.set_attribute("title", detail);
    if let Some(body) = document.body() {
        let _ = body.append_child(&node);
    }
}

pub fn probe() -> bool {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .is_some_and(|query| query.split(['?', '&']).any(|part| part == "probe=1"))
}

pub fn hash() -> String {
    let Some(window) = web_sys::window() else {
        return String::new();
    };
    window.location().hash().unwrap_or_default()
}

pub fn set_hash(fragment: &str) -> Result<(), String> {
    let window = web_sys::window().ok_or_else(|| "the page has no window".to_string())?;
    window
        .location()
        .set_hash(fragment)
        .map_err(|_| "could not update the link".to_string())
}

pub fn download(bytes: &[u8]) -> Result<(), String> {
    let window = web_sys::window().ok_or_else(|| "the page has no window".to_string())?;
    let document = window
        .document()
        .ok_or_else(|| "the page has no document".to_string())?;
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array);
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts)
        .map_err(|_| "could not save the world".to_string())?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|_| "could not save the world".to_string())?;
    let anchor = document
        .create_element("a")
        .map_err(|_| "could not save the world".to_string())?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "could not save the world".to_string())?;
    anchor.set_href(&url);
    anchor.set_download(crate::world::FILE_NAME);
    anchor.click();
    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(())
}

pub fn pick_file(slot: crate::Inbox) {
    let Some(window) = web_sys::window() else {
        *slot.borrow_mut() = Some(Err("the page has no window".to_string()));
        return;
    };
    let Some(document) = window.document() else {
        *slot.borrow_mut() = Some(Err("the page has no document".to_string()));
        return;
    };
    let Ok(element) = document.create_element("input") else {
        *slot.borrow_mut() = Some(Err("could not open a file".to_string()));
        return;
    };
    let Ok(input) = element.dyn_into::<web_sys::HtmlInputElement>() else {
        *slot.borrow_mut() = Some(Err("could not open a file".to_string()));
        return;
    };
    input.set_type("file");
    let _ = input.set_attribute("accept", ".grains");
    let chosen = input.clone();
    let change = Closure::wrap(Box::new(move |_event: web_sys::Event| {
        let Some(files) = chosen.files() else {
            return;
        };
        let Some(file) = files.get(0) else {
            return;
        };
        let Ok(reader) = web_sys::FileReader::new() else {
            *slot.borrow_mut() = Some(Err("could not read the file".to_string()));
            return;
        };
        let reading = reader.clone();
        let loaded = Rc::clone(&slot);
        let done = Closure::wrap(Box::new(move |_event: web_sys::Event| {
            *loaded.borrow_mut() = Some(file_bytes(&reading));
        }) as Box<dyn FnMut(web_sys::Event)>);
        reader.set_onload(Some(done.as_ref().unchecked_ref()));
        ONLOAD.with(|onload| *onload.borrow_mut() = Some(done));
        if reader.read_as_array_buffer(&file).is_err() {
            *slot.borrow_mut() = Some(Err("could not read the file".to_string()));
        }
    }) as Box<dyn FnMut(web_sys::Event)>);
    input.set_onchange(Some(change.as_ref().unchecked_ref()));
    ONCHANGE.with(|onchange| *onchange.borrow_mut() = Some(change));
    if let Some(body) = document.body() {
        let _ = body.append_child(&input);
    }
    input.click();
}

fn file_bytes(reader: &web_sys::FileReader) -> Result<Vec<u8>, String> {
    let value = reader
        .result()
        .map_err(|_| "could not read the file".to_string())?;
    let buffer = value
        .dyn_into::<js_sys::ArrayBuffer>()
        .map_err(|_| "could not read the file".to_string())?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

type Listener = RefCell<Option<Closure<dyn FnMut(web_sys::Event)>>>;

thread_local! {
    static ONCHANGE: Listener = const { RefCell::new(None) };
    static ONLOAD: Listener = const { RefCell::new(None) };
}

fn device_pixels(css_px: i32, ratio: f64) -> u32 {
    crate::physical_pixel(f64::from(css_px.max(0)), ratio).max(0) as u32
}
