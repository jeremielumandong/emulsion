// Original Emulsion implementation of persistent dry-brush accumulation.
struct Dab { geometry: vec4<f32>, color: vec4<f32>, flow: vec4<f32> }
@group(0) @binding(0) var<storage, read> params: array<u32>;
@group(0) @binding(1) var<storage, read> base: array<vec2<u32>>;
@group(0) @binding(2) var<storage, read> dabs: array<Dab>;
@group(0) @binding(3) var<storage, read_write> accumulation: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> output: array<vec2<u32>>;
fn falloff(d: f32, hardness: f32) -> f32 {
    if d >= 1.0 { return 0.0; }
    let h = min(hardness, 0.99);
    if d <= h { return 1.0; }
    let t = (d-h)/(1.0-h);
    return 1.0-t*t*(3.0-2.0*t);
}
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params[0]*params[1] { return; }
    let point = vec2<f32>(f32(i%params[0])+0.5,f32(i/params[0])+0.5);
    var ink = accumulation[i];
    for (var j=0u; j<params[2]; j++) {
        let dab = dabs[j];
        let offset = point-dab.geometry.xy;
        let r = dab.geometry.z;
        if abs(offset.x)>r+1.0 || abs(offset.y)>r+1.0 { continue; }
        let d = length(offset)/r;
        let hard = dab.geometry.w;
        let footprint = 0.7071067811865476/r;
        var shape = falloff(d,hard);
        if ((1.0-hard)*r<1.0 || r<2.0) && d+footprint>hard && d-footprint<1.0 {
            shape = 0.0;
            for(var sy=0u;sy<4u;sy++) {
                for(var sx=0u;sx<4u;sx++) {
                    let sample = vec2<f32>(f32(sx)*0.25-0.375,f32(sy)*0.25-0.375);
                    shape += falloff(length(offset+sample)/r,hard);
                }
            }
            shape /= 16.0;
        }
        let a = shape*dab.flow.x;
        if a>0.0005 { ink = dab.color*a+ink*(1.0-a); }
    }
    accumulation[i] = ink;
    let packed = base[i];
    let b = vec4<f32>(f32(packed.x&65535u),f32(packed.x>>16u),f32(packed.y&65535u),f32(packed.y>>16u))*(1.0/65535.0);
    let source = ink*bitcast<f32>(params[3]);
    let result = source+b*(1.0-source.a);
    let rgba = vec4<u32>(clamp(result,vec4<f32>(0.0),vec4<f32>(1.0))*65535.0+0.5);
    output[i] = vec2<u32>(rgba.r|(rgba.g<<16u),rgba.b|(rgba.a<<16u));
}
