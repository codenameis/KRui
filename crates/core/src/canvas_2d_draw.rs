//! Gradient, dash-aware fill/stroke, text, and image drawing methods split out of [`super::canvas_2d`].

use super::canvas_2d::{
    CanvasCommand, CanvasContext2D, CanvasFillRule, CanvasGradient, CanvasTextAlign, PathSegment,
};
use super::canvas_2d_extras::{circle_to_path_segments, dash_path_segments, rect_to_path_segments};
use crate::media::ImageSource;
use krui_layout::LayoutRect;
use krui_style::Color;

impl CanvasContext2D {
    /// Fills the current path with `color`.
    pub fn fill_path(&mut self, color: Color) {
        if !self.current_path.is_empty() {
            self.sync_transform();
            let eff = self.effective_color(color);
            self.commands.push(CanvasCommand::FillPath {
                segments: self.current_path.clone(),
                fill_rule: self.state.fill_rule,
                color: eff,
            });
        }
    }

    /// Fills the current path using a linear or radial [`CanvasGradient`].
    pub fn fill_path_gradient(&mut self, gradient: CanvasGradient) {
        if !self.current_path.is_empty() {
            self.sync_transform();
            self.commands.push(CanvasCommand::FillGradient {
                segments: self.current_path.clone(),
                fill_rule: self.state.fill_rule,
                gradient,
                alpha: self.state.global_alpha,
            });
        }
    }

    /// Strokes the current path with `line_width` and `color`.
    pub fn stroke_path(&mut self, line_width: f32, color: Color) {
        if self.current_path.is_empty() {
            return;
        }
        let segments = if self.state.line_dash.is_empty() {
            self.current_path.clone()
        } else {
            dash_path_segments(
                &self.current_path,
                &self.state.line_dash,
                self.state.line_dash_offset,
            )
        };
        if !segments.is_empty() {
            self.sync_transform();
            let eff = self.effective_color(color);
            self.commands.push(CanvasCommand::StrokePath {
                segments,
                line_width: line_width.max(0.05),
                line_cap: self.state.line_cap,
                line_join: self.state.line_join,
                color: eff,
            });
        }
    }

