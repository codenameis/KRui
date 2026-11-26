//! Canva-style 2D interactive `<canvas>` shape studio controller (`examples/component_gallery`).

use krui::*;
use std::cell::RefCell;
use std::rc::Rc;

const CANVAS_PALETTE: &[[u8; 4]] = &[
    [0x67, 0x50, 0xa4, 0xff],
    [0xe0, 0x53, 0x6c, 0xff],
    [0x15, 0x80, 0x3d, 0xff],
    [0x02, 0x84, 0xc7, 0xff],
    [0xd9, 0x77, 0x06, 0xff],
    [0x1e, 0x29, 0x3b, 0xff],
];

const fn rgba(c: [u8; 4]) -> Color {
    Color {
        r: c[0],
        g: c[1],
        b: c[2],
        a: c[3],
    }
}

const fn rgba_alpha(c: [u8; 4], a: u8) -> Color {
    Color {
        r: c[0],
        g: c[1],
        b: c[2],
        a,
    }
}

/// Interactive vector shape variants placeable on the Canva-style board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Rect,
    RoundedCard,
    Circle,
    Triangle,
    Star,
    Badge,
}

impl ShapeKind {
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Rect,
            1 => Self::RoundedCard,
            2 => Self::Circle,
            3 => Self::Triangle,
            4 => Self::Star,
            _ => Self::Badge,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Rect => "Rectangle",
            Self::RoundedCard => "Poster Card",
            Self::Circle => "Circle",
            Self::Triangle => "Triangle",
            Self::Star => "Star",
            Self::Badge => "Pill Badge",
        }
    }
}

/// Single interactive shape object on the 2D studio artboard.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasShape {
    pub id: u32,
    pub kind: ShapeKind,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color_idx: usize,
    pub label: String,
}

#[cfg(feature = "preview")]
impl krui::IntoRegistryValue for CanvasShape {
    fn into_registry_value(self) -> krui::Value {
        krui::Value::Unit
    }
}

