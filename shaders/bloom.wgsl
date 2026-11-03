const DOWNSAMPLE: i32 = 4;

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var bloom_tex: texture_2d<f32>;
@group(0) @binding(2) var bloom_sampler: sampler;

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

@fragment
fn bright(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let origin = vec2<i32>(floor(position.xy)) * DOWNSAMPLE;
    var sum = vec3<f32>(0.0);
    for (var y = 0; y < DOWNSAMPLE; y++) {
        for (var x = 0; x < DOWNSAMPLE; x++) {
            sum += textureLoad(source, origin + vec2<i32>(x, y), 0).rgb;
        }
    }
    return vec4<f32>(sum / f32(DOWNSAMPLE * DOWNSAMPLE), 1.0);
}

fn blur_along(step: vec2<i32>, position: vec4<f32>) -> vec4<f32> {
    let pixel = vec2<i32>(floor(position.xy));
    let centre = textureLoad(source, pixel, 0).rgb;
    let near = textureLoad(source, pixel + step, 0).rgb + textureLoad(source, pixel - step, 0).rgb;
    let far = textureLoad(source, pixel + step * 2, 0).rgb + textureLoad(source, pixel - step * 2, 0).rgb;
    return vec4<f32>(centre * 0.4 + near * 0.2 + far * 0.1, 1.0);
}

@fragment
fn blur_x(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return blur_along(vec2<i32>(1, 0), position);
}

@fragment
fn blur_y(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return blur_along(vec2<i32>(0, 1), position);
}

@fragment
fn composite(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(floor(position.xy));
    let base = textureLoad(source, pixel, 0);
    let size = vec2<f32>(textureDimensions(source));
    let uv = (position.xy + vec2<f32>(0.5)) / size;
    let glow = textureSample(bloom_tex, bloom_sampler, uv);
    let lit = base.rgb + glow.rgb * 0.75;
    if (base.a > 1.5) {
        return vec4<f32>(clamp(base.rgb + glow.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
    }
    return vec4<f32>(aces_tonemap(lit), 1.0);
}

fn aces_tonemap(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn blit(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(floor(position.xy));
    return textureLoad(source, pixel, 0);
}
