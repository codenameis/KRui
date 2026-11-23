// All color outputs are premultiplied RGBA (`BlendState::PREMULTIPLIED_ALPHA_BLENDING`).

@group(0) @binding(0)
var t_mask_atlas: texture_2d_array<f32>;
@group(0) @binding(1)
var t_color_atlas: texture_2d_array<f32>;
@group(0) @binding(2)
var s_linear: sampler;

@group(1) @binding(0)
var t_aux: texture_2d<f32>;

const KIND_STYLED_BOX: u32 = 0u;
const KIND_OUTER_SHADOW: u32 = 1u;
const KIND_INSET_SHADOW: u32 = 2u;
const KIND_MASK_GLYPH: u32 = 3u;
const KIND_COLOR_GLYPH: u32 = 4u;
const KIND_SAMPLE_AUX_TEX: u32 = 5u;
const KIND_IMAGE_TEX: u32 = 6u;

struct SdfInstanceInput {
    @builtin(vertex_index) vertex_idx: u32,
    @location(0) screen_bounds: vec4<f32>,
    @location(1) center_and_vp: vec4<f32>,
    @location(2) inv_linear: vec4<f32>,
    @location(3) box_geom: vec4<f32>,
    @location(4) clip_geom: vec4<f32>,
    @location(5) color_a: vec4<f32>,
    @location(6) color_b: vec4<f32>,
    @location(7) grad_start: vec4<f32>,
    @location(8) grad_end: vec4<f32>,
    @location(9) uv_rect: vec4<f32>,
    @location(10) shadow_params: vec4<f32>,
    @location(11) extra_params: vec4<f32>,
    @location(12) flags: vec4<u32>,
    @location(13) corner_radii: vec4<f32>,
    @location(14) clip_radii: vec4<f32>,
};

struct SdfVaryings {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) pos_and_local: vec4<f32>,
    @location(1) uv_and_scale: vec4<f32>,
    @location(2) box_geom: vec4<f32>,
    @location(3) clip_geom: vec4<f32>,
    @location(4) color_a: vec4<f32>,
    @location(5) color_b: vec4<f32>,
    @location(6) grad_start: vec4<f32>,
    @location(7) grad_end: vec4<f32>,
    @location(8) uv_bounds: vec4<f32>,
    @location(9) shadow_params: vec4<f32>,
    @location(10) extra: vec4<f32>,
    @location(11) @interpolate(flat) flags: vec4<u32>,
    @location(12) corner_radii: vec4<f32>,
    @location(13) clip_radii: vec4<f32>,
};

fn quad_corner(vid: u32) -> vec2<f32> {
    switch vid {
        case 0u: { return vec2<f32>(0.0, 0.0); }
        case 1u: { return vec2<f32>(1.0, 0.0); }
        case 2u: { return vec2<f32>(1.0, 1.0); }
        case 3u: { return vec2<f32>(0.0, 0.0); }
        case 4u: { return vec2<f32>(1.0, 1.0); }
        default: { return vec2<f32>(0.0, 1.0); }
    }
}

