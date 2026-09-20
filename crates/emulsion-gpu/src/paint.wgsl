// Final ordinary stroke composition in premultiplied linear RGBA16.
@group(0) @binding(0) var<storage, read> params: array<u32>;
@group(0) @binding(1) var<storage, read> source: array<vec2<u32>>;
@group(0) @binding(2) var<storage, read> paint: array<f32>;
@group(0) @binding(3) var<storage, read> clips: array<f32>;
@group(0) @binding(4) var<storage, read_write> output: array<vec2<u32>>;

fn blend(base: vec4<f32>, ink: vec4<f32>, multiply: bool) -> vec4<f32> {
    if ink.a <= 0.0 { return base; }
    if multiply {
        let cs = clamp(ink.rgb * (1.0 / ink.a), vec3<f32>(0.0), vec3<f32>(1.0));
        var cb = vec3<f32>(0.0);
        if base.a > 0.0 {
            cb = clamp(base.rgb * (1.0 / base.a), vec3<f32>(0.0), vec3<f32>(1.0));
        }
        let mixed = (1.0 - base.a) * cs + base.a * (cb * cs);
        return vec4<f32>(ink.a * mixed + (1.0 - ink.a) * base.rgb,
            ink.a + base.a * (1.0 - ink.a));
    }
    return ink + base * (1.0 - ink.a);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params[0] { return; }
    let packed = source[i];
    output[i] = packed;
    let offset = i * params[3];
    let coverage = paint[offset + 4u];
    if coverage <= 0.0 { return; }
    let flags = params[1];
    let erase = (flags & 1u) != 0u;
    let alpha_lock = (flags & 2u) != 0u;
    let behind = (flags & 4u) != 0u;
    let multiply = (flags & 8u) != 0u;
    let base = vec4<f32>(
        f32(packed.x & 65535u), f32(packed.x >> 16u),
        f32(packed.y & 65535u), f32(packed.y >> 16u)
    ) * (1.0 / 65535.0);
    if alpha_lock && (base.a <= 0.0 || erase) { return; }
    var k = min(coverage, 1.0) * bitcast<f32>(params[2]);
    k *= clips[i];
    if k <= 0.0 { return; }
    var result: vec4<f32>;
    if erase {
        result = base * (1.0 - k);
    } else {
        if behind { k *= 1.0 - min(base.a, 1.0); }
        let ink = vec4<f32>(paint[offset], paint[offset + 1u],
            paint[offset + 2u], paint[offset + 3u]) / max(coverage, 0.000001) * k;
        if alpha_lock {
            let mixed = blend(vec4<f32>(base.rgb / base.a, 1.0), ink, multiply);
            result = vec4<f32>(mixed.rgb * base.a, base.a);
        } else {
            result = blend(base, ink, multiply);
        }
    }
    let rgba = vec4<u32>(clamp(result, vec4<f32>(0.0), vec4<f32>(1.0)) * 65535.0 + 0.5);
    output[i] = vec2<u32>(rgba.r | (rgba.g << 16u), rgba.b | (rgba.a << 16u));
}
