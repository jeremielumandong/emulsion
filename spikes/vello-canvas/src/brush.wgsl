// Brush test B: round dabs drawn straight into atlas tiles.
//
// Hardware blending (One, OneMinusSrcAlpha) performs the CPU accumulation
// `ink = colour·a + ink·(1 − a)` per dab in draw order. With stroke opacity 1
// and Normal blending, stamping into the layer equals accumulating ink and
// compositing it over the layer once: both give ink + layer·Π(1 − aᵢ).
// Shape and anti-aliasing follow `emulsion-gpu`'s persistent_paint.wgsl.

struct Quad {
    // Document-space rectangle to cover (x0, y0, x1, y1).
    rect: vec4<f32>,
    // Tile origin in document space (xy) and in the atlas page (zw).
    origin: vec4<f32>,
    // Centre, radius, hardness.
    dab: vec4<f32>,
    // Premultiplied linear colour.
    color: vec4<f32>,
    // Flow.
    extra: vec4<f32>,
}

@group(0) @binding(0) var<storage, read> quads: array<Quad>;

const PAGE: f32 = 2048.0;

struct Varying {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) index: u32,
}

@vertex
fn vs(@builtin(vertex_index) v: u32, @builtin(instance_index) i: u32) -> Varying {
    let q = quads[i];
    let corner = array(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0))[v];
    let doc = mix(q.rect.xy, q.rect.zw, corner);
    let p = (doc - q.origin.xy + q.origin.zw) / PAGE;
    return Varying(vec4(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0), i);
}

fn falloff(d: f32, hardness: f32) -> f32 {
    if d >= 1.0 { return 0.0; }
    let h = clamp(hardness, 0.0, 0.99);
    if d <= h { return 1.0; }
    let t = (d - h) / (1.0 - h);
    return 1.0 - t * t * (3.0 - 2.0 * t);
}

@fragment
fn fs_dab(in: Varying) -> @location(0) vec4<f32> {
    let q = quads[in.index];
    let point = in.position.xy - q.origin.zw + q.origin.xy;
    let offset = point - q.dab.xy;
    let r = q.dab.z;
    let hard = q.dab.w;
    let d = length(offset) / r;
    let footprint = 0.7071067811865476 / r;
    var shape = falloff(d, hard);
    if ((1.0 - hard) * r < 1.0 || r < 2.0) && d + footprint > hard && d - footprint < 1.0 {
        shape = 0.0;
        for (var sy = 0u; sy < 4u; sy++) {
            for (var sx = 0u; sx < 4u; sx++) {
                let s = vec2(f32(sx) * 0.25 - 0.375, f32(sy) * 0.25 - 0.375);
                shape += falloff(length(offset + s) / r, hard);
            }
        }
        shape /= 16.0;
    }
    let a = shape * q.extra.x;
    if a <= 0.0005 { discard; }
    return q.color * a;
}

@fragment
fn fs_clear(in: Varying) -> @location(0) vec4<f32> {
    return quads[in.index].color;
}
