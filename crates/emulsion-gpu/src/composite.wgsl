@group(0) @binding(0) var<storage, read> program: array<u32>;
@group(0) @binding(1) var<storage, read> sources: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> output: array<vec4<f32>>;

fn burn(b: f32, s: f32) -> f32 {
    if b >= 1.0 { return 1.0; }
    if s <= 0.0 { return 0.0; }
    return 1.0 - min((1.0 - b) / s, 1.0);
}
fn dodge(b: f32, s: f32) -> f32 {
    if b <= 0.0 { return 0.0; }
    if s >= 1.0 { return 1.0; }
    return min(b / (1.0 - s), 1.0);
}
fn hard(b: f32, s: f32) -> f32 {
    if s <= 0.5 { return 2.0 * b * s; }
    return 1.0 - 2.0 * (1.0 - b) * (1.0 - s);
}
fn channel(mode: u32, b: f32, s: f32) -> f32 {
    switch mode {
        case 1u: { return min(b, s); }
        case 2u: { return b * s; }
        case 3u: { return burn(b, s); }
        case 4u: { return max(b + s - 1.0, 0.0); }
        case 5u: { return max(b, s); }
        case 6u: { return b + s - b * s; }
        case 7u: { return dodge(b, s); }
        case 8u: { return min(b + s, 1.0); }
        case 9u: { return hard(s, b); }
        case 10u: {
            if s <= 0.5 { return b - (1.0 - 2.0 * s) * b * (1.0 - b); }
            var d = sqrt(b);
            if b <= 0.25 { d = ((16.0 * b - 12.0) * b + 4.0) * b; }
            return b + (2.0 * s - 1.0) * (d - b);
        }
        case 11u: { return hard(b, s); }
        case 12u: {
            if s <= 0.5 { return burn(b, 2.0 * s); }
            return dodge(b, 2.0 * s - 1.0);
        }
        case 13u: { return clamp(b + 2.0 * s - 1.0, 0.0, 1.0); }
        case 14u: {
            if s <= 0.5 { return min(b, 2.0 * s); }
            return max(b, 2.0 * s - 1.0);
        }
        case 15u: { return select(0.0, 1.0, b + s >= 1.0); }
        case 16u: { return abs(b - s); }
        case 17u: { return b + s - 2.0 * b * s; }
        case 18u: { return max(b - s, 0.0); }
        case 19u: {
            if s <= 0.0 { return select(0.0, 1.0, b > 0.0); }
            return min(b / s, 1.0);
        }
        default: { return s; }
    }
}
fn encode(v: f32) -> f32 {
    if v <= 0.0031308 { return 12.92 * v; }
    return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}
