const MATERIAL_SHIFT: u32 = 0u;
const VARIATION_SHIFT: u32 = 8u;
const LIFE_SHIFT: u32 = 16u;
const FLAGS_SHIFT: u32 = 24u;

struct Frame {
    cell_x: f32,
    cell_y: f32,
    zoom: f32,
    grid_width: u32,
    grid_height: u32,
    frame_width: u32,
    frame_height: u32,
    _pad: u32,
}

struct MaterialInfo {
    density: u32,
    kind: u32,
    life: u32,
    becomes: u32,
    colour: vec4<f32>,
    emissive: f32,
    grain: f32,
    style: u32,
    _pad: u32,
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
    let sky = backdrop(vec2<f32>(position.xy));
    let zoom = max(frame.zoom, 0.0001);
    let looked_up = vec2<f32>(frame.cell_x, frame.cell_y) + position.xy / zoom;
    if (looked_up.x < 0.0 || looked_up.y < 0.0) {
        return Shade(vec4<f32>(sky, 1.0), vec4<f32>(0.0), true);
    }
    let cell = vec2<u32>(u32(floor(looked_up.x)), u32(floor(looked_up.y)));
    if (cell.x >= frame.grid_width || cell.y >= frame.grid_height) {
        return Shade(vec4<f32>(sky, 1.0), vec4<f32>(0.0), true);
    }
    let word = cells[cell.y * frame.grid_width + cell.x];
    let material = (word >> MATERIAL_SHIFT) & 0xffu;
    let variation = (word >> VARIATION_SHIFT) & 0xffu;
    let life = (word >> LIFE_SHIFT) & 0xffu;
    let _flags = (word >> FLAGS_SHIFT) & 0xffu;
    if (material >= 10u) {
        // Ids past the table stay on the debug path so letterbox tests can read the bytes back.
        let debug = vec3<f32>(f32(material), f32(variation), f32(life)) / 255.0;
        return Shade(vec4<f32>(debug, 2.0), vec4<f32>(0.0), true);
    }
    let info = materials[material];
    let varied = f32(variation) / 255.0;
    var rgb = info.colour.rgb;
    var glow = info.colour.rgb * info.emissive;
    if (info.style == 1u) {
        rgb = info.colour.rgb * (1.0 + (varied - 0.5) * info.grain);
    } else if (info.style == 2u) {
        let highlight = smoothstep(0.72, 1.0, varied) * info.grain;
        let body = info.colour.rgb * (1.0 + (varied - 0.5) * info.grain) + vec3<f32>(highlight);
        rgb = mix(sky, body, 0.84);
    } else if (info.style == 3u) {
        let age = f32(life) / 255.0;
        let hot = vec3<f32>(1.0, 0.93, 0.62);
        let ember = vec3<f32>(0.42, 0.03, 0.01);
        let mid = mix(info.colour.rgb, hot, smoothstep(0.4, 1.0, age));
        rgb = mix(ember, mid, smoothstep(0.0, 0.55, age));
        rgb = rgb * (0.9 + varied * info.grain);
        glow = rgb * info.emissive;
    } else if (info.style == 4u) {
        let fade = f32(life) / 255.0;
        rgb = mix(sky, info.colour.rgb, fade * 0.8);
        glow = info.colour.rgb * fade * info.emissive;
    } else {
        rgb = sky;
    }
    return Shade(vec4<f32>(rgb, 1.0), vec4<f32>(glow, 1.0), true);
}

fn backdrop(pixel: vec2<f32>) -> vec3<f32> {
    let size = max(vec2<f32>(f32(frame.frame_width), f32(frame.frame_height)), vec2<f32>(1.0));
    let uv = pixel / size;
    let sky = mix(vec3<f32>(0.20, 0.22, 0.28), vec3<f32>(0.05, 0.055, 0.07), uv.y);
    let from_centre = length(uv - vec2<f32>(0.5, 0.42));
    return sky * clamp(1.15 - from_centre * 0.7, 0.55, 1.0);
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let shaded = shade(position);
    if (shaded.colour.a > 1.5) {
        return vec4<f32>(shaded.colour.rgb, 1.0);
    }
    return vec4<f32>(aces(shaded.colour.rgb), 1.0);
}

@fragment
fn fs_hdr(@builtin(position) position: vec4<f32>) -> HdrOut {
    let shaded = shade(position);
    return HdrOut(shaded.colour, shaded.glow);
}