@vertex
fn vs_main(in: SdfInstanceInput) -> SdfVaryings {
    let corner = quad_corner(in.vertex_idx);
    let screen_pos = mix(in.screen_bounds.xy, in.screen_bounds.zw, corner);
    let ndc = vec2<f32>(
        screen_pos.x * in.center_and_vp.z * 2.0 - 1.0,
        1.0 - screen_pos.y * in.center_and_vp.w * 2.0,
    );
    let d_screen = screen_pos - in.center_and_vp.xy;
    let local_p = vec2<f32>(
        in.inv_linear.x * d_screen.x + in.inv_linear.z * d_screen.y,
        in.inv_linear.y * d_screen.x + in.inv_linear.w * d_screen.y,
    );
    let px_scale = 0.5 * (
        length(vec2<f32>(in.inv_linear.x, in.inv_linear.y)) +
        length(vec2<f32>(in.inv_linear.z, in.inv_linear.w))
    );
    let aux_uv = screen_pos * in.center_and_vp.zw;
    let angle = in.extra_params.z;
    let grad_dir = vec2<f32>(sin(angle), -cos(angle));

    var out: SdfVaryings;
    out.clip_position = vec4<f32>(ndc, 0.0, 1.0);
    out.pos_and_local = vec4<f32>(screen_pos, local_p);
    out.uv_and_scale = vec4<f32>(aux_uv, max(px_scale, 1e-4), in.extra_params.x);
    out.box_geom = in.box_geom;
    out.clip_geom = in.clip_geom;
    out.color_a = in.color_a;
    out.color_b = in.color_b;
    out.grad_start = in.grad_start;
    out.grad_end = in.grad_end;
    out.uv_bounds = in.uv_rect;
    if in.flags.x == KIND_IMAGE_TEX {
        out.shadow_params = in.inv_linear;
    } else {
        out.shadow_params = in.shadow_params;
    }
    if in.flags.z == 3u {
        out.extra = in.extra_params;
    } else {
        out.extra = vec4<f32>(in.extra_params.y, grad_dir.x, grad_dir.y, in.extra_params.w);
    }
    out.flags = in.flags;
    out.corner_radii = in.corner_radii;
    out.clip_radii = in.clip_radii;
    return out;
}