fn decode(v: f32) -> f32 {
    if v <= 0.04045 { return v / 12.92; }
    return pow((v + 0.055) / 1.055, 2.4);
}
fn lum(c: vec3<f32>) -> f32 { return 0.3 * c.r + 0.59 * c.g + 0.11 * c.b; }
fn sat(c: vec3<f32>) -> f32 { return max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b)); }
fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    let mx = max(c.r, max(c.g, c.b));
    let mn = min(c.r, min(c.g, c.b));
    if mx - mn <= 1e-9 { return vec3(0.0); }
    return (c - vec3(mn)) * s / (mx - mn);
}
fn set_lum(c: vec3<f32>, desired: f32) -> vec3<f32> {
    let adjusted = c + vec3(desired - lum(c));
    let l = lum(adjusted);
    let n = min(adjusted.r, min(adjusted.g, adjusted.b));
    let x = max(adjusted.r, max(adjusted.g, adjusted.b));
    var result = adjusted;
    if n < 0.0 { result = vec3(l) + (result - vec3(l)) * l / max(l - n, 1e-9); }
    if x > 1.0 { result = vec3(l) + (result - vec3(l)) * (1.0 - l) / max(x - l, 1e-9); }
    return result;
}
fn mix_color(mode: u32, cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    switch mode {
        case 20u: { if lum(cs) < lum(cb) { return cs; } return cb; }
        case 21u: { if lum(cs) > lum(cb) { return cs; } return cb; }
        case 22u: { return set_lum(set_sat(cs, sat(cb)), lum(cb)); }
        case 23u: { return set_lum(set_sat(cb, sat(cs)), lum(cb)); }
        case 24u: { return set_lum(cs, lum(cb)); }
        case 25u: { return set_lum(cb, lum(cs)); }
        default: { return vec3(channel(mode, cb.r, cs.r), channel(mode, cb.g, cs.g), channel(mode, cb.b, cs.b)); }
    }
}
fn blend(mode: u32, dst: vec4<f32>, src: vec4<f32>) -> vec4<f32> {
    if src.a <= 0.0 { return dst; }
    if mode == 0u { return src + dst * (1.0 - src.a); }
    let cs = clamp(src.rgb / src.a, vec3(0.0), vec3(1.0));
    var cb = vec3(0.0);
    if dst.a > 0.0 { cb = clamp(dst.rgb / dst.a, vec3(0.0), vec3(1.0)); }
    var mixed: vec3<f32>;
    if program[1] == 1u {
        let encoded = mix_color(mode, vec3(encode(cb.r), encode(cb.g), encode(cb.b)), vec3(encode(cs.r), encode(cs.g), encode(cs.b)));
        mixed = vec3(decode(encoded.r), decode(encoded.g), decode(encoded.b));
    } else { mixed = mix_color(mode, cb, cs); }
    return vec4(src.a * ((1.0 - dst.a) * cs + dst.a * mixed) + (1.0 - src.a) * dst.rgb, src.a + dst.a * (1.0 - src.a));
}

