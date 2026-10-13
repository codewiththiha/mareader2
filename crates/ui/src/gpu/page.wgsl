// One tinted quad per page or fill. `LINEAR` is prepended:
// true when the target encodes sRGB itself.

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
};

// `rect` is (left, top, right, bottom) in clip space.
@vertex
fn vs(
    @builtin(vertex_index) i: u32,
    @location(0) rect: vec4<f32>,
    @location(1) tint: vec4<f32>,
) -> VsOut {
    let corner = vec2<f32>(f32(i & 1u), f32(i >> 1u));
    var out: VsOut;
    out.pos = vec4<f32>(mix(rect.xy, rect.zw, corner), 0.0, 1.0);
    out.uv = corner;
    out.tint = tint;
    return out;
}

@group(0) @binding(0) var page: texture_2d<f32>;
@group(0) @binding(1) var filtered: sampler;

// Rasters hold gamma-encoded, premultiplied colour.
fn to_target(c: vec4<f32>) -> vec4<f32> {
    if (!LINEAR || c.a <= 0.0) {
        return c;
    }
    let rgb = c.rgb / c.a;
    let low = rgb / 12.92;
    let high = pow((rgb + 0.055) / 1.055, vec3<f32>(2.4));
    return vec4<f32>(select(high, low, rgb <= vec3<f32>(0.04045)) * c.a, c.a);
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return to_target(textureSample(page, filtered, in.uv) * in.tint);
}

// For the shrunk glass strip: a 4x4 average per pixel
// stops scrolled text shimmering.
@fragment
fn fs_strip(in: VsOut) -> @location(0) vec4<f32> {
    let step = fwidth(in.uv) / 4.0;
    var sum = vec4<f32>(0.0);
    for (var y = 0; y < 4; y++) {
        for (var x = 0; x < 4; x++) {
            let at = in.uv + (vec2<f32>(f32(x), f32(y)) - 1.5) * step;
            sum += textureSampleLevel(page, filtered, at, 0.0);
        }
    }
    return to_target(sum / 16.0 * in.tint);
}
