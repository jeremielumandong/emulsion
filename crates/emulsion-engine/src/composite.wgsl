// Screen compositor. Prepended at build time with `emulsion-gpu`'s blend
// functions (`channel`, `mix_color`, `blend`, `encode`, `decode`), which read
// the blend space from `program[1]`.

struct View {
    screen: vec2<f32>,
    doc: vec2<f32>,
    // Document coordinate at the screen's top-left corner.
    origin: vec2<f32>,
    // Document pixels per screen pixel.
    scale: f32,
    level: u32,
    hud: vec2<f32>,
    checker: f32,
    // 0: surface output, sRGB encoded in the shader.
    // 1: premultiplied linear, for fidelity readback.
    // 2: surface output, hardware sRGB encoding.
    output: u32,
    // 0: Vello targets hold sRGB-encoded colour; 1: linear colour.
    vector_space: u32,
    runs: u32,
    // Leading ops whose result comes from the composite cache (0: none).
    cached_ops: u32,
    // Cache tiles per row at `level`.
    cache_columns: u32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(2) var<storage, read> tables: array<u32>;
@group(0) @binding(3) var atlas: texture_2d_array<f32>;

// Screen pass only.
@group(1) @binding(0) var vectors: texture_2d_array<f32>;
@group(1) @binding(1) var hud: texture_2d<f32>;
@group(1) @binding(2) var<storage, read> cache_table: array<u32>;
@group(1) @binding(3) var cache: texture_2d_array<f32>;

// Cache fill only: (slot, level, tile x, tile y) per instance.
@group(1) @binding(4) var<storage, read> fills: array<vec4<u32>>;

const NONE: u32 = 0xffffffffu;
const HEADER: u32 = 8u;
const OP_WORDS: u32 = 16u;

fn slot_texel(slot: u32, size: i32) -> vec2<i32> {
    let local = slot % 64u;
    return vec2<i32>(i32(local % 8u), i32(local / 8u)) * size;
}

fn sample_source(o: u32, px: vec2<i32>, level: u32) -> vec4<f32> {
    let tiles = vec2<i32>(i32(program[o + 6u]), i32(program[o + 7u]));
    let t = px / 256;
    if t.x >= tiles.x || t.y >= tiles.y { return vec4(0.0); }
    let slot = tables[program[o + 5u] + u32(t.y * tiles.x + t.x)];
    if slot == NONE { return vec4(0.0); }
    let texel = slot_texel(slot, 256 >> level) + ((px - t * 256) >> vec2(level));
    return textureLoad(atlas, texel, i32(slot / 64u), i32(level));
}

fn sample_vector(run: u32, screen: vec2<i32>) -> vec4<f32> {
    let v = textureLoad(vectors, screen, i32(run), 0);
    var rgb = v.rgb;
    if view.vector_space == 0u { rgb = vec3(decode(v.r), decode(v.g), decode(v.b)); }
    return vec4(rgb * v.a, v.a);
}

// Run ops [first, last) over `start`. The range never splits a group or a
// clipping base from its clipped layers (see `Canvas::cacheable_prefix`).
fn composite_range(px: vec2<i32>, screen: vec2<i32>, level: u32, first: u32, last: u32, start: vec4<f32>) -> vec4<f32> {
    var acc = start;
    var stack: array<vec4<f32>, 8>;
    var alpha: array<f32, 16>;
    for (var s = 0u; s < 16u; s++) { alpha[s] = 1.0; }
    var depth = 0u;
    for (var i = first; i < last; i++) {
        let o = HEADER + i * OP_WORDS;
        let op = program[o];
        if op == 1u || op == 2u {
            stack[depth] = acc;
            depth++;
            if op == 1u { acc = vec4(0.0); }
            continue;
        }
        let mode = program[o + 1u];
        let slot = program[o + 2u];
        let clip = program[o + 3u];
        var coverage = bitcast<f32>(program[o + 4u]);
        if clip != NONE { coverage *= alpha[clip]; }
        if op == 3u || op == 4u {
            depth--;
            var mask = 1.0;
            if program[o + 8u] != NONE { mask = sample_source(o, px, level).a; }
            if op == 4u {
                acc = stack[depth] + (acc - stack[depth]) * (coverage * mask);
            } else {
                let src = acc * mask;
                if slot != NONE { alpha[slot] = src.a; }
                acc = stack[depth];
                if coverage > 0.0 { acc = blend(mode, acc, src * coverage); }
            }
            continue;
        }
        var src: vec4<f32>;
        if op == 0u {
            src = sample_source(o, px, level);
        } else if op == 5u {
            src = vec4(bitcast<f32>(program[o + 8u]), bitcast<f32>(program[o + 9u]), bitcast<f32>(program[o + 10u]), bitcast<f32>(program[o + 11u]));
        } else {
            src = sample_vector(program[o + 5u], screen);
        }
        if slot != NONE { alpha[slot] = src.a; }
        if coverage > 0.0 { acc = blend(mode, acc, src * coverage); }
    }
    return acc;
}

@vertex
fn vs(@builtin(vertex_index) v: u32) -> @builtin(position) vec4<f32> {
    let p = vec2(f32((v << 1u) & 2u), f32(v & 2u));
    return vec4(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let screen = position.xy;
    let d = view.origin + screen * view.scale;
    let inside = all(d >= vec2(0.0)) && all(d < view.doc);
    var acc = vec4(0.0);
    if inside {
        let px = vec2<i32>(floor(d));
        var start = vec4(0.0);
        if view.cached_ops > 0u {
            let lp = px >> vec2(view.level);
            let ct = lp / 256;
            let slot = cache_table[u32(ct.y) * view.cache_columns + u32(ct.x)];
            if slot != NONE {
                start = textureLoad(cache, slot_texel(slot, 256) + (lp - ct * 256), i32(slot / 64u), 0);
            }
        }
        acc = composite_range(px, vec2<i32>(screen), view.level, view.cached_ops, program[0], start);
    }
    if view.output == 1u { return acc; }
    var background = 0.05;
    if inside {
        let cell = vec2<i32>(floor(screen / view.checker));
        background = select(0.527, 0.807, ((cell.x + cell.y) & 1) == 0);
    }
    let lin = clamp(acc.rgb + background * (1.0 - clamp(acc.a, 0.0, 1.0)), vec3(0.0), vec3(1.0));
    var out = vec3(encode(lin.r), encode(lin.g), encode(lin.b));
    if all(screen < view.hud) {
        let h = textureLoad(hud, vec2<i32>(screen), 0);
        out = mix(out, h.rgb, h.a);
    }
    if view.output == 2u { out = vec3(decode(out.r), decode(out.g), decode(out.b)); }
    return vec4(out, 1.0);
}

// ── Composite cache fill: one quad per cache tile, drawn into its page ──

struct Fill {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) index: u32,
}

@vertex
fn vs_fill(@builtin(vertex_index) v: u32, @builtin(instance_index) i: u32) -> Fill {
    let origin = vec2<f32>(slot_texel(fills[i].x, 256));
    let corner = array(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0))[v];
    let p = (origin + corner * 256.0) / 2048.0;
    return Fill(vec4(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0), i);
}

@fragment
fn fs_fill(in: Fill) -> @location(0) vec4<f32> {
    let f = fills[in.index];
    let local = vec2<i32>(in.position.xy) - slot_texel(f.x, 256);
    let lp = vec2<i32>(i32(f.z), i32(f.w)) * 256 + local;
    let px = lp << vec2(f.y);
    if any(vec2<f32>(px) >= view.doc) { return vec4(0.0); }
    return composite_range(px, vec2(0), f.y, 0u, view.cached_ops, vec4(0.0));
}
