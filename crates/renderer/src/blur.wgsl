// Dual-Kawase Multi-Pass Backdrop Blur Shader (5-tap downsample + 8-tap tent upsample).

@group(0) @binding(0)
var t_src: texture_2d<f32>;
@group(0) @binding(1)
var s_blur: sampler;
@group(0) @binding(2)
var<uniform> u_blur: vec4<f32>; // (texel_w, texel_h, offset_scale, 0.0)

struct BlurVaryings {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_blur(@builtin(vertex_index) vid: u32) -> BlurVaryings {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: BlurVaryings;
    out.clip_position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

@fragment
fn fs_downsample(in: BlurVaryings) -> @location(0) vec4<f32> {
    let hp = 0.5 * u_blur.xy * u_blur.z;
    let c = textureSampleLevel(t_src, s_blur, in.uv, 0.0) * 4.0;
    let s0 = textureSampleLevel(t_src, s_blur, in.uv - hp, 0.0);
    let s1 = textureSampleLevel(t_src, s_blur, in.uv + hp, 0.0);
    let s2 = textureSampleLevel(t_src, s_blur, in.uv + vec2<f32>(hp.x, -hp.y), 0.0);
    let s3 = textureSampleLevel(t_src, s_blur, in.uv - vec2<f32>(hp.x, -hp.y), 0.0);
    return (c + s0 + s1 + s2 + s3) * 0.125;
}

@fragment
fn fs_upsample(in: BlurVaryings) -> @location(0) vec4<f32> {
    let hp = 0.5 * u_blur.xy * u_blur.z;
    var sum = textureSampleLevel(t_src, s_blur, in.uv + vec2<f32>(-hp.x * 2.0, 0.0), 0.0);
    sum += textureSampleLevel(t_src, s_blur, in.uv + vec2<f32>(-hp.x, hp.y), 0.0) * 2.0;
    sum += textureSampleLevel(t_src, s_blur, in.uv + vec2<f32>(0.0, hp.y * 2.0), 0.0);
    sum += textureSampleLevel(t_src, s_blur, in.uv + vec2<f32>(hp.x, hp.y), 0.0) * 2.0;
    sum += textureSampleLevel(t_src, s_blur, in.uv + vec2<f32>(hp.x * 2.0, 0.0), 0.0);
    sum += textureSampleLevel(t_src, s_blur, in.uv + vec2<f32>(hp.x, -hp.y), 0.0) * 2.0;
    sum += textureSampleLevel(t_src, s_blur, in.uv + vec2<f32>(0.0, -hp.y * 2.0), 0.0);
    sum += textureSampleLevel(t_src, s_blur, in.uv + vec2<f32>(-hp.x, -hp.y), 0.0) * 2.0;
    return sum / 12.0;
}
