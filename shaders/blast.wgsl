const EMPTY: u32 = 0u;
const SAND: u32 = 1u;
const STONE: u32 = 2u;
const WATER: u32 = 3u;
const OIL: u32 = 4u;
const FIRE: u32 = 7u;
const TNT: u32 = 9u;
const RADIUS: i32 = 4;
const NONE: u32 = 0xffffffffu;
const CHUNK: u32 = 64u;

struct World {
    offset: u32,
    width: u32,
    height: u32,
    awake: u32,
    chunk_cols: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

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
@group(0) @binding(4) var<uniform> world: World;
@group(0) @binding(5) var<storage, read_write> chunks: array<u32>;

@group(0) @binding(6) var<storage, read_write> dirty: array<atomic<u32>>;

fn mark_dirty(x: u32, y: u32) {
    atomicOr(&dirty[(y / CHUNK) * world.chunk_cols + x / CHUNK], 1u);
}

fn material_of(word: u32) -> u32 {
    return word & 0xffu;
}

fn index_at(x: i32, y: i32) -> u32 {
    return u32(y) * world.width + u32(x);
}

fn inside(x: i32, y: i32) -> bool {
    return x >= 0 && y >= 0 && u32(x) < world.width && u32(y) < world.height;
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
    return id.y * world.width + id.x;
}


fn chunk_xy(id: vec3<u32>) -> vec2<u32> {
    let packed = chunks[id.z];
    return vec2<u32>((packed & 0xffffu) * CHUNK + id.x, (packed >> 16u) * CHUNK + id.y);
}

fn locate(id: vec3<u32>, x: ptr<function, u32>, y: ptr<function, u32>) -> bool {
    if (world.awake == 0xffffffffu) {
        if (id.x >= world.width || id.y >= world.height) {
            return false;
        }
        *x = id.x;
        *y = id.y;
        return true;
    }
    if (id.z >= world.awake || id.x >= CHUNK || id.y >= CHUNK) {
        return false;
    }
    let cell = chunk_xy(id);
    if (cell.x >= world.width || cell.y >= world.height) {
        return false;
    }
    *x = cell.x;
    *y = cell.y;
    return true;
}

@compute @workgroup_size(8, 8)
fn blast(@builtin(global_invocation_id) id: vec3<u32>) {
    var x: u32;
    var y: u32;
    if (!locate(id, &x, &y)) {
        return;
    }
    let here = y * world.width + x;
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
            let sx = i32(x) + dx;
            let sy = i32(y) + dy;
            if (!detonator(sx, sy) || dist2 >= nearest) {
                continue;
            }
            nearest = dist2;
            center_x = sx;
            center_y = sy;
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
        let throw_x = f32(i32(x) - center_x);
        let throw_y = f32(i32(y) - center_y);
        let span = max(sqrt(throw_x * throw_x + throw_y * throw_y), 1.0);
        var thrown: Particle;
        thrown.x = f32(x) + 0.5;
        thrown.y = f32(y) + 0.5;
        thrown.vx = throw_x / span * 1.8;
        thrown.vy = throw_y / span * 1.8 - 1.4;
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
    var x: u32;
    var y: u32;
    if (!locate(id, &x, &y)) {
        return;
    }
    let here = y * world.width + x;
    var speck = particles[here];
    if (speck.word == 0u) {
        return;
    }
    // Flight is stored at the launch cell until landing. Keep its chunk
    // and the moving destination awake even when the grid itself is still.
    mark_dirty(x, y);
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
    speck.x = clamp(speck.x, 0.5, f32(world.width) - 0.5);
    speck.y = clamp(speck.y, 0.5, f32(world.height) - 0.5);
    mark_dirty(u32(floor(speck.x)), u32(floor(speck.y)));
    particles[here] = speck;
}

@compute @workgroup_size(8, 8)
fn clear_claims(@builtin(global_invocation_id) id: vec3<u32>) {
    var x: u32;
    var y: u32;
    if (!locate(id, &x, &y)) {
        return;
    }
    atomicStore(&claims[y * world.width + x], NONE);
}

@compute @workgroup_size(8, 8)
fn claim(@builtin(global_invocation_id) id: vec3<u32>) {
    var cell_x: u32;
    var cell_y: u32;
    if (!locate(id, &cell_x, &cell_y)) {
        return;
    }
    let here = cell_y * world.width + cell_x;
    let speck = particles[here];
    if (speck.word == 0u || speck.age < 8u) {
        return;
    }
    let x = i32(floor(speck.x));
    let y = i32(floor(speck.y));
    if (!inside(x, y)) {
        return;
    }
    atomicMin(&claims[index_at(x, y)], here);
}

@compute @workgroup_size(8, 8)
fn land(@builtin(global_invocation_id) id: vec3<u32>) {
    var x: u32;
    var y: u32;
    if (!locate(id, &x, &y)) {
        return;
    }
    let here = y * world.width + x;
    let slot = atomicLoad(&claims[here]);
    if (slot == NONE || material_of(settled[here]) != EMPTY) {
        return;
    }
    let word = particles[slot].word;
    if (word == 0u) {
        return;
    }
    settled[here] = word;
    mark_dirty(x, y);
    particles[slot].word = 0u;
    particles[slot].vx = 0.0;
    particles[slot].vy = 0.0;
}

@compute @workgroup_size(8, 8)
fn nudge(@builtin(global_invocation_id) id: vec3<u32>) {
    var cell_x: u32;
    var cell_y: u32;
    if (!locate(id, &cell_x, &cell_y)) {
        return;
    }
    let here = cell_y * world.width + cell_x;
    var speck = particles[here];
    if (speck.word == 0u || speck.age < 8u) {
        return;
    }
    let x = i32(floor(speck.x));
    let y = i32(floor(speck.y));
    if (!inside(x, y)) {
        return;
    }
    let won = atomicLoad(&claims[index_at(x, y)]) == here;
    let blocked = material_of(settled[index_at(x, y)]) != EMPTY;
    if (won && !blocked) {
        return;
    }
    // Look both ways in this column so a thick floor or ceiling cannot
    // trap a grain. If the whole column is full, try the next one next pass.
    var retry_x = (u32(x) + 1u) % world.width;
    var retry_y = y;
    for (var distance = 1; distance < i32(world.height); distance++) {
        let above = y - distance;
        let below = y + distance;
        if (above >= 0 && material_of(settled[index_at(x, above)]) == EMPTY) {
            retry_x = u32(x);
            retry_y = above;
            break;
        }
        if (below < i32(world.height) && material_of(settled[index_at(x, below)]) == EMPTY) {
            retry_x = u32(x);
            retry_y = below;
            break;
        }
    }
    speck.x = f32(retry_x) + 0.5;
    speck.y = f32(retry_y) + 0.5;
    speck.vx = 0.0;
    speck.vy = 0.0;
    mark_dirty(retry_x, u32(retry_y));
    particles[here] = speck;
}
