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

// A `PageTone`: seven 3x4 matrices, the paper, then count and blend.
struct Tone {
    rows: array<vec4<f32>, 21>,
    paper: vec4<f32>,
    mode: vec4<u32>,
};
@group(1) @binding(0) var<uniform> tone: Tone;

// CSS filter functions, clamping after each, in sRGB terms.
fn filter_chain(c: vec3<f32>) -> vec3<f32> {
    var out = c;
    for (var i = 0u; i < min(tone.mode.x, 7u); i++) {
        let r = tone.rows[i * 3u];
        let g = tone.rows[i * 3u + 1u];
        let b = tone.rows[i * 3u + 2u];
        let m = vec3<f32>(dot(r.xyz, out) + r.w, dot(g.xyz, out) + g.w, dot(b.xyz, out) + b.w);
        out = clamp(m, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return out;
}

fn soft_light(b: f32, s: f32) -> f32 {
    if (s <= 0.5) {
        return b - (1.0 - 2.0 * s) * b * (1.0 - b);
    }
    var d = sqrt(b);
    if (b <= 0.25) {
        d = ((16.0 * b - 12.0) * b + 4.0) * b;
    }
    return b + (2.0 * s - 1.0) * (d - b);
}

// W3C blend modes: 1 multiply, 2 screen, 3 soft light.
fn blend(b: vec3<f32>, s: vec3<f32>) -> vec3<f32> {
    switch tone.mode.y {
        case 1u: { return b * s; }
        case 2u: { return b + s - b * s; }
        default: {
            return vec3<f32>(soft_light(b.r, s.r), soft_light(b.g, s.g), soft_light(b.b, s.b));
        }
    }
}

// The page filtered, then blended over opaque paper.
fn themed(c: vec4<f32>) -> vec4<f32> {
    if (tone.mode.y == 0u) {
        return c;
    }
    let paper = tone.paper.rgb;
    let s = filter_chain(c.rgb / max(c.a, 1e-5));
    return vec4<f32>(mix(paper, blend(paper, s), c.a), 1.0);
}

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
    return to_target(themed(textureSample(page, filtered, in.uv)) * in.tint);
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
    return to_target(themed(sum / 16.0) * in.tint);
}