#[cfg(feature = "preview")]
impl krui::TryFromRegistryValue for CanvasShape {
    fn try_from_registry_value(_value: &krui::Value) -> Option<Self> {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DragOp {
    None,
    Move {
        id: u32,
        off_x: f32,
        off_y: f32,
    },
    Resize {
        id: u32,
        start_x: f32,
        start_y: f32,
        orig_w: f32,
        orig_h: f32,
    },
}

fn starter_shapes() -> Vec<CanvasShape> {
    vec![
        CanvasShape {
            id: 1,
            kind: ShapeKind::RoundedCard,
            x: 44.0,
            y: 44.0,
            w: 220.0,
            h: 140.0,
            color_idx: 0,
            label: "KRui Canvas".to_string(),
        },
        CanvasShape {
            id: 2,
            kind: ShapeKind::Circle,
            x: 310.0,
            y: 62.0,
            w: 112.0,
            h: 112.0,
            color_idx: 1,
            label: String::new(),
        },
        CanvasShape {
            id: 3,
            kind: ShapeKind::Star,
            x: 470.0,
            y: 120.0,
            w: 116.0,
            h: 116.0,
            color_idx: 4,
            label: String::new(),
        },
        CanvasShape {
            id: 4,
            kind: ShapeKind::Badge,
            x: 110.0,
            y: 230.0,
            w: 180.0,
            h: 52.0,
            color_idx: 3,
            label: "Drag & Drop Me".to_string(),
        },
    ]
}

fn shape_contains(s: &CanvasShape, px: f32, py: f32) -> bool {
    px >= s.x && px <= s.x + s.w && py >= s.y && py <= s.y + s.h
}

fn resize_handle_contains(s: &CanvasShape, px: f32, py: f32) -> bool {
    let hx = s.x + s.w - 14.0;
    let hy = s.y + s.h - 14.0;
    px >= hx && px <= s.x + s.w + 4.0 && py >= hy && py <= s.y + s.h + 4.0
}

fn trace_triangle_path(ctx: &mut CanvasContext2D, s: &CanvasShape) {
    ctx.begin_path();
    ctx.move_to(s.x + s.w * 0.5, s.y);
    ctx.line_to(s.x + s.w, s.y + s.h);
    ctx.line_to(s.x, s.y + s.h);
    ctx.close_path();
}

fn trace_star_path(ctx: &mut CanvasContext2D, s: &CanvasShape) {
    let (cx, cy) = (s.x + s.w * 0.5, s.y + s.h * 0.5);
    let rx = s.w * 0.5;
    let ry = s.h * 0.5;
    ctx.begin_path();
    for i in 0..10 {
        let ang = -std::f32::consts::FRAC_PI_2 + (i as f32) * std::f32::consts::PI / 5.0;
        let k = if i % 2 == 0 { 1.0 } else { 0.44 };
        let (px, py) = (cx + rx * k * ang.cos(), cy + ry * k * ang.sin());
        if i == 0 {
            ctx.move_to(px, py);
        } else {
            ctx.line_to(px, py);
        }
    }
    ctx.close_path();
}

fn draw_single_shape(ctx: &mut CanvasContext2D, s: &CanvasShape, selected: bool) {
    let base = CANVAS_PALETTE[s.color_idx % CANVAS_PALETTE.len()];
    let fill = rgba(base);
    let shadow = rgba([0x0f, 0x17, 0x2a, 0x26]);
    match s.kind {
        ShapeKind::Rect => {
            ctx.fill_rect(s.x + 3.0, s.y + 4.0, s.w, s.h, shadow);
            ctx.fill_rect(s.x, s.y, s.w, s.h, fill);
        }
        ShapeKind::RoundedCard | ShapeKind::Badge => {
            let rad = if s.kind == ShapeKind::Badge {
                s.h * 0.5
            } else {
                16.0
            };
            ctx.fill_round_rect(s.x + 3.0, s.y + 4.0, s.w, s.h, rad, shadow);
            ctx.fill_round_rect(s.x, s.y, s.w, s.h, rad, fill);
            if !s.label.is_empty() {
                ctx.draw_text_styled(
                    &s.label,
                    s.x + s.w * 0.5,
                    s.y + s.h * 0.5 - 8.0,
                    16.0,
                    600,
                    rgba([0xff, 0xff, 0xff, 0xff]),
                    CanvasTextAlign::Center,
                );
            }
        }
        ShapeKind::Circle => {
            let r = s.w.min(s.h) * 0.5;
            ctx.fill_circle(s.x + r + 3.0, s.y + r + 4.0, r, shadow);
            ctx.fill_circle(s.x + r, s.y + r, r, fill);
        }
        ShapeKind::Triangle => {
            trace_triangle_path(ctx, s);
            ctx.fill_path(fill);
        }
        ShapeKind::Star => {
            trace_star_path(ctx, s);
            ctx.fill_path(fill);
        }
    }
    if selected {
        let accent = rgba([0x25, 0x63, 0xeb, 0xff]);
        ctx.set_line_dash(&[5.0, 3.0]);
        ctx.stroke_rect(s.x - 2.0, s.y - 2.0, s.w + 4.0, s.h + 4.0, 2.0, accent);
        ctx.set_line_dash(&[]);
        ctx.fill_rect(
            s.x + s.w - 6.0,
            s.y + s.h - 6.0,
            10.0,
            10.0,
            rgba([0xff, 0xff, 0xff, 0xff]),
        );
        ctx.stroke_rect(s.x + s.w - 6.0, s.y + s.h - 6.0, 10.0, 10.0, 2.0, accent);
    }
}

fn draw_artboard(ctx: &mut CanvasContext2D, size: Size, shapes: &[CanvasShape], selected_id: u32) {
    ctx.fill_rect(
        0.0,
        0.0,
        size.width,
        size.height,
        rgba([0xf8, 0xfa, 0xfc, 0xff]),
    );
    let dot_col = rgba_alpha([0x94, 0xa3, 0xb8, 0xff], 0x70);
    let mut y = 20.0;
    while y < size.height {
        let mut x = 20.0;
        while x < size.width {
            ctx.fill_circle(x, y, 1.3, dot_col);
            x += 24.0;
        }
        y += 24.0;
    }
    for s in shapes {
        draw_single_shape(ctx, s, s.id == selected_id);
    }
}

/// Interactive Canva-style 2D canvas shape editor controller.
#[controller]
#[derive(Clone)]
pub struct CanvasStudioController {
    shapes: Signal<Vec<CanvasShape>>,
    pub selected_id: Signal<u32>,
    pub active_color_idx: Signal<i32>,
    pub status_label: Computed<String>,
    pub shape_count_label: Computed<String>,
    pub canvas_draw: CanvasDraw,
    pub on_pointer_down: PointerHandler,
    pub on_pointer_move: PointerHandler,
    pub on_pointer_up: PointerHandler,
    next_id: Signal<u32>,
}

#[controller]
impl CanvasStudioController {
    /// Adds a new shape (`0=Rect, 1=RoundedCard, 2=Circle, 3=Triangle, 4=Star, 5=Badge`) and selects it.
    pub fn add_shape(&self, kind_code: i32) {
        let kind = ShapeKind::from_code(kind_code);
        let id = self.next_id.get_untracked();
        self.next_id.set(id + 1);
        let offset = ((id % 6) as f32) * 22.0;
        let (w, h) = match kind {
            ShapeKind::Badge => (160.0, 46.0),
            ShapeKind::RoundedCard => (170.0, 110.0),
            _ => (96.0, 96.0),
        };
        let label = match kind {
            ShapeKind::RoundedCard => format!("Card #{id}"),
            ShapeKind::Badge => format!("Tag #{id}"),
            _ => String::new(),
        };
        let shape = CanvasShape {
            id,
            kind,
            x: 80.0 + offset,
            y: 70.0 + offset * 0.7,
            w,
            h,
            color_idx: self.active_color_idx.get_untracked().max(0) as usize,
            label,
        };
        self.shapes.update(|list| list.push(shape));
        self.selected_id.set(id);
    }

    /// Changes active palette swatch and recolors the currently selected shape.
    pub fn set_selected_color(&self, idx: i32) {
        let c = idx.clamp(0, (CANVAS_PALETTE.len() - 1) as i32);
        self.active_color_idx.set(c);
        let sel = self.selected_id.get_untracked();
        if sel != 0 {
            self.shapes.update(|list| {
                if let Some(s) = list.iter_mut().find(|item| item.id == sel) {
                    s.color_idx = c as usize;
                }
            });
        }
    }

    /// Moves the selected shape to the top of the draw stack.
    pub fn bring_to_front(&self) {
        let sel = self.selected_id.get_untracked();
        self.shapes.update(|list| {
            if let Some(pos) = list.iter().position(|s| s.id == sel) {
                let item = list.remove(pos);
                list.push(item);
            }
        });
    }

    /// Moves the selected shape to the bottom of the draw stack.
    pub fn send_to_back(&self) {
        let sel = self.selected_id.get_untracked();
        self.shapes.update(|list| {
            if let Some(pos) = list.iter().position(|s| s.id == sel) {
                let item = list.remove(pos);
                list.insert(0, item);
            }
        });
    }

    /// Duplicates the currently selected shape slightly offset.
    pub fn duplicate_selected(&self) {
        let sel = self.selected_id.get_untracked();
        let next = self.next_id.get_untracked();
        let copy = self
            .shapes
            .get_untracked()
            .into_iter()
            .find(|s| s.id == sel);
        if let Some(mut s) = copy {
            self.next_id.set(next + 1);
            s.id = next;
            s.x += 24.0;
            s.y += 24.0;
            self.shapes.update(|list| list.push(s));
            self.selected_id.set(next);
        }
    }

    /// Deletes the currently selected shape (if any).
    pub fn delete_selected(&self) {
        let sel = self.selected_id.get_untracked();
        if sel == 0 {
            return;
        }
        self.shapes.update(|list| list.retain(|s| s.id != sel));
        let last_id = self.shapes.get_untracked().last().map_or(0, |s| s.id);
        self.selected_id.set(last_id);
    }

    /// Restores the initial Canva starter artboard.
    pub fn reset_board(&self) {
        self.shapes.set(starter_shapes());
        self.selected_id.set(1);
        self.next_id.set(5);
    }
}

fn build_canvas_pointer_handlers(
    shapes: Signal<Vec<CanvasShape>>,
    selected_id: Signal<u32>,
) -> (PointerHandler, PointerHandler, PointerHandler) {
    let drag: Rc<RefCell<DragOp>> = Rc::new(RefCell::new(DragOp::None));
    let drag_down = drag.clone();
    let on_down = PointerHandler::new(move |ev: PointerEvent| {
        let list = shapes.get_untracked();
        let cur_sel = selected_id.get_untracked();
        if let Some(sel) = list.iter().find(|s| s.id == cur_sel) {
            if resize_handle_contains(sel, ev.local_x, ev.local_y) {
                *drag_down.borrow_mut() = DragOp::Resize {
                    id: sel.id,
                    start_x: ev.local_x,
                    start_y: ev.local_y,
                    orig_w: sel.w,
                    orig_h: sel.h,
                };
                return;
            }
        }
        if let Some(hit) = list
            .iter()
            .rev()
            .find(|s| shape_contains(s, ev.local_x, ev.local_y))
        {
            selected_id.set(hit.id);
            *drag_down.borrow_mut() = DragOp::Move {
                id: hit.id,
                off_x: ev.local_x - hit.x,
                off_y: ev.local_y - hit.y,
            };
        } else {
            selected_id.set(0);
            *drag_down.borrow_mut() = DragOp::None;
        }
    });
    let drag_move = drag.clone();
    let on_move = PointerHandler::new(move |ev: PointerEvent| {
        let op = *drag_move.borrow();
        match op {
            DragOp::None => {}
            DragOp::Move { id, off_x, off_y } => shapes.update(|list| {
                if let Some(s) = list.iter_mut().find(|item| item.id == id) {
                    s.x = (ev.local_x - off_x).max(0.0);
                    s.y = (ev.local_y - off_y).max(0.0);
                }
            }),
            DragOp::Resize {
                id,
                start_x,
                start_y,
                orig_w,
                orig_h,
            } => shapes.update(|list| {
                if let Some(s) = list.iter_mut().find(|item| item.id == id) {
                    s.w = (orig_w + ev.local_x - start_x).clamp(32.0, 560.0);
                    s.h = (orig_h + ev.local_y - start_y).clamp(24.0, 360.0);
                }
            }),
        }
    });
    let on_up = PointerHandler::new(move |_| *drag.borrow_mut() = DragOp::None);
    (on_down, on_move, on_up)
}

impl CanvasStudioController {
    /// Constructs a new Canva-style [`CanvasStudioController`].
    pub fn new() -> Self {
        let shapes = Signal::new(starter_shapes());
        let selected_id = Signal::new(1_u32);
        let active_color_idx = Signal::new(0_i32);
        let status_label = Computed::new(move || {
            let sel = selected_id.get();
            shapes.get().iter().find(|s| s.id == sel).map_or_else(
                || "빈 공간 클릭 · 도형을 드래그해 이동/우하단 핸들로 크기 조절".to_string(),
                |s| {
                    format!(
                        "선택됨: #{} {} ({:.0}×{:.0} @ {:.0},{:.0})",
                        s.id,
                        s.kind.title(),
                        s.w,
                        s.h,
                        s.x,
                        s.y
                    )
                },
            )
        });
        let shape_count_label = Computed::new(move || format!("도형 {}개", shapes.get().len()));
        let canvas_draw = CanvasDraw::new(move |ctx: &mut CanvasContext2D, sz: Size| {
            draw_artboard(ctx, sz, &shapes.get(), selected_id.get());
        });
        let (on_pointer_down, on_pointer_move, on_pointer_up) =
            build_canvas_pointer_handlers(shapes, selected_id);
        Self {
            shapes,
            selected_id,
            active_color_idx,
            status_label,
            shape_count_label,
            canvas_draw,
            on_pointer_down,
            on_pointer_move,
            on_pointer_up,
            next_id: Signal::new(5),
        }
    }

    /// Returns a snapshot of current canvas shapes (used by Rust test/gate verification).
    pub fn shapes_snapshot(&self) -> Vec<CanvasShape> {
        self.shapes.get_untracked()
    }
}
