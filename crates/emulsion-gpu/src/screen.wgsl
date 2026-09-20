@group(0) @binding(0) var<storage, read> params: array<u32>;
@group(0) @binding(1) var<storage, read> matrix: array<f32>;
@group(0) @binding(2) var<storage, read> grid: array<u32>;
@group(0) @binding(3) var<storage, read> pixels: array<u32>;
@group(0) @binding(4) var<storage, read_write> output: array<vec2<u32>>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = id.x + id.y * groups.x * 64u;
    if i >= params[0] * params[1] { return; }
    let x = i % params[0]; let y = i / params[0];
    let base = vec2<f32>(matrix[0], matrix[1]);
    let dx = vec2<f32>(matrix[2], matrix[3]) * f32(x);
    let dy = vec2<f32>(matrix[4], matrix[5]) * f32(y);
    let source = base + dx + dy;
    // Conservative error bound for f64 -> f32 coefficients and affine math.
    // Correct only ambiguous nearest-neighbor edges with the CPU reference.
    let epsilon = (abs(base) + abs(dx) + abs(dy)) * 0.000001 + vec2<f32>(0.00001);
    let fraction = fract(source);
    if any(fraction <= epsilon) || any(vec2<f32>(1.0) - fraction <= epsilon) {
        output[i] = vec2<u32>(0u, 1u); return;
    }
    output[i] = vec2<u32>(0u, 0u);
    if any(source < vec2<f32>(0.0)) || any(source >= vec2<f32>(f32(params[2]), f32(params[3]))) { return; }
    let p = vec2<u32>(floor(source));
    let tile = vec2<i32>(p / 256u) - vec2<i32>(bitcast<i32>(params[4]), bitcast<i32>(params[5]));
    if any(tile < vec2<i32>(0)) || any(tile >= vec2<i32>(i32(params[6]), i32(params[7]))) { return; }
    let cell = u32(tile.y) * params[6] + u32(tile.x) + select(0u, params[6] * params[7], x < params[8]);
    let offset = grid[cell];
    if offset == 0xffffffffu { return; }
    output[i].x = pixels[offset + (p.y % 256u) * 256u + (p.x % 256u)];
}