    /// Fills a sharp rectangle at `(x, y)` of size `(w, h)`.
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.fill_round_rect(x, y, w, h, 0.0, color);
    }

    /// Fills a rounded rectangle at `(x, y)` of size `(w, h)` with uniform corner `radius`.
    pub fn fill_round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
        self.sync_transform();
        let eff = self.effective_color(color);
        self.commands.push(CanvasCommand::FillRect {
            x,
            y,
            w,
            h,
            radius,
            color: eff,
        });
    }

    /// Fills a sharp or rounded rectangle using `gradient`.
    pub fn fill_rect_gradient(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        gradient: CanvasGradient,
    ) {
        let segments = rect_to_path_segments(x, y, w, h, radius);
        if !segments.is_empty() {
            self.sync_transform();
            self.commands.push(CanvasCommand::FillGradient {
                segments,
                fill_rule: CanvasFillRule::NonZero,
                gradient,
                alpha: self.state.global_alpha,
            });
        }
    }

    /// Alias of [`Self::fill_round_rect`] for CSS-style naming consistency.
    pub fn fill_rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
        self.fill_round_rect(x, y, w, h, radius, color);
    }

    /// Strokes a rectangle outline at `(x, y)` of size `(w, h)`.
    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, line_width: f32, color: Color) {
        self.sync_transform();
        let eff = self.effective_color(color);
        if self.state.line_dash.is_empty() {
            self.commands.push(CanvasCommand::StrokeRect {
                x,
                y,
                w,
                h,
                line_width: line_width.max(0.05),
                color: eff,
            });
        } else {
            let segs = dash_path_segments(
                &rect_to_path_segments(x, y, w, h, 0.0),
                &self.state.line_dash,
                self.state.line_dash_offset,
            );
            if !segs.is_empty() {
                self.commands.push(CanvasCommand::StrokePath {
                    segments: segs,
                    line_width: line_width.max(0.05),
                    line_cap: self.state.line_cap,
                    line_join: self.state.line_join,
                    color: eff,
                });
            }
        }
    }

    /// Fills a circle centered at `(cx, cy)` with `radius`.
    pub fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, color: Color) {
        self.sync_transform();
        let eff = self.effective_color(color);
        self.commands.push(CanvasCommand::FillCircle {
            cx,
            cy,
            radius,
            color: eff,
        });
    }

    /// Fills a circle centered at `(cx, cy)` with `radius` using `gradient`.
    pub fn fill_circle_gradient(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        gradient: CanvasGradient,
    ) {
        let segments = circle_to_path_segments(cx, cy, radius);
        if !segments.is_empty() {
            self.sync_transform();
            self.commands.push(CanvasCommand::FillGradient {
                segments,
                fill_rule: CanvasFillRule::NonZero,
                gradient,
                alpha: self.state.global_alpha,
            });
        }
    }

    /// Strokes a circle centered at `(cx, cy)` with `radius` and `line_width`.
    pub fn stroke_circle(&mut self, cx: f32, cy: f32, radius: f32, line_width: f32, color: Color) {
        let raw = circle_to_path_segments(cx, cy, radius);
        if raw.is_empty() {
            return;
        }
        let segments = if self.state.line_dash.is_empty() {
            raw
        } else {
            dash_path_segments(&raw, &self.state.line_dash, self.state.line_dash_offset)
        };
        if !segments.is_empty() {
            self.sync_transform();
            let eff = self.effective_color(color);
            self.commands.push(CanvasCommand::StrokePath {
                segments,
                line_width: line_width.max(0.05),
                line_cap: self.state.line_cap,
                line_join: self.state.line_join,
                color: eff,
            });
        }
    }

    /// Strokes a straight line segment from `(x1, y1)` to `(x2, y2)`.
    pub fn stroke_line(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        line_width: f32,
        color: Color,
    ) {
        self.sync_transform();
        let eff = self.effective_color(color);
        if self.state.line_dash.is_empty() {
            self.commands.push(CanvasCommand::StrokeLine {
                x1,
                y1,
                x2,
                y2,
                line_width: line_width.max(0.05),
                line_cap: self.state.line_cap,
                color: eff,
            });
        } else {
            let raw = [PathSegment::MoveTo(x1, y1), PathSegment::LineTo(x2, y2)];
            let segments =
                dash_path_segments(&raw, &self.state.line_dash, self.state.line_dash_offset);
            if !segments.is_empty() {
                self.commands.push(CanvasCommand::StrokePath {
                    segments,
                    line_width: line_width.max(0.05),
                    line_cap: self.state.line_cap,
                    line_join: self.state.line_join,
                    color: eff,
                });
            }
        }
    }

    /// Draws text `text` with top-left baseline box at `(x, y)` using `font_size` and `color`.
    pub fn draw_text(
        &mut self,
        text: impl Into<String>,
        x: f32,
        y: f32,
        font_size: f32,
        color: Color,
    ) {
        self.draw_text_styled(text, x, y, font_size, 400, color, CanvasTextAlign::Left);
    }

    /// Draws text `text` at `(x, y)` with explicit `font_size`, `font_weight`, `color`, and `align`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text_styled(
        &mut self,
        text: impl Into<String>,
        x: f32,
        y: f32,
        font_size: f32,
        font_weight: u16,
        color: Color,
        align: CanvasTextAlign,
    ) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        self.sync_transform();
        let eff = self.effective_color(color);
        self.commands.push(CanvasCommand::DrawText {
            text,
            x,
            y,
            font_size: font_size.max(1.0),
            font_weight,
            color: eff,
            align,
        });
    }

    /// Draws a decoded RGBA8 or external texture `image` stretched into `(x, y, w, h)`.
    pub fn draw_image(&mut self, image: impl Into<ImageSource>, x: f32, y: f32, w: f32, h: f32) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.sync_transform();
        self.commands.push(CanvasCommand::DrawImage {
            source: image.into(),
            src_rect: None,
            dst_rect: LayoutRect {
                x,
                y,
                width: w,
                height: h,
            },
            alpha: self.state.global_alpha,
        });
    }

    /// Draws a sub-rectangle `src_rect` (`[sx, sy, sw, sh]` in source pixels) into `dst_rect`.
    pub fn draw_image_region(
        &mut self,
        image: impl Into<ImageSource>,
        src_rect: [f32; 4],
        dst_rect: LayoutRect,
    ) {
        if dst_rect.width <= 0.0
            || dst_rect.height <= 0.0
            || src_rect[2] <= 0.0
            || src_rect[3] <= 0.0
        {
            return;
        }
        self.sync_transform();
        self.commands.push(CanvasCommand::DrawImage {
            source: image.into(),
            src_rect: Some(src_rect),
            dst_rect,
            alpha: self.state.global_alpha,
        });
    }
}
