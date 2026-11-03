struct Screen {
    width: f32,
    height: f32,
    _pad0: f32,
    _pad1: f32,
}

@group(0) @binding(0) var<uniform> screen: Screen;

struct VsIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs(input: VsIn) -> VsOut {
    let ndc = vec2<f32>(
        input.pos.x / max(screen.width, 1.0) * 2.0 - 1.0,
        1.0 - input.pos.y / max(screen.height, 1.0) * 2.0,
    );
    return VsOut(vec4<f32>(ndc, 0.0, 1.0), input.color);
}

@fragment
fn fs(input: VsOut) -> @location(0) vec4<f32> {
    return input.color;
}
