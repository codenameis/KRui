//! Browser HTML5 `<canvas>` 2D renderer executing [`CanvasDraw`] + `ResizeObserver`.

use krui_style::{Affine2, Color};
use krui_ui_types::{
    CanvasCommand, CanvasContext2D, CanvasDraw, CanvasFillRule, CanvasGradient, CanvasLineCap,
    CanvasLineJoin, CanvasTextAlign, PathSegment, Size,
};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{Clamped, JsCast};

pub(crate) struct CanvasBinding {
    canvas: web_sys::HtmlCanvasElement,
    draw: Rc<RefCell<Option<CanvasDraw>>>,
    observer: web_sys::ResizeObserver,
    _observer_cb: Closure<dyn FnMut(js_sys::Array)>,
}

impl Drop for CanvasBinding {
    fn drop(&mut self) {
        self.observer.disconnect();
    }
}

impl CanvasBinding {
    pub(crate) fn new(
        canvas: web_sys::HtmlCanvasElement,
        draw: CanvasDraw,
        session: krui_signals::ReactiveSession,
    ) -> Option<Self> {
        let shared_draw = Rc::new(RefCell::new(Some(draw)));
        let canvas_for_obs = canvas.clone();
        let draw_for_obs = Rc::clone(&shared_draw);
        let observer_cb = Closure::wrap(Box::new(move |_: js_sys::Array| {
            let Some(draw_fn) = draw_for_obs.borrow().as_ref().cloned() else {
                return;
            };
            session.enter(|| {
                krui_signals::untracked(|| render_canvas_now(&canvas_for_obs, &draw_fn));
            });
        }) as Box<dyn FnMut(js_sys::Array)>);
        let observer = web_sys::ResizeObserver::new(observer_cb.as_ref().unchecked_ref()).ok()?;
        observer.observe(canvas.as_ref());
        let binding = Self {
            canvas,
            draw: shared_draw,
            observer,
            _observer_cb: observer_cb,
        };
        binding.redraw();
        Some(binding)
    }

    pub(crate) fn set_draw(&self, draw: CanvasDraw) {
        *self.draw.borrow_mut() = Some(draw);
        self.redraw();
    }

    pub(crate) fn redraw(&self) {
        let Some(draw) = self.draw.borrow().as_ref().cloned() else {
            return;
        };
        render_canvas_now(&self.canvas, &draw);
    }
}

fn color_css(c: Color) -> String {
    let alpha = c.a as f32 / 255.0;
    format!("rgba({}, {}, {}, {alpha:.4})", c.r, c.g, c.b)
}