// Exact low/high-word arithmetic for the CPU reference's 64-bit Dissolve hash.
fn mul_high(a: u32, b: u32) -> u32 {
    let a0 = a & 65535u; let a1 = a >> 16u;
    let b0 = b & 65535u; let b1 = b >> 16u;
    let w0 = a0 * b0;
    let t = a1 * b0 + (w0 >> 16u);
    let w1 = (t & 65535u) + a0 * b1;
    return a1 * b1 + (t >> 16u) + (w1 >> 16u);
}
fn mul64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    return vec2(a.x * b.x, mul_high(a.x, b.x) + a.x * b.y + a.y * b.x);
}
fn noise(pixel: u32, seed: vec2<u32>) -> f32 {
    let x = program[4] + pixel % 256u;
    let y = program[5] + pixel / 256u;
    let xx = vec2(x, select(0u, 0xffffffffu, (x & 0x80000000u) != 0u));
    let yy = vec2(y, select(0u, 0xffffffffu, (y & 0x80000000u) != 0u));
    var h = mul64(xx, vec2(0x7f4a7c15u, 0x9e3779b9u)) ^ mul64(yy, vec2(0x27d4eb4fu, 0xc2b2ae3du)) ^ (seed ^ vec2(0u, program[6] << 24u));
    h.x ^= h.y >> 1u;
    h = mul64(h, vec2(0xed558ccdu, 0xff51afd7u));
    h.x ^= h.y >> 1u;
    return f32(h.y >> 8u) / 16777216.0;
}
fn composite(mode: u32, dst: vec4<f32>, src: vec4<f32>, pixel: u32, seed: vec2<u32>) -> vec4<f32> {
    if mode == 26u {
        if src.a <= 0.0 || noise(pixel, seed) >= src.a { return dst; }
        return vec4(src.rgb / src.a, 1.0);
    }
    return blend(mode, dst, src);
}
fn enc(c: vec3<f32>) -> vec3<f32> {
    let v = clamp(c, vec3(0.0), vec3(1.0));
    return vec3(encode(v.r), encode(v.g), encode(v.b));
}
fn dec(c: vec3<f32>) -> vec3<f32> {
    let v = clamp(c, vec3(0.0), vec3(1.0));
    return vec3(decode(v.r), decode(v.g), decode(v.b));
}
fn luma(c: vec3<f32>) -> f32 { return 0.299 * c.r + 0.587 * c.g + 0.114 * c.b; }
fn keep_luma(c: vec3<f32>, desired: f32) -> vec3<f32> {
    let l = luma(c);
    if l <= 1e-6 { return vec3(desired); }
    return clamp(c * desired / l, vec3(0.0), vec3(1.0));
}
fn rgb_hsl(c: vec3<f32>) -> vec3<f32> {
    let mx = max(c.r, max(c.g, c.b)); let mn = min(c.r, min(c.g, c.b));
    let l = (mx + mn) / 2.0;
    if mx - mn < 1e-6 { return vec3(0.0, 0.0, l); }
    let d = mx - mn;
    var s = d / (mx + mn);
    if l > 0.5 { s = d / (2.0 - mx - mn); }
    var h = (c.r - c.g) / d + 4.0;
    if mx == c.r { h = (c.g - c.b) / d + select(0.0, 6.0, c.g < c.b); }
    else if mx == c.g { h = (c.b - c.r) / d + 2.0; }
    return vec3(h / 6.0, s, l);
}
fn hsl_channel(t_in: f32, p: f32, q: f32) -> f32 {
    let t = t_in - floor(t_in);
    if t < 1.0 / 6.0 { return p + (q - p) * 6.0 * t; }
    if t < 0.5 { return q; }
    if t < 2.0 / 3.0 { return p + (q - p) * (2.0 / 3.0 - t) * 6.0; }
    return p;
}
fn hsl_rgb(h: f32, s: f32, l: f32) -> vec3<f32> {
    if s <= 0.0 { return vec3(l); }
    var q = l + s - l * s;
    if l < 0.5 { q = l * (1.0 + s); }
    let p = 2.0 * l - q;
    return vec3(hsl_channel(h + 1.0 / 3.0, p, q), hsl_channel(h, p, q), hsl_channel(h - 1.0 / 3.0, p, q));
}
fn grain_noise(x: i32, y: i32, seed: u32) -> f32 {
    var h = bitcast<u32>(x) * 0x8da6b343u ^ bitcast<u32>(y) * 0xd8255f9du ^ seed * 0x9e3779b9u;
    h ^= h >> 15u; h *= 0x2c1b3c6du; h ^= h >> 12u; h *= 0x297a2d39u; h ^= h >> 15u;
    return f32(h & 0x00ffffffu) / 16777216.0 - 0.5;
}
fn adjustment(kind: u32, data: u32, c: vec3<f32>, pixel: u32) -> vec3<f32> {
    let param = sources[data];
    if kind == 10u {
        let x = clamp(c, vec3(0.0), vec3(1.0)) * 4095.0;
        var result: vec3<f32>;
        for (var k = 0u; k < 3u; k++) {
            let i = min(u32(x[k]), 4095u);
            let a = sources[data + i][k];
            result[k] = a + (sources[data + min(i + 1u, 4095u)][k] - a) * (x[k] - f32(i));
        }
        return result;
    }
    let e = enc(c); let l = luma(e);
    switch kind {
        case 11u: { return select(vec3(0.0), vec3(1.0), l >= param.x); }
        case 12u: { return dec(sources[data + min(u32(floor(l * 255.0 + 0.5)), 255u)].rgb); }
        case 13u: {
            let hsl = rgb_hsl(e);
            let h = hsl.x + param.x - floor(hsl.x + param.x);
            var s = hsl.y * (1.0 + param.y);
            if param.y >= 0.0 { s = hsl.y + (1.0 - hsl.y) * param.y * min(sqrt(max(hsl.y, 0.0001)), 1.0); }
            var light = hsl.z * (1.0 + param.z);
            if param.z >= 0.0 { light = hsl.z + (1.0 - hsl.z) * param.z; }
            return dec(hsl_rgb(h, clamp(s, 0.0, 1.0), clamp(light, 0.0, 1.0)));
        }
        case 14u: {
            let shadow = clamp(1.0 - l * 2.0, 0.0, 1.0); let highlight = clamp(l * 2.0 - 1.0, 0.0, 1.0);
            let ws = shadow * shadow; let wh = highlight * highlight; let wm = 1.0 - ws - wh;
            var result = clamp(e + (param.rgb * ws + sources[data+1u].rgb * wm + sources[data+2u].rgb * wh) * 0.35, vec3(0.0), vec3(1.0));
            if param.w != 0.0 { result = keep_luma(result, l); }
            return dec(result);
        }
        case 15u: {
            let mx = max(e.r, max(e.g,e.b)); let mn = min(e.r,min(e.g,e.b));
            var saturation = 0.0; if mx > 1e-6 { saturation = (mx-mn)/mx; }
            let hue = rgb_hsl(e).x;
            let skin = select(1.0, 0.5, hue >= 0.02 && hue < 0.12);
            let k = max(1.0 + param.x * (1.0-saturation) * skin + param.y, 0.0);
            return dec(vec3(l) + (e-vec3(l))*k);
        }
        case 16u: {
            let mx = max(e.r,max(e.g,e.b)); let mn = min(e.r,min(e.g,e.b));
            var gray = mx;
            if mx-mn >= 1e-6 {
                let sector = rgb_hsl(e).x * 6.0;
                let i = u32(floor(sector)) % 6u; let j = (i+1u)%6u;
                let f = sector-floor(sector);
                let w = sources[data + i/4u][i%4u]*(1.0-f) + sources[data+j/4u][j%4u]*f;
                gray = clamp(mn+(mx-mn)*w,0.0,1.0);
            }
            let tint = sources[data+2u];
            return dec(vec3(gray) + (gray*tint.rgb-vec3(gray))*tint.w*0.8);
        }
        case 17u: {
            var result = e*(vec3(1.0-param.w)+param.w*param.rgb);
            if sources[data+1u].x != 0.0 { result = keep_luma(result,l); }
            return dec(result);
        }
        case 18u: {
            let x = bitcast<i32>((program[4]+pixel%256u)<<program[6]);
            let y = bitcast<i32>((program[5]+pixel/256u)<<program[6]);
            let gx = i32(floor(f32(x)/param.y)); let gy = i32(floor(f32(y)/param.y));
            let mid = 2.0*l-1.0; let visibility = 1.0-mid*mid*0.6;
            var result: vec3<f32>;
            for (var k=0u;k<3u;k++) { result[k]=e[k]+grain_noise(gx,gy,select(11u+k,11u,param.z!=0.0))*param.x*0.5*visibility; }
            return dec(result);
        }
        case 19u: {
            let x = bitcast<i32>((program[4]+pixel%256u)<<program[6]); let y = bitcast<i32>((program[5]+pixel/256u)<<program[6]);
            let fw=f32(max(program[7],1u)); let fh=f32(max(program[8],1u));
            let nx=(f32(x)+0.5)/fw*2.0-1.0; let ny=(f32(y)+0.5)/fh*2.0-1.0;
            let re=sqrt(nx*nx+ny*ny)/1.4142135623730951;
            let rc=sqrt((nx*fw)*(nx*fw)+(ny*fh)*(ny*fh))/min(fw,fh)/1.4142135623730951;
            let r=re+(rc-re)*param.w; let t=clamp((r-param.y)/param.z,0.0,1.0); let k=t*t*(3.0-2.0*t);
            let f=1.0-param.x*k*select(0.6,0.92,param.x>=0.0);
            return clamp(c*f,vec3(0.0),vec3(1.0));
        }
        case 20u: {
            let n=u32(param.x); let f=e*f32(n-1u); let lo=vec3<u32>(floor(f)); let hi=min(lo+vec3(1u),vec3(n-1u)); let t=f-vec3<f32>(lo);
            var result=vec3(0.0);
            for(var r=0u;r<2u;r++){for(var g=0u;g<2u;g++){for(var b=0u;b<2u;b++){
                let rr=select(lo.r,hi.r,r==1u);let gg=select(lo.g,hi.g,g==1u);let bb=select(lo.b,hi.b,b==1u);
                let weight=select(1.0-t.r,t.r,r==1u)*select(1.0-t.g,t.g,g==1u)*select(1.0-t.b,t.b,b==1u);
                if weight>0.0 { result+=sources[data+1u+rr+gg*n+bb*n*n].rgb*weight; }
            }}}
            return dec(e+(result-e)*param.y);
        }
        default: { return c; }
    }
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) invocation: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let pixel = invocation.x + invocation.y * groups.x * 64u;
    if pixel >= 65536u { return; }
    if pixel % 256u >= program[2] || pixel / 256u >= program[3] { output[pixel] = vec4(0.0); return; }
    var acc = vec4(0.0);
    var stack: array<vec4<f32>, 16>;
    var alpha: array<f32, 64>;
    for (var slot = 0u; slot < 64u; slot++) { alpha[slot] = 1.0; }
    var depth = 0u;
    var adjustment_before = vec4(0.0);
    var adjusted = vec3(0.0);
    for (var i = 0u; i < program[0]; i++) {
        let offset = 12u + i * 8u;
        let op = program[offset];
        if op == 5u {
            adjustment_before = acc;
            adjusted = vec3(0.0);
            if acc.a > 0.0 { adjusted = acc.rgb / acc.a; }
            continue;
        }
        if op >= 10u {
            if adjustment_before.a > 0.0 { adjusted = adjustment(op, program[offset + 4u], adjusted, pixel); }
            continue;
        }
        if op == 1u || op == 2u {
            stack[depth] = acc;
            depth++;
            if op == 1u { acc = vec4(0.0); }
            continue;
        }
        let mode = program[offset + 1u];
        let slot = program[offset + 2u];
        let clip = program[offset + 3u];
        let source = program[offset + 4u];
        var coverage = bitcast<f32>(program[offset + 5u]);
        if clip != 0xffffffffu { coverage *= alpha[clip]; }
        if op == 6u {
            if source != 0xffffffffu { coverage *= sources[source + pixel].a; }
            if adjustment_before.a > 0.0 && coverage > 0.0 {
                let rgb = adjustment_before.rgb / adjustment_before.a;
                var desired = adjusted;
                if mode != 0u && mode != 26u { desired = blend(mode, vec4(clamp(rgb,vec3(0.0),vec3(1.0)),1.0), vec4(clamp(adjusted,vec3(0.0),vec3(1.0)),1.0)).rgb; }
                acc = vec4((rgb+(desired-rgb)*coverage)*adjustment_before.a, adjustment_before.a);
            }
            continue;
        }
        let seed = vec2(program[offset+6u],program[offset+7u]);
        if op == 0u {
            let src = sources[source + pixel];
            alpha[slot] = src.a;
            if coverage > 0.0 { acc = composite(mode, acc, src * coverage, pixel, seed); }
        } else {
            depth--;
            var mask = 1.0;
            if source != 0xffffffffu { mask = sources[source + pixel].a; }
            if op == 4u {
                acc = stack[depth] + (acc - stack[depth]) * (coverage * mask);
            } else {
                let src = acc * mask;
                alpha[slot] = src.a;
                acc = stack[depth];
                if coverage > 0.0 { acc = composite(mode, acc, src * coverage, pixel, seed); }
            }
        }
    }
    output[pixel] = acc;
}