fn sd_round_rect(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let r = clamp(radius, 0.0, min(half_size.x, half_size.y));
    let q = abs(p) - (half_size - vec2<f32>(r, r));
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn sd_round_rect_4(p: vec2<f32>, half_size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let w = max(2.0 * half_size.x, 0.0);
    let h = max(2.0 * half_size.y, 0.0);
    let r0 = max(radii, vec4<f32>(0.0));
    let f = min(
        1.0,
        min(
            min(w / max(r0.x + r0.y, 1e-4), w / max(r0.w + r0.z, 1e-4)),
            min(h / max(r0.x + r0.w, 1e-4), h / max(r0.y + r0.z, 1e-4)),
        ),
    );
    let nr = r0 * f;
    var r_sel = nr.x;
    if p.x > 0.0 {
        if p.y > 0.0 {
            r_sel = nr.z;
        } else {
            r_sel = nr.y;
        }
    } else if p.y > 0.0 {
        r_sel = nr.w;
    }
    let r = clamp(r_sel, 0.0, min(half_size.x, half_size.y));
    let q = abs(p) - (half_size - vec2<f32>(r, r));
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn erf_approx(x: f32) -> f32 {
    let s = sign(x);
    let a = abs(x);
    let t = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    let t2 = t * t;
    return s - s / (t2 * t2);
}

fn to_premul(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(c.rgb * c.a, c.a);
}

fn over_premul(src_pm: vec4<f32>, dst_pm: vec4<f32>) -> vec4<f32> {
    return src_pm + dst_pm * (1.0 - src_pm.a);
}

fn eval_clip_coverage(screen_pos: vec2<f32>, clip_geom: vec4<f32>, clip_radii: vec4<f32>) -> f32 {
    if clip_geom.z < 0.0 {
        return 1.0;
    }
    let d_clip = sd_round_rect_4(screen_pos - clip_geom.xy, clip_geom.zw, clip_radii);
    return smoothstep(0.5, -0.5, d_clip);
}

fn eval_styled_box(in: SdfVaryings, local_p: vec2<f32>, aa: f32) -> vec4<f32> {
    let half_size = in.box_geom.xy;
    let bw = in.box_geom.w;
    let d_outer = sd_round_rect_4(local_p, half_size, in.corner_radii);
    let outer_cov = smoothstep(aa, -aa, d_outer);
    if outer_cov <= 0.0 {
        return vec4<f32>(0.0);
    }

    var fill_pm = to_premul(in.color_a);
    if in.flags.z == 3u {
        let d = (local_p - in.uv_bounds.xy) / max(in.uv_bounds.zw, vec2<f32>(1e-3, 1e-3));
        let dist = length(d);
        let p0 = in.extra.z;
        let p1 = in.extra.w;
        let span = max(p1 - p0, 1e-4);
        let t = clamp((dist - p0) / span, 0.0, 1.0);
        let grad_pm = mix(to_premul(in.grad_start), to_premul(in.grad_end), t);
        fill_pm = over_premul(grad_pm, fill_pm);
    } else if in.flags.z != 0u {
        let dir = in.extra.yz;
        let half_proj = max(abs(dir.x) * half_size.x + abs(dir.y) * half_size.y, 1e-4);
        let t = clamp(0.5 + dot(local_p, dir) / (2.0 * half_proj), 0.0, 1.0);
        let p0 = in.uv_bounds.x;
        let p1 = in.uv_bounds.y;
        let p2 = in.uv_bounds.z;
        let c0 = to_premul(in.grad_start);
        let c1 = to_premul(in.grad_end);
        var grad_pm = mix(c0, c1, clamp((t - p0) / max(p2 - p0, 1e-4), 0.0, 1.0));
        if in.flags.z == 2u {
            let cm = to_premul(in.shadow_params);
            if t <= p1 {
                grad_pm = mix(c0, cm, clamp((t - p0) / max(p1 - p0, 1e-4), 0.0, 1.0));
            } else {
                grad_pm = mix(cm, c1, clamp((t - p1) / max(p2 - p1, 1e-4), 0.0, 1.0));
            }
        }
        fill_pm = over_premul(grad_pm, fill_pm);
    }

    var box_pm = fill_pm;
    if bw > 0.0 && in.color_b.a > 0.0 {
        let border_pm = to_premul(in.color_b);
        if bw >= min(half_size.x, half_size.y) {
            box_pm = border_pm;
        } else {
            let inner_half = half_size - vec2<f32>(bw, bw);
            let inner_r = max(in.corner_radii - vec4<f32>(bw), vec4<f32>(0.0));
            let d_inner = sd_round_rect_4(local_p, inner_half, inner_r);
            let inner_cov = smoothstep(aa, -aa, d_inner);
            box_pm = mix(border_pm, fill_pm, inner_cov);
        }
    }
    return box_pm * outer_cov;
}

fn eval_outer_shadow(in: SdfVaryings, local_p: vec2<f32>, aa: f32) -> vec4<f32> {
    let half_size = in.box_geom.xy;
    let d_box = sd_round_rect_4(local_p, half_size, in.corner_radii);
    let outside_mask = smoothstep(-aa, aa, d_box);
    if outside_mask <= 0.0 {
        return vec4<f32>(0.0);
    }
    let spread = in.shadow_params.w;
    let sh_half = max(half_size + vec2<f32>(spread, spread), vec2<f32>(0.0, 0.0));
    let sh_r = max(in.corner_radii + vec4<f32>(spread), vec4<f32>(0.0));
    let d_sh = sd_round_rect_4(local_p - in.shadow_params.xy, sh_half, sh_r);
    let blur = in.shadow_params.z;
    var sh_cov = smoothstep(aa, -aa, d_sh);
    if blur > 0.25 {
        let inv_denom = 1.0 / max(blur * 0.70710678, 0.35);
        sh_cov = 0.5 - 0.5 * erf_approx(d_sh * inv_denom);
    }
    return to_premul(in.color_a) * (sh_cov * outside_mask);
}

fn eval_inset_shadow(in: SdfVaryings, local_p: vec2<f32>, aa: f32) -> vec4<f32> {
    let bw = in.box_geom.w;
    let pad_half = max(in.box_geom.xy - vec2<f32>(bw, bw), vec2<f32>(0.0, 0.0));
    let pad_r = max(in.corner_radii - vec4<f32>(bw), vec4<f32>(0.0));
    let d_pad = sd_round_rect_4(local_p, pad_half, pad_r);
    let inside_mask = smoothstep(aa, -aa, d_pad);
    if inside_mask <= 0.0 {
        return vec4<f32>(0.0);
    }
    let spread = in.shadow_params.w;
    let cut_half = max(pad_half - vec2<f32>(spread, spread), vec2<f32>(0.0, 0.0));
    let cut_r = max(pad_r - vec4<f32>(spread), vec4<f32>(0.0));
    let d_cut = sd_round_rect_4(local_p - in.shadow_params.xy, cut_half, cut_r);
    let blur = in.shadow_params.z;
    var sh_cov = smoothstep(-aa, aa, d_cut);
    if blur > 0.25 {
        let inv_denom = 1.0 / max(blur * 0.70710678, 0.35);
        sh_cov = 0.5 + 0.5 * erf_approx(d_cut * inv_denom);
    }
    return to_premul(in.color_a) * (sh_cov * inside_mask);
}

fn srgb_to_linear_ch(c: f32) -> f32 {
    if c <= 0.04045 {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

fn pm_srgb_to_linear(pm: vec4<f32>) -> vec4<f32> {
    if pm.a <= 1e-5 {
        return vec4<f32>(0.0);
    }
    let srgb = clamp(pm.rgb / pm.a, vec3<f32>(0.0), vec3<f32>(1.0));
    let lin = vec3<f32>(
        srgb_to_linear_ch(srgb.r),
        srgb_to_linear_ch(srgb.g),
        srgb_to_linear_ch(srgb.b),
    );
    return vec4<f32>(lin * pm.a, pm.a);
}

fn eval_sdf_fragment(in: SdfVaryings) -> vec4<f32> {
    let screen_pos = in.pos_and_local.xy;
    let local_p = in.pos_and_local.zw;
    let aa = 0.5 * in.uv_and_scale.z;
    let opacity = in.uv_and_scale.w;
    let clip_cov = eval_clip_coverage(screen_pos, in.clip_geom, in.clip_radii);
    if clip_cov <= 0.0 || opacity <= 0.0 {
        return vec4<f32>(0.0);
    }

    let kind = in.flags.x;
    var pm = vec4<f32>(0.0);

    if kind == KIND_STYLED_BOX {
        pm = eval_styled_box(in, local_p, aa);
    } else if kind == KIND_OUTER_SHADOW {
        pm = eval_outer_shadow(in, local_p, aa);
    } else if kind == KIND_INSET_SHADOW {
        pm = eval_inset_shadow(in, local_p, aa);
    } else if kind == KIND_MASK_GLYPH || kind == KIND_COLOR_GLYPH {
        let half_size = in.box_geom.xy;
        let d_glyph = sd_round_rect_4(local_p, half_size, vec4<f32>(0.0));
        let edge_cov = smoothstep(aa, -aa, d_glyph);
        if edge_cov > 0.0 {
            let t_uv = clamp(
                (local_p + half_size) / max(2.0 * half_size, vec2<f32>(1e-4, 1e-4)),
                vec2<f32>(0.0),
                vec2<f32>(1.0),
            );
            let half_tex = vec2<f32>(0.5 / 1024.0, 0.5 / 1024.0);
            let uv_mid = 0.5 * (in.uv_bounds.xy + in.uv_bounds.zw);
            let uv_min = min(in.uv_bounds.xy + half_tex, uv_mid);
            let uv_max = max(in.uv_bounds.zw - half_tex, uv_mid);
            let guv = clamp(mix(in.uv_bounds.xy, in.uv_bounds.zw, t_uv), uv_min, uv_max);
            let layer = i32(in.flags.y);
            if kind == KIND_MASK_GLYPH {
                let cov = textureSampleLevel(t_mask_atlas, s_linear, guv, layer, 0.0).r;
                pm = to_premul(in.color_a) * (cov * edge_cov);
            } else {
                pm = textureSampleLevel(t_color_atlas, s_linear, guv, layer, 0.0) * edge_cov;
            }
        }
    } else if kind == KIND_SAMPLE_AUX_TEX {
        var mask = 1.0;
        if in.extra.w > 0.5 {
            let d_box = sd_round_rect_4(local_p, in.box_geom.xy, in.corner_radii);
            mask = smoothstep(aa, -aa, d_box);
        }
        if mask > 0.0 {
            pm = textureSampleLevel(t_aux, s_linear, in.uv_and_scale.xy, 0.0) * mask;
        }
    } else if kind == KIND_IMAGE_TEX {
        let half_size = in.box_geom.xy;
        let d_box = sd_round_rect_4(local_p, half_size, in.corner_radii);
        let mask = smoothstep(aa, -aa, d_box);
        if mask > 0.0 {
            let local_uv = clamp(
                (local_p + half_size) / max(2.0 * half_size, vec2<f32>(1e-4, 1e-4)),
                vec2<f32>(0.0),
                vec2<f32>(1.0),
            );
            let tex_dims = vec2<f32>(max(textureDimensions(t_aux, 0), vec2<u32>(1u, 1u)));
            let uv_span = abs(in.uv_bounds.zw - in.uv_bounds.xy);
            let duv_dlocal = (uv_span * tex_dims) / max(2.0 * half_size, vec2<f32>(1e-4, 1e-4));
            let j_x = vec2<f32>(in.shadow_params.x, in.shadow_params.y) * duv_dlocal;
            let j_y = vec2<f32>(in.shadow_params.z, in.shadow_params.w) * duv_dlocal;
            let max_mip = f32(max(textureNumLevels(t_aux), 1u) - 1u);
            let lod = clamp(log2(max(max(length(j_x), length(j_y)), 1e-4)) - 0.15, 0.0, max_mip);
            let mip_dims = max(tex_dims * exp2(-ceil(lod)), vec2<f32>(1.0, 1.0));
            let half_tex = vec2<f32>(0.5, 0.5) / mip_dims;
            let uv_mid = 0.5 * (in.uv_bounds.xy + in.uv_bounds.zw);
            let uv_min = min(in.uv_bounds.xy + half_tex, uv_mid);
            let uv_max = max(in.uv_bounds.zw - half_tex, uv_mid);
            let iuv = clamp(mix(in.uv_bounds.xy, in.uv_bounds.zw, local_uv), uv_min, uv_max);
            let texel = textureSampleLevel(t_aux, s_linear, iuv, lod);
            if in.flags.y != 0u {
                pm = texel * mask;
            } else {
                pm = to_premul(texel) * mask;
            }
        }
    }

    return pm * (clip_cov * opacity);
}

@fragment
fn fs_main(in: SdfVaryings) -> @location(0) vec4<f32> {
    return eval_sdf_fragment(in);
}

@fragment
fn fs_main_srgb(in: SdfVaryings) -> @location(0) vec4<f32> {
    return pm_srgb_to_linear(eval_sdf_fragment(in));
}

struct CanvasVertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct CanvasVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_canvas(in: CanvasVertexInput) -> CanvasVertexOutput {
    var out: CanvasVertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}

@fragment
fn fs_canvas(in: CanvasVertexOutput) -> @location(0) vec4<f32> {
    let mask_alpha = textureSampleLevel(t_mask_atlas, s_linear, in.uv, 0, 0.0).r;
    let a = in.color.a * mask_alpha;
    return vec4<f32>(in.color.rgb * a, a);
}

@fragment
fn fs_canvas_srgb(in: CanvasVertexOutput) -> @location(0) vec4<f32> {
    let mask_alpha = textureSampleLevel(t_mask_atlas, s_linear, in.uv, 0, 0.0).r;
    let a = in.color.a * mask_alpha;
    let lin = vec3<f32>(
        srgb_to_linear_ch(in.color.r),
        srgb_to_linear_ch(in.color.g),
        srgb_to_linear_ch(in.color.b),
    );
    return vec4<f32>(lin * a, a);
}
