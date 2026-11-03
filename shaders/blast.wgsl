const WIDTH: u32 = 512u;
const HEIGHT: u32 = 288u;
const EMPTY: u32 = 0u;
const SAND: u32 = 1u;
const STONE: u32 = 2u;
const WATER: u32 = 3u;
const OIL: u32 = 4u;
const FIRE: u32 = 7u;
const TNT: u32 = 9u;
const RADIUS: i32 = 4;
const NONE: u32 = 0xffffffffu;

struct Particle {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    word: u32,
    age: u32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<storage, read_write> live: array<u32>;
@group(0) @binding(1) var<storage, read_write> settled: array<u32>;
@group(0) @binding(2) var<storage, read_write> particles: array<Particle>;
@group(0) @binding(3) var<storage, read_write> claims: array<atomic<u32>>;

fn material_of(word: u32) -> u32 {
    return word & 0xffu;
}

fn index_at(x: i32, y: i32) -> u32 {
    return u32(y) * WIDTH + u32(x);
}

fn inside(x: i32, y: i32) -> bool {
    return x >= 0 && y >= 0 && u32(x) < WIDTH && u32(y) < HEIGHT;
}

fn loose(word: u32) -> bool {
    let id = material_of(word);
    return id == SAND || id == WATER || id == OIL;
}

fn flash(life: u32) -> u32 {
    return FIRE | (life << 16u);
}

fn detonator(x: i32, y: i32) -> bool {
    if (!inside(x, y)) {
        return false;
    }
    if (material_of(settled[index_at(x, y)]) != TNT) {
        return false;
    }
    let around = array<vec2<i32>, 8>(
        vec2<i32>(-1, 0),
        vec2<i32>(1, 0),
        vec2<i32>(0, -1),
        vec2<i32>(0, 1),
        vec2<i32>(-1, -1),
        vec2<i32>(1, -1),
        vec2<i32>(-1, 1),
        vec2<i32>(1, 1),
    );
    for (var n = 0; n < 8; n++) {
        let nx = x + around[n].x;
        let ny = y + around[n].y;
        if (inside(nx, ny) && material_of(settled[index_at(nx, ny)]) == FIRE) {
            return true;
        }
    }
    return false;
}

fn cell_of(id: vec2<u32>) -> u32 {
    return id.y * WIDTH + id.x;
}

@compute @workgroup_size(8, 8)
fn blast(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= WIDTH || id.y >= HEIGHT) {
        return;
    }
    let here = cell_of(id.xy);
    let word = settled[here];
    var nearest = 999;
    var center_x = 0;
    var center_y = 0;
    var found = false;
    for (var dy = -RADIUS; dy <= RADIUS; dy++) {
        for (var dx = -RADIUS; dx <= RADIUS; dx++) {
            let dist2 = dx * dx + dy * dy;
            if (dist2 > RADIUS * RADIUS) {
                continue;
            }
            let x = i32(id.x) + dx;
            let y = i32(id.y) + dy;
            if (!detonator(x, y) || dist2 >= nearest) {
                continue;
            }
            nearest = dist2;
            center_x = x;
            center_y = y;
            found = true;
        }
    }
    if (!found || material_of(word) == STONE) {
        live[here] = word;
        return;
    }
    if (material_of(word) == TNT) {
        live[here] = flash(8u);
        return;
    }
    if (loose(word) && particles[here].word == 0u) {
        let dx = f32(i32(id.x) - center_x);
        let dy = f32(i32(id.y) - center_y);
        let span = max(sqrt(dx * dx + dy * dy), 1.0);
        var thrown: Particle;
        thrown.x = f32(id.x) + 0.5;
        thrown.y = f32(id.y) + 0.5;
        thrown.vx = dx / span * 1.8;
        thrown.vy = dy / span * 1.8 - 1.4;
        thrown.word = word;
        thrown.age = 0u;
        thrown._pad0 = 0u;
        thrown._pad1 = 0u;
        particles[here] = thrown;
        if (nearest <= 2) {
            live[here] = flash(6u);
        } else {
            live[here] = EMPTY;
        }
        return;
    }
    if (material_of(word) == EMPTY && nearest <= 4) {
        live[here] = flash(6u);
        return;
    }
    live[here] = word;
}

@compute @workgroup_size(8, 8)
fn fly(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= WIDTH || id.y >= HEIGHT) {
        return;
    }
    let here = cell_of(id.xy);
    var speck = particles[here];
    if (speck.word == 0u) {
        return;
    }
    speck.vy = speck.vy + 0.16;
    speck.x = speck.x + speck.vx;
    speck.y = speck.y + speck.vy;
    speck.vx = speck.vx * 0.97;
    speck.vy = speck.vy * 0.97;
    speck.age = speck.age + 1u;
    if (speck.x != speck.x || speck.vx != speck.vx) {
        speck.x = 0.5;
        speck.vx = 0.0;
    }
    if (speck.y != speck.y || speck.vy != speck.vy) {
        speck.y = 0.5;
        speck.vy = 0.0;
    }
    speck.x = clamp(speck.x, 0.5, f32(WIDTH) - 1.5);
    speck.y = clamp(speck.y, 0.5, f32(HEIGHT) - 1.5);
    particles[here] = speck;
}

@compute @workgroup_size(8, 8)
fn clear_claims(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= WIDTH || id.y >= HEIGHT) {
        return;
    }
    atomicStore(&claims[cell_of(id.xy)], NONE);
}

@compute @workgroup_size(8, 8)
fn claim(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= WIDTH || id.y >= HEIGHT) {
        return;
    }
    let here = cell_of(id.xy);
    let speck = particles[here];
    if (speck.word == 0u || speck.age < 8u) {
        return;
    }
    let x = i32(floor(speck.x + 0.5));
    let y = i32(floor(speck.y + 0.5));
    if (!inside(x, y)) {
        return;
    }
    atomicMin(&claims[index_at(x, y)], here);
}

@compute @workgroup_size(8, 8)
fn land(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= WIDTH || id.y >= HEIGHT) {
        return;
    }
    let here = cell_of(id.xy);
    let slot = atomicLoad(&claims[here]);
    if (slot == NONE || material_of(settled[here]) != EMPTY) {
        return;
    }
    let word = particles[slot].word;
    if (word == 0u) {
        return;
    }
    settled[here] = word;
    particles[slot].word = 0u;
    particles[slot].vx = 0.0;
    particles[slot].vy = 0.0;
}

@compute @workgroup_size(8, 8)
fn nudge(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= WIDTH || id.y >= HEIGHT) {
        return;
    }
    let here = cell_of(id.xy);
    var speck = particles[here];
    if (speck.word == 0u || speck.age < 8u) {
        return;
    }
    let x = i32(floor(speck.x + 0.5));
    let y = i32(floor(speck.y + 0.5));
    if (!inside(x, y)) {
        return;
    }
    let won = atomicLoad(&claims[index_at(x, y)]) == here;
    let blocked = material_of(settled[index_at(x, y)]) != EMPTY;
    if (won && !blocked) {
        return;
    }
    speck.x = clamp(speck.x + 1.25, 0.5, f32(WIDTH) - 1.5);
    speck.y = clamp(speck.y + 0.4, 0.5, f32(HEIGHT) - 1.5);
    particles[here] = speck;
}