fn render_canvas_now(canvas: &web_sys::HtmlCanvasElement, draw: &CanvasDraw) {
    let rect = canvas.get_bounding_client_rect();
    let css_w = (rect.width() as f32).max(1.0);
    let css_h = (rect.height() as f32).max(1.0);
    let dpr = web_sys::window()
        .map(|w| w.device_pixel_ratio() as f32)
        .unwrap_or(1.0)
        .max(1.0);
    let px_w = (css_w * dpr).round().max(1.0) as u32;
    let px_h = (css_h * dpr).round().max(1.0) as u32;
    if canvas.width() != px_w {
        canvas.set_width(px_w);
    }
    if canvas.height() != px_h {
        canvas.set_height(px_h);
    }
    let Ok(Some(obj)) = canvas.get_context("2d") else {
        return;
    };
    let Ok(ctx2d) = obj.dyn_into::<web_sys::CanvasRenderingContext2d>() else {
        return;
    };
    let mut rec = CanvasContext2D::with_size(css_w, css_h);
    draw.draw(&mut rec, Size::new(css_w, css_h));
    let _ = ctx2d.set_transform(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
    ctx2d.clear_rect(0.0, 0.0, px_w as f64, px_h as f64);
    ctx2d.save();
    let _ = ctx2d.scale(dpr as f64, dpr as f64);
    for cmd in rec.commands() {
        execute_command(&ctx2d, cmd, dpr);
    }
    ctx2d.restore();
}

fn execute_command(ctx: &web_sys::CanvasRenderingContext2d, cmd: &CanvasCommand, dpr: f32) {
    match cmd {
        CanvasCommand::FillRect {
            x,
            y,
            w,
            h,
            radius,
            color,
        } => draw_fill_rect(ctx, *x, *y, *w, *h, *radius, *color),
        CanvasCommand::StrokeRect {
            x,
            y,
            w,
            h,
            line_width,
            color,
        } => {
            ctx.set_stroke_style_str(&color_css(*color));
            ctx.set_line_width(*line_width as f64);
            ctx.stroke_rect(*x as f64, *y as f64, *w as f64, *h as f64);
        }
        CanvasCommand::FillCircle {
            cx,
            cy,
            radius,
            color,
        } => {
            ctx.begin_path();
            let _ = ctx.arc(
                *cx as f64,
                *cy as f64,
                (*radius).max(0.0) as f64,
                0.0,
                std::f64::consts::TAU,
            );
            ctx.set_fill_style_str(&color_css(*color));
            ctx.fill();
        }
        CanvasCommand::StrokeLine {
            x1,
            y1,
            x2,
            y2,
            line_width,
            line_cap,
            color,
        } => {
            ctx.begin_path();
            ctx.move_to(*x1 as f64, *y1 as f64);
            ctx.line_to(*x2 as f64, *y2 as f64);
            ctx.set_line_cap(cap_str(*line_cap));
            ctx.set_line_width(*line_width as f64);
            ctx.set_stroke_style_str(&color_css(*color));
            ctx.stroke();
        }
        other => execute_complex_command(ctx, other, dpr),
    }
}

fn execute_complex_command(ctx: &web_sys::CanvasRenderingContext2d, cmd: &CanvasCommand, dpr: f32) {
    match cmd {
        CanvasCommand::FillPath {
            segments,
            fill_rule,
            color,
        } => {
            replay_segments(ctx, segments);
            ctx.set_fill_style_str(&color_css(*color));
            fill_with_rule(ctx, *fill_rule);
        }
        CanvasCommand::StrokePath {
            segments,
            line_width,
            line_cap,
            line_join,
            color,
        } => {
            replay_segments(ctx, segments);
            ctx.set_line_cap(cap_str(*line_cap));
            ctx.set_line_join(join_str(*line_join));
            ctx.set_line_width(*line_width as f64);
            ctx.set_stroke_style_str(&color_css(*color));
            ctx.stroke();
        }
        CanvasCommand::SetTransform(m) => apply_affine(ctx, *m, dpr),
        CanvasCommand::PushClip {
            segments,
            fill_rule,
        } => {
            ctx.save();
            replay_segments(ctx, segments);
            clip_with_rule(ctx, *fill_rule);
        }
        CanvasCommand::PopClip => ctx.restore(),
        CanvasCommand::FillGradient {
            segments,
            fill_rule,
            gradient,
            alpha,
        } => fill_gradient(ctx, segments, *fill_rule, gradient, *alpha),
        CanvasCommand::DrawText {
            text,
            x,
            y,
            font_size,
            font_weight,
            color,
            align,
        } => draw_text(ctx, text, *x, *y, *font_size, *font_weight, *color, *align),
        CanvasCommand::DrawImage {
            source: krui_ui_types::ImageSource::Decoded(img),
            dst_rect,
            alpha,
            ..
        } => {
            draw_decoded_rgba(ctx, &img.rgba8, img.width, img.height, *dst_rect, *alpha);
        }
        _ => {}
    }
}

fn draw_fill_rect(
    ctx: &web_sys::CanvasRenderingContext2d,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    color: Color,
) {
    ctx.set_fill_style_str(&color_css(color));
    if r <= 0.1 {
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);
        return;
    }
    let rr = r.min(w * 0.5).min(h * 0.5).max(0.0) as f64;
    let (x0, y0, wf, hf) = (x as f64, y as f64, w as f64, h as f64);
    ctx.begin_path();
    ctx.move_to(x0 + rr, y0);
    ctx.line_to(x0 + wf - rr, y0);
    ctx.quadratic_curve_to(x0 + wf, y0, x0 + wf, y0 + rr);
    ctx.line_to(x0 + wf, y0 + hf - rr);
    ctx.quadratic_curve_to(x0 + wf, y0 + hf, x0 + wf - rr, y0 + hf);
    ctx.line_to(x0 + rr, y0 + hf);
    ctx.quadratic_curve_to(x0, y0 + hf, x0, y0 + hf - rr);
    ctx.line_to(x0, y0 + rr);
    ctx.quadratic_curve_to(x0, y0, x0 + rr, y0);
    ctx.close_path();
    ctx.fill();
}

fn replay_segments(ctx: &web_sys::CanvasRenderingContext2d, segments: &[PathSegment]) {
    ctx.begin_path();
    for &seg in segments {
        match seg {
            PathSegment::MoveTo(x, y) => ctx.move_to(x as f64, y as f64),
            PathSegment::LineTo(x, y) => ctx.line_to(x as f64, y as f64),
            PathSegment::QuadTo { cx, cy, x, y } => {
                ctx.quadratic_curve_to(cx as f64, cy as f64, x as f64, y as f64);
            }
            PathSegment::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => ctx.bezier_curve_to(
                c1x as f64, c1y as f64, c2x as f64, c2y as f64, x as f64, y as f64,
            ),
            PathSegment::Arc {
                cx,
                cy,
                rx,
                ry,
                start_rad,
                sweep_rad,
                x_rotation_rad,
            } => {
                let _ = ctx.ellipse_with_anticlockwise(
                    cx as f64,
                    cy as f64,
                    rx.abs().max(0.01) as f64,
                    ry.abs().max(0.01) as f64,
                    x_rotation_rad as f64,
                    start_rad as f64,
                    (start_rad + sweep_rad) as f64,
                    sweep_rad < 0.0,
                );
            }
            PathSegment::Close => ctx.close_path(),
        }
    }
}

