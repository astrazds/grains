const KIND_EMPTY: u32 = 0u;
const KIND_SOLID: u32 = 1u;
const KIND_POWDER: u32 = 2u;
const KIND_LIQUID: u32 = 3u;
const KIND_GAS: u32 = 4u;
const WIDTH: u32 = 512u;
const HEIGHT: u32 = 288u;
const MATERIAL_COUNT: u32 = 6u;

struct Sim {
    offset: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

struct MaterialInfo {
    density: u32,
    kind: u32,
    life: u32,
    _pad: u32,
    colour: vec4<f32>,
}

@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;
@group(0) @binding(2) var<uniform> sim: Sim;
@group(0) @binding(3) var<storage, read> materials: array<MaterialInfo>;

fn material_id(word: u32) -> u32 {
    return word & 0xffu;
}

fn info_of(word: u32) -> MaterialInfo {
    let id = material_id(word);
    if (id >= MATERIAL_COUNT) {
        return materials[0];
    }
    return materials[id];
}

fn kind_of(word: u32) -> u32 {
    return info_of(word).kind;
}

fn density_of(word: u32) -> u32 {
    return info_of(word).density;
}

fn index_at(x: u32, y: u32) -> u32 {
    return y * WIDTH + x;
}

fn prefer_right(x: u32, y: u32) -> bool {
    let mixed = x * 374761393u + y * 668265263u;
    return (mixed & 1u) == 1u;
}

fn falls_through(top: u32, bottom: u32) -> bool {
    let top_kind = kind_of(top);
    if (top_kind != KIND_POWDER && top_kind != KIND_LIQUID) {
        return false;
    }
    if (kind_of(bottom) == KIND_SOLID) {
        return false;
    }
    return density_of(top) > density_of(bottom);
}

fn rises_through(top: u32, bottom: u32) -> bool {
    if (kind_of(bottom) != KIND_GAS) {
        return false;
    }
    let top_kind = kind_of(top);
    if (top_kind == KIND_SOLID || top_kind == KIND_GAS) {
        return false;
    }
    return true;
}

fn exchange(top: ptr<function, u32>, bottom: ptr<function, u32>) {
    if (falls_through(*top, *bottom) || rises_through(*top, *bottom)) {
        let dropped = *top;
        *top = *bottom;
        *bottom = dropped;
    }
}

fn spread(left: ptr<function, u32>, right: ptr<function, u32>, left_above: u32, right_above: u32) {
    let left_kind = kind_of(*left);
    let right_kind = kind_of(*right);
    let left_flows = right_kind == KIND_EMPTY && (left_kind == KIND_GAS || (left_kind == KIND_LIQUID && kind_of(left_above) == KIND_EMPTY));
    let right_flows = left_kind == KIND_EMPTY && (right_kind == KIND_GAS || (right_kind == KIND_LIQUID && kind_of(right_above) == KIND_EMPTY));
    if (left_flows || right_flows) {
        let moved = *left;
        *left = *right;
        *right = moved;
    }
}

fn above_cell(x: u32, y: u32) -> u32 {
    if (y == 0u) {
        return 0u;
    }
    return src[index_at(x, y - 1u)];
}

fn slide(top_left: ptr<function, u32>, top_right: ptr<function, u32>, bottom_left: ptr<function, u32>, bottom_right: ptr<function, u32>, right: bool) {
    if (right) {
        slide_down_right(top_left, top_right, bottom_left, bottom_right);
    } else {
        slide_down_left(top_left, top_right, bottom_left, bottom_right);
    }
}

fn blocked(top: u32, bottom: u32) -> bool {
    return kind_of(top) == KIND_POWDER && !falls_through(top, bottom) && material_id(bottom) != 0u;
}

fn slide_down_right(top_left: ptr<function, u32>, top_right: ptr<function, u32>, bottom_left: ptr<function, u32>, bottom_right: ptr<function, u32>) {
    if (blocked(*top_left, *bottom_left) && kind_of(*bottom_right) == KIND_EMPTY && kind_of(*top_right) == KIND_EMPTY) {
        let slid = *top_left;
        *top_left = *bottom_right;
        *bottom_right = slid;
    } else if (blocked(*top_right, *bottom_right) && kind_of(*bottom_left) == KIND_EMPTY && kind_of(*top_left) == KIND_EMPTY) {
        let slid = *top_right;
        *top_right = *bottom_left;
        *bottom_left = slid;
    }
}

fn slide_down_left(top_left: ptr<function, u32>, top_right: ptr<function, u32>, bottom_left: ptr<function, u32>, bottom_right: ptr<function, u32>) {
    if (blocked(*top_right, *bottom_right) && kind_of(*bottom_left) == KIND_EMPTY && kind_of(*top_left) == KIND_EMPTY) {
        let slid = *top_right;
        *top_right = *bottom_left;
        *bottom_left = slid;
    } else if (blocked(*top_left, *bottom_left) && kind_of(*bottom_right) == KIND_EMPTY && kind_of(*top_right) == KIND_EMPTY) {
        let slid = *top_left;
        *top_left = *bottom_right;
        *bottom_right = slid;
    }
}

fn age(word: u32) -> u32 {
    if (kind_of(word) != KIND_GAS) {
        return word;
    }
    let life = (word >> 16u) & 0xffu;
    if (life <= 1u) {
        return 0u;
    }
    return (word & 0xff00ffffu) | ((life - 1u) << 16u);
}

@compute @workgroup_size(8, 8)
fn step(@builtin(global_invocation_id) id: vec3<u32>) {
    let origin = id.xy * 2u + vec2<u32>(sim.offset, sim.offset);
    if (origin.x + 1u >= WIDTH || origin.y + 1u >= HEIGHT) {
        return;
    }
    let i0 = index_at(origin.x, origin.y);
    let i1 = index_at(origin.x + 1u, origin.y);
    let i2 = index_at(origin.x, origin.y + 1u);
    let i3 = index_at(origin.x + 1u, origin.y + 1u);
    var top_left = src[i0];
    var top_right = src[i1];
    var bottom_left = src[i2];
    var bottom_right = src[i3];

    exchange(&top_left, &bottom_left);
    exchange(&top_right, &bottom_right);
    spread(&top_left, &top_right, above_cell(origin.x, origin.y), above_cell(origin.x + 1u, origin.y));
    spread(&bottom_left, &bottom_right, top_left, top_right);
    slide(&top_left, &top_right, &bottom_left, &bottom_right, prefer_right(origin.x, origin.y));

    dst[i0] = age(top_left);
    dst[i1] = age(top_right);
    dst[i2] = age(bottom_left);
    dst[i3] = age(bottom_right);
}
