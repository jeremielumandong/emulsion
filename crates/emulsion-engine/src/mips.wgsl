// Per-tile 2×2 box reduction of mip k-1 into mip k of one atlas page.
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<storage, read> slots: array<u32>;
@group(0) @binding(2) var<uniform> level: vec4<u32>;

const PAGE: f32 = 2048.0;
const PER_ROW: u32 = 8u;
const PER_PAGE: u32 = 64u;

@vertex
fn vs(@builtin(vertex_index) v: u32, @builtin(instance_index) i: u32) -> @builtin(position) vec4<f32> {
    let local = slots[i] % PER_PAGE;
    let size = 256.0 / f32(1u << level.x);
    let origin = vec2<f32>(f32(local % PER_ROW), f32(local / PER_ROW)) * size;
    let corner = array(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0))[v];
    let p = (origin + corner * size) / (PAGE / f32(1u << level.x));
    return vec4(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
}

fn taps(p: vec2<i32>) -> array<vec4<f32>, 4> {
    let q = p * 2;
    return array(
        textureLoad(source, q, 0),
        textureLoad(source, q + vec2(1, 0), 0),
        textureLoad(source, q + vec2(0, 1), 0),
        textureLoad(source, q + vec2(1, 1), 0),
    );
}

// Emulsion's CPU mips: (a + b + c + d + 2) >> 2 on u16 channels.
@fragment
fn fs_unorm16(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let t = taps(vec2<i32>(position.xy));
    let sum = round(t[0] * 65535.0) + round(t[1] * 65535.0) + round(t[2] * 65535.0) + round(t[3] * 65535.0);
    return floor((sum + 2.0) / 4.0) / 65535.0;
}

@fragment
fn fs_float(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let t = taps(vec2<i32>(position.xy));
    return (t[0] + t[1] + t[2] + t[3]) * 0.25;
}
