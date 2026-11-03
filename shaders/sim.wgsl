const EMPTY: u32 = 0u;
const SAND: u32 = 1u;
const WIDTH: u32 = 512u;
const HEIGHT: u32 = 288u;

struct Sim {
    offset: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;
@group(0) @binding(2) var<uniform> sim: Sim;

fn material(word: u32) -> u32 {
    return word & 0xffu;
}

fn index(x: u32, y: u32) -> u32 {
    return y * WIDTH + x;
}

fn prefer_right(x: u32, y: u32) -> bool {
    let mixed = x * 374761393u + y * 668265263u;
    return (mixed & 1u) == 1u;
}

@compute @workgroup_size(8, 8)
fn step(@builtin(global_invocation_id) id: vec3<u32>) {
    let origin = id.xy * 2u + vec2<u32>(sim.offset, sim.offset);
    if (origin.x + 1u >= WIDTH || origin.y + 1u >= HEIGHT) {
        return;
    }
    let i0 = index(origin.x, origin.y);
    let i1 = index(origin.x + 1u, origin.y);
    let i2 = index(origin.x, origin.y + 1u);
    let i3 = index(origin.x + 1u, origin.y + 1u);
    var top_left = src[i0];
    var top_right = src[i1];
    var bottom_left = src[i2];
    var bottom_right = src[i3];

    if (material(top_left) == SAND && material(bottom_left) == EMPTY) {
        let fallen = top_left;
        top_left = bottom_left;
        bottom_left = fallen;
    }
    if (material(top_right) == SAND && material(bottom_right) == EMPTY) {
        let fallen = top_right;
        top_right = bottom_right;
        bottom_right = fallen;
    }

    if (prefer_right(origin.x, origin.y)) {
        slide_right(&top_left, &top_right, &bottom_left, &bottom_right);
    } else {
        slide_left(&top_left, &top_right, &bottom_left, &bottom_right);
    }

    dst[i0] = top_left;
    dst[i1] = top_right;
    dst[i2] = bottom_left;
    dst[i3] = bottom_right;
}

fn slide_right(top_left: ptr<function, u32>, top_right: ptr<function, u32>, bottom_left: ptr<function, u32>, bottom_right: ptr<function, u32>) {
    if (material(*top_left) == SAND && material(*bottom_left) != EMPTY && material(*bottom_right) == EMPTY && material(*top_right) == EMPTY) {
        let slid = *top_left;
        *top_left = *bottom_right;
        *bottom_right = slid;
    } else if (material(*top_right) == SAND && material(*bottom_right) != EMPTY && material(*bottom_left) == EMPTY && material(*top_left) == EMPTY) {
        let slid = *top_right;
        *top_right = *bottom_left;
        *bottom_left = slid;
    }
}

fn slide_left(top_left: ptr<function, u32>, top_right: ptr<function, u32>, bottom_left: ptr<function, u32>, bottom_right: ptr<function, u32>) {
    if (material(*top_right) == SAND && material(*bottom_right) != EMPTY && material(*bottom_left) == EMPTY && material(*top_left) == EMPTY) {
        let slid = *top_right;
        *top_right = *bottom_left;
        *bottom_left = slid;
    } else if (material(*top_left) == SAND && material(*bottom_left) != EMPTY && material(*bottom_right) == EMPTY && material(*top_right) == EMPTY) {
        let slid = *top_left;
        *top_left = *bottom_right;
        *bottom_right = slid;
    }
}