fn apply_affine(ctx: &web_sys::CanvasRenderingContext2d, m: Affine2, dpr: f32) {
    let d = dpr as f64;
    let _ = ctx.set_transform(
        m.a as f64 * d,
        m.b as f64 * d,
        m.c as f64 * d,
        m.d as f64 * d,
        m.tx as f64 * d,
        m.ty as f64 * d,
    );
}

fn fill_with_rule(ctx: &web_sys::CanvasRenderingContext2d, rule: CanvasFillRule) {
    let winding = match rule {
        CanvasFillRule::NonZero => web_sys::CanvasWindingRule::Nonzero,
        CanvasFillRule::EvenOdd => web_sys::CanvasWindingRule::Evenodd,
    };
    ctx.fill_with_canvas_winding_rule(winding);
}

fn clip_with_rule(ctx: &web_sys::CanvasRenderingContext2d, rule: CanvasFillRule) {
    let winding = match rule {
        CanvasFillRule::NonZero => web_sys::CanvasWindingRule::Nonzero,
        CanvasFillRule::EvenOdd => web_sys::CanvasWindingRule::Evenodd,
    };
    ctx.clip_with_canvas_winding_rule(winding);
}

fn fill_gradient(
    ctx: &web_sys::CanvasRenderingContext2d,
    segments: &[PathSegment],
    rule: CanvasFillRule,
    grad: &CanvasGradient,
    alpha: f32,
) {
    let js_grad = match grad {
        CanvasGradient::Linear { x0, y0, x1, y1, .. } => {
            Ok(ctx.create_linear_gradient(*x0 as f64, *y0 as f64, *x1 as f64, *y1 as f64))
        }
        CanvasGradient::Radial {
            x0,
            y0,
            r0,
            x1,
            y1,
            r1,
            ..
        } => ctx.create_radial_gradient(
            *x0 as f64, *y0 as f64, *r0 as f64, *x1 as f64, *y1 as f64, *r1 as f64,
        ),
    };
    let Ok(g) = js_grad else {
        return;
    };
    for stop in grad.stops() {
        let mut c = stop.color;
        c.a = ((c.a as f32) * alpha.clamp(0.0, 1.0)).round() as u8;
        let _ = g.add_color_stop(stop.offset, &color_css(c));
    }
    replay_segments(ctx, segments);
    ctx.set_fill_style_canvas_gradient(&g);
    fill_with_rule(ctx, rule);
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    ctx: &web_sys::CanvasRenderingContext2d,
    text: &str,
    x: f32,
    y: f32,
    font_size: f32,
    font_weight: u16,
    color: Color,
    align: CanvasTextAlign,
) {
    ctx.set_font(&format!(
        "{font_weight} {font_size:.1}px Roboto, sans-serif"
    ));
    ctx.set_fill_style_str(&color_css(color));
    ctx.set_text_align(match align {
        CanvasTextAlign::Left => "left",
        CanvasTextAlign::Center => "center",
        CanvasTextAlign::Right => "right",
    });
    ctx.set_text_baseline("top");
    let _ = ctx.fill_text(text, x as f64, y as f64);
}

fn draw_decoded_rgba(
    ctx: &web_sys::CanvasRenderingContext2d,
    rgba: &[u8],
    w: u32,
    h: u32,
    dst: krui_style::LayoutRect,
    alpha: f32,
) {
    let Some(doc) = web_sys::window().and_then(|win| win.document()) else {
        return;
    };
    let Some(tmp) = doc
        .create_element("canvas")
        .ok()
        .and_then(|el| el.dyn_into::<web_sys::HtmlCanvasElement>().ok())
    else {
        return;
    };
    tmp.set_width(w);
    tmp.set_height(h);
    let Ok(Some(obj)) = tmp.get_context("2d") else {
        return;
    };
    let Ok(tctx) = obj.dyn_into::<web_sys::CanvasRenderingContext2d>() else {
        return;
    };
    if let Ok(img_data) = web_sys::ImageData::new_with_u8_clamped_array(Clamped(rgba), w) {
        let _ = tctx.put_image_data(&img_data, 0.0, 0.0);
        ctx.save();
        ctx.set_global_alpha(alpha.clamp(0.0, 1.0) as f64);
        let _ = ctx.draw_image_with_html_canvas_element_and_dw_and_dh(
            &tmp,
            dst.x as f64,
            dst.y as f64,
            dst.width as f64,
            dst.height as f64,
        );
        ctx.restore();
    }
}

fn cap_str(cap: CanvasLineCap) -> &'static str {
    match cap {
        CanvasLineCap::Butt => "butt",
        CanvasLineCap::Round => "round",
        CanvasLineCap::Square => "square",
    }
}

fn join_str(join: CanvasLineJoin) -> &'static str {
    match join {
        CanvasLineJoin::Miter => "miter",
        CanvasLineJoin::Round => "round",
        CanvasLineJoin::Bevel => "bevel",
    }
}
