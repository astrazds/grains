const STONE: u32 = 2u;

struct Brush {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    radius: u32,
    material: u32,
    erase: u32,
    clear: u32,
    width: u32,
    height: u32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<storage, read_write> cells: array<u32>;
@group(0) @binding(1) var<uniform> brush: Brush;

fn distance_to_stroke(x: f32, y: f32) -> f32 {
    let abx = f32(brush.x1 - brush.x0);
    let aby = f32(brush.y1 - brush.y0);
    let apx = x - f32(brush.x0);
    let apy = y - f32(brush.y0);
    let length_sq = abx * abx + aby * aby;
    var t = 0.0;
    if (length_sq > 0.0) {
        t = clamp((apx * abx + apy * aby) / length_sq, 0.0, 1.0);
    }
    let dx = x - (f32(brush.x0) + abx * t);
    let dy = y - (f32(brush.y0) + aby * t);
    return sqrt(dx * dx + dy * dy);
}

@compute @workgroup_size(8, 8)
fn paint(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= brush.width || id.y >= brush.height) {
        return;
    }
    let index = id.y * brush.width + id.x;
    if (brush.clear != 0u) {
        cells[index] = 0u;
        return;
    }
    if (distance_to_stroke(f32(id.x), f32(id.y)) > f32(brush.radius) + 0.5) {
        return;
    }
    if (brush.erase == 0u && (cells[index] & 0xffu) == STONE) {
        return;
    }
    let painted = (brush.material & 0xffff00ffu) | (grain_of(id.x, id.y) << 8u);
    cells[index] = select(painted, 0u, brush.erase != 0u);
}

fn grain_of(x: u32, y: u32) -> u32 {
    // Tint rejects a `*` mixed with `^` unless both sides are parenthesized.
    let mixed = (x * 374761393u) ^ (y * 668265263u);
    return (mixed >> 16u) & 0xffu;
}
