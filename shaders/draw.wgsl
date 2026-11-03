const MATERIAL_SHIFT: u32 = 0u;
const VARIATION_SHIFT: u32 = 8u;
const LIFE_SHIFT: u32 = 16u;
const FLAGS_SHIFT: u32 = 24u;

struct Frame {
    origin_x: i32,
    origin_y: i32,
    scale: u32,
    grid_width: u32,
    grid_height: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

struct MaterialInfo {
    density: u32,
    kind: u32,
    life: u32,
    becomes: u32,
    colour: vec4<f32>,
    emissive: f32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

struct Shade {
    colour: vec4<f32>,
    glow: vec4<f32>,
    drawn: bool,
}

struct HdrOut {
    @location(0) colour: vec4<f32>,
    @location(1) glow: vec4<f32>,
}

@group(0) @binding(0) var<storage, read> cells: array<u32>;
@group(0) @binding(1) var<uniform> frame: Frame;
@group(0) @binding(2) var<storage, read> materials: array<MaterialInfo>;

struct VsOut {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vs(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let xy = positions[vertex_index];
    return VsOut(vec4<f32>(xy, 0.0, 1.0));
}

fn shade(position: vec4<f32>) -> Shade {
    let pixel = vec2<i32>(floor(position.xy));
    let local = pixel - vec2<i32>(frame.origin_x, frame.origin_y);
    if (local.x < 0 || local.y < 0) {
        return Shade(vec4<f32>(0.0), vec4<f32>(0.0), false);
    }
    let scale = i32(frame.scale);
    let cell = vec2<u32>(u32(local.x / scale), u32(local.y / scale));
    if (cell.x >= frame.grid_width || cell.y >= frame.grid_height) {
        return Shade(vec4<f32>(0.0), vec4<f32>(0.0), false);
    }
    let word = cells[cell.y * frame.grid_width + cell.x];
    let material = (word >> MATERIAL_SHIFT) & 0xffu;
    let variation = (word >> VARIATION_SHIFT) & 0xffu;
    let life = (word >> LIFE_SHIFT) & 0xffu;
    let _flags = (word >> FLAGS_SHIFT) & 0xffu;
    var colour = vec4<f32>(0.0);
    var glow = vec3<f32>(0.0);
    if (material < 9u) {
        let info = materials[material];
        colour = info.colour;
        if (info.kind == 4u) {
            colour = vec4<f32>(info.colour.rgb * (f32(life) / 255.0), 1.0);
        }
        glow = colour.rgb * info.emissive;
    } else {
        colour = vec4<f32>(f32(material) / 255.0, f32(variation) / 255.0, f32(life) / 255.0, 1.0);
    }
    return Shade(colour, vec4<f32>(glow, 1.0), true);
}

@fragment
fn fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let shaded = shade(position);
    if (!shaded.drawn) {
        discard;
    }
    return shaded.colour;
}

@fragment
fn fs_hdr(@builtin(position) position: vec4<f32>) -> HdrOut {
    let shaded = shade(position);
    if (!shaded.drawn) {
        discard;
    }
    return HdrOut(shaded.colour, shaded.glow);
}
