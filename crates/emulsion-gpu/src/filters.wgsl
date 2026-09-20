@group(0) @binding(0) var<storage, read> pixels: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> auxiliary: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> params: array<u32>;
@group(0) @binding(3) var<storage, read> kernel: array<f32>;
@group(0) @binding(4) var<storage, read_write> output: array<vec4<f32>>;

fn pixel(x: i32, y: i32) -> vec4<f32> {
    if x < 0 || y < 0 || x >= i32(params[0]) || y >= i32(params[1]) {
        return vec4<f32>(0.0);
    }
    return pixels[u32(y) * params[0] + u32(x)];
}

fn luma(p: vec4<f32>) -> f32 {
    if p.a <= 0.000001 { return 0.0; }
    return dot(p.rgb / p.a, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 { return c * 12.92; }
    return 1.055 * pow(c, 1.0 / 2.4) - 0.055;
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { return c / 12.92; }
    return pow((c + 0.055) / 1.055, 2.4);
}

fn sample_at(position: vec2<f32>) -> vec4<f32> {
    let f = position - vec2<f32>(0.5);
    let base = vec2<i32>(floor(f));
    let t = f - floor(f);
    return (pixel(base.x, base.y) * (1.0-t.x) + pixel(base.x+1, base.y) * t.x) * (1.0-t.y)
        + (pixel(base.x, base.y+1) * (1.0-t.x) + pixel(base.x+1, base.y+1) * t.x) * t.y;
}

fn noise_hash(x: u32, y: u32, seed: u32) -> f32 {
    var h = x * 0x8da6b343u ^ y * 0xd8255f9du ^ seed * 0x9e3779b9u;
    h ^= h >> 15u;
    h *= 0x2c1b3c6du;
    h ^= h >> 12u;
    h *= 0x297a2d39u;
    h ^= h >> 15u;
    return f32(h & 0x00ffffffu) / 16777216.0 - 0.5;
}

fn gain_color(p: vec4<f32>, gain: f32) -> vec4<f32> {
    if p.a <= 0.000001 { return p; }
    return vec4<f32>(clamp(p.rgb / p.a * gain, vec3<f32>(0.0), vec3<f32>(1.0)) * p.a, p.a);
}

// Native GPU trig may use reduced-precision approximations. Small errors in
// source coordinates become visible after bilinear sampling of sharp alpha
// edges. Evaluate a range-reduced polynomial for the distortion coordinates.
fn coordinate_sin_cos(angle: f32) -> vec2<f32> {
    let turns = floor(angle / 6.283185307179586 + 0.5);
    // Split tau so reducing large Wave arguments does not cancel low bits.
    var x = (angle - turns * 6.28125) - turns * 0.001935307179586477;
    var cosine_sign = 1.0;
    if x > 1.5707963267948966 {
        x = 3.141592653589793 - x;
        cosine_sign = -1.0;
    } else if x < -1.5707963267948966 {
        x = -3.141592653589793 - x;
        cosine_sign = -1.0;
    }
    let z = x * x;
    let s = x * (1.0 + z * (-1.0/6.0 + z * (1.0/120.0 + z * (-1.0/5040.0
        + z * (1.0/362880.0 + z * (-1.0/39916800.0))))));
    let c = 1.0 + z * (-1.0/2.0 + z * (1.0/24.0 + z * (-1.0/720.0
        + z * (1.0/40320.0 + z * (-1.0/3628800.0 + z * (1.0/479001600.0))))));
    return vec2<f32>(s, c * cosine_sign);
}

fn effect(mode: u32, x: i32, y: i32, p: vec4<f32>) -> vec4<f32> {
    let position = vec2<f32>(f32(x)+0.5, f32(y)+0.5);
    let center = vec2<f32>(f32(params[0]), f32(params[1])) / 2.0;
    if mode == 6u {
        var sum = vec4<f32>(0.0);
        let n = i32(kernel[0]);
        for (var k=0; k<n; k+=1) {
            let t = f32(k) - f32(n-1)/2.0;
            sum += sample_at(position + vec2<f32>(kernel[1]*t, -kernel[2]*t)) / f32(n);
        }
        return sum;
    }
    if mode == 7u {
        let radius = i32(kernel[0]);
        var sum = vec4<f32>(0.0);
        for (var dy = -radius; dy <= radius; dy+=1) {
            for (var dx = -radius; dx <= radius; dx+=1) {
                if dx*dx + dy*dy <= radius*radius {
                    sum += pixel(x+dx,y+dy) * kernel[1];
                }
            }
        }
        return sum;
    }
    if mode >= 11u {
        let delta = position-center;
        let rmax = max(min(center.x,center.y),1.0);
        let radius = length(delta)/rmax;
        var sample_position = position;
        if mode == 11u && radius < 1.0 && radius > 0.0 {
            sample_position = center + delta * (pow(radius,1.0+kernel[0]*0.9)/radius);
        } else if mode == 12u && radius < 1.0 {
            let t = kernel[0]*(1.0-radius)*(1.0-radius);
            let sc = coordinate_sin_cos(t);
            sample_position = center + vec2<f32>(delta.x*sc.y-delta.y*sc.x, delta.x*sc.x+delta.y*sc.y);
        } else if mode == 13u {
            let sy = coordinate_sin_cos(position.y/kernel[1]*6.283185307179586).x;
            let cx = coordinate_sin_cos(position.x/kernel[1]*6.283185307179586).y;
            sample_position = position + vec2<f32>(sy*kernel[0], cx*kernel[0]*0.5);
        } else if mode == 14u {
            let unit = max(length(center),1.0);
            let normalized = delta/unit;
            let factor = 1.0+kernel[0]*dot(normalized,normalized);
            sample_position = center + normalized*factor*unit;
            let warped = sample_at(sample_position);
            if kernel[1] == 0.0 { return warped; }
            let r = length(normalized);
            return gain_color(warped, clamp(1.0-kernel[1]*r*r*1.5,0.0,2.0));
        } else if mode == 15u {
            let unit = max(min(center.x,center.y),1.0);
            let ru = length(delta)/unit*kernel[6];
            if ru > 0.000001 && (kernel[0] != 0.0 || kernel[1] != 0.0 || kernel[2] != 0.0) {
                let d = 1.0-kernel[0]-kernel[1]-kernel[2];
                let rd = ru*(kernel[0]*ru*ru*ru + kernel[1]*ru*ru + kernel[2]*ru + d);
                sample_position = center + delta*(rd/ru);
            }
            let warped = sample_at(sample_position);
            if kernel[3] == 0.0 && kernel[4] == 0.0 && kernel[5] == 0.0 { return warped; }
            let r2 = dot(delta,delta)/(unit*unit)*kernel[6]*kernel[6];
            let cd = 1.0+kernel[3]*r2+kernel[4]*r2*r2+kernel[5]*r2*r2*r2;
            return gain_color(warped,clamp(1.0/max(cd,0.05),0.2,4.0));
        }
        return sample_at(sample_position);
    }
    if p.a <= 0.000001 { return p; }
    if mode == 8u {
        var out = p;
        for (var ch=0u; ch<3u; ch+=1u) {
            var seed = 1u;
            if kernel[1] == 0.0 { seed += ch; }
            let e = linear_to_srgb(p[ch]/p.a) + noise_hash(u32(x),u32(y),seed)*kernel[0];
            out[ch] = srgb_to_linear(clamp(e,0.0,1.0))*p.a;
        }
        return out;
    }
    if mode == 9u {
        if kernel[1] <= 0.0 { return p; }
        var sum = vec4<f32>(0.0);
        var weights = 0.0;
        for (var dy = -2; dy <= 2; dy+=1) {
            for (var dx = -2; dx <= 2; dx+=1) {
                let other = pixel(x+dx,y+dy);
                let dist = length(other.rgb-p.rgb);
                let weight = exp(-(dist*dist)/(2.0*kernel[0]*kernel[0])) * exp(-f32(dx*dx+dy*dy)/6.0);
                sum += other * weight;
                weights += weight;
            }
        }
        return sum / max(weights,0.000001);
    }
    if mode == 10u {
        let delta = vec2<f32>(kernel[0],kernel[1]);
        let difference = (luma(sample_at(position+delta))-luma(sample_at(position-delta)))*kernel[2];
        let e = srgb_to_linear(clamp(0.5+difference,0.0,1.0))*p.a;
        return vec4<f32>(e,e,e,p.a);
    }
    return p;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = id.x + id.y * groups.x * 64u;
    if i >= params[0] * params[1] { return; }
    let x = i32(i % params[0]);
    let y = i32(i / params[0]);
    let mode = params[2];
    if mode <= 1u {
        let radius = i32(params[3] / 2u);
        var sum = vec4<f32>(0.0);
        for (var k = 0u; k < params[3]; k += 1u) {
            let offset = i32(k) - radius;
            var p: vec4<f32>;
            if mode == 0u { p = pixel(x + offset, y); }
            else { p = pixel(x, y + offset); }
            sum += p * kernel[k];
        }
        output[i] = sum;
        return;
    }
    let p = pixels[i];
    if mode >= 6u {
        output[i] = effect(mode,x,y,p);
        return;
    }
    if p.a <= 0.000001 {
        output[i] = p;
        return;
    }
    if mode == 5u {
        let gx = luma(pixel(x+1,y-1)) + 2.0*luma(pixel(x+1,y)) + luma(pixel(x+1,y+1))
            - luma(pixel(x-1,y-1)) - 2.0*luma(pixel(x-1,y)) - luma(pixel(x-1,y+1));
        let gy = luma(pixel(x-1,y+1)) + 2.0*luma(pixel(x,y+1)) + luma(pixel(x+1,y+1))
            - luma(pixel(x-1,y-1)) - 2.0*luma(pixel(x,y-1)) - luma(pixel(x+1,y-1));
        let e = srgb_to_linear(1.0 - min(sqrt(gx*gx + gy*gy), 1.0)) * p.a;
        output[i] = vec4<f32>(e, e, e, p.a);
        return;
    }
    let b = auxiliary[i];
    var out = p;
    if mode == 4u {
        for (var ch = 0u; ch < 3u; ch += 1u) {
            let e = linear_to_srgb(clamp(p[ch] / p.a, 0.0, 1.0))
                - linear_to_srgb(clamp(b[ch] / max(b.a, 0.000001), 0.0, 1.0)) + 0.5;
            out[ch] = srgb_to_linear(clamp(e, 0.0, 1.0)) * p.a;
        }
    } else if abs(luma(p) - luma(b)) >= bitcast<f32>(params[5]) {
        for (var ch = 0u; ch < 3u; ch += 1u) {
            let difference = p[ch] - b[ch] * (p.a / max(b.a, 0.000001));
            var gain = bitcast<f32>(params[4]);
            if mode == 3u { gain = gain / (1.0 + abs(difference) * 6.0); }
            out[ch] = clamp(p[ch] + difference * gain, 0.0, p.a);
        }
    }
    output[i] = out;
}
