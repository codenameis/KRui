//! Extraction of CSS `clip-path` (`polygon()`, `path()`, `circle()`, `ellipse()`, `inset()`)
//! from Stylo `ComputedValues` into [`ClipShape`].

use app_units::Au;
use krui_style::{
    ClipCoord, ClipFillRule, ClipPathCmd, ClipPathCmdsRef, ClipPolygonRef, ClipRadius, ClipShape,
};
use style::properties::ComputedValues;
use style::values::computed::basic_shape::{
    BasicShape, ClipPath, PathOrShapeFunction, ShapeRadius,
};
use style::values::computed::length_percentage::{LengthPercentage, Unpacked};
use style::values::computed::Position;
use style::values::generics::basic_shape::{
    CommandEndPoint, ControlPoint, FillRule, GenericShapeCommand,
};
use style::values::generics::position::GenericPositionOrAuto;

const PCT_BASIS_PX: f32 = 100.0;

fn lp_to_clip_coord(lp: &LengthPercentage) -> ClipCoord {
    match lp.unpack() {
        Unpacked::Percentage(p) => ClipCoord::frac(p.0),
        Unpacked::Length(l) => ClipCoord::px(l.px()),
        Unpacked::Calc(_) => ClipCoord::px(lp.to_pixel_length(Au::from_f32_px(PCT_BASIS_PX)).px()),
    }
}

fn fill_rule_to_krui(r: FillRule) -> ClipFillRule {
    match r {
        FillRule::Evenodd => ClipFillRule::EvenOdd,
        FillRule::Nonzero => ClipFillRule::NonZero,
    }
}

fn shape_radius_to_krui(r: &ShapeRadius) -> ClipRadius {
    match r {
        ShapeRadius::Length(nn) => ClipRadius::Length(lp_to_clip_coord(&nn.0)),
        ShapeRadius::ClosestSide => ClipRadius::ClosestSide,
        ShapeRadius::FarthestSide | ShapeRadius::FarthestCorner | ShapeRadius::ClosestCorner => {
            ClipRadius::FarthestSide
        }
    }
}

fn pos_or_auto_to_coords(pos: &GenericPositionOrAuto<Position>) -> (ClipCoord, ClipCoord) {
    match pos {
        GenericPositionOrAuto::Auto => (ClipCoord::frac(0.5), ClipCoord::frac(0.5)),
        GenericPositionOrAuto::Position(p) => (
            lp_to_clip_coord(&p.horizontal),
            lp_to_clip_coord(&p.vertical),
        ),
    }
}

fn extract_svg_path_cmds(p: &style::values::generics::basic_shape::Path) -> Vec<ClipPathCmd> {
    let normalized = p.path.normalize(true);
    let mut cmds = Vec::with_capacity(normalized.commands().len());
    for cmd in normalized.commands() {
        match cmd {
            GenericShapeCommand::Move {
                point: CommandEndPoint::ToPosition(pt),
            } => cmds.push(ClipPathCmd::MoveTo(pt.horizontal, pt.vertical)),
            GenericShapeCommand::Line {
                point: CommandEndPoint::ToPosition(pt),
            } => cmds.push(ClipPathCmd::LineTo(pt.horizontal, pt.vertical)),
            GenericShapeCommand::QuadCurve {
                point: CommandEndPoint::ToPosition(pt),
                control1: ControlPoint::Absolute(c1),
            } => cmds.push(ClipPathCmd::QuadTo(
                c1.horizontal,
                c1.vertical,
                pt.horizontal,
                pt.vertical,
            )),
            GenericShapeCommand::CubicCurve {
                point: CommandEndPoint::ToPosition(pt),
                control1: ControlPoint::Absolute(c1),
                control2: ControlPoint::Absolute(c2),
            } => cmds.push(ClipPathCmd::CubicTo(
                c1.horizontal,
                c1.vertical,
                c2.horizontal,
                c2.vertical,
                pt.horizontal,
                pt.vertical,
            )),
            GenericShapeCommand::Arc {
                point: CommandEndPoint::ToPosition(pt),
                ..
            } => cmds.push(ClipPathCmd::LineTo(pt.horizontal, pt.vertical)),
            GenericShapeCommand::Close => cmds.push(ClipPathCmd::Close),
            _ => {}
        }
    }
    cmds
}

/// Extracts a supported CSS `clip-path` basic shape from `style`.
#[must_use]
pub(crate) fn extract_clip_path(style: &ComputedValues) -> Option<ClipShape> {
    let ClipPath::Shape(shape, _) = &style.get_svg().clip_path else {
        return None;
    };
    match &**shape {
        BasicShape::Polygon(poly) => {
            let pairs: Vec<_> = poly
                .coordinates
                .iter()
                .map(|c| (lp_to_clip_coord(&c.0), lp_to_clip_coord(&c.1)))
                .collect();
            Some(ClipShape::Polygon {
                fill_rule: fill_rule_to_krui(poly.fill),
                points: ClipPolygonRef::from_pairs(&pairs),
            })
        }
        BasicShape::PathOrShape(PathOrShapeFunction::Path(p)) => {
            let cmds = extract_svg_path_cmds(p);
            Some(ClipShape::Path {
                fill_rule: fill_rule_to_krui(p.fill),
                cmds: ClipPathCmdsRef::from_cmds(&cmds),
            })
        }
        BasicShape::Circle(c) => {
            let (cx, cy) = pos_or_auto_to_coords(&c.position);
            Some(ClipShape::Circle {
                cx,
                cy,
                radius: shape_radius_to_krui(&c.radius),
            })
        }
        BasicShape::Ellipse(e) => {
            let (cx, cy) = pos_or_auto_to_coords(&e.position);
            Some(ClipShape::Ellipse {
                cx,
                cy,
                rx: shape_radius_to_krui(&e.semiaxis_x),
                ry: shape_radius_to_krui(&e.semiaxis_y),
            })
        }
        BasicShape::Rect(ins) => {
            let r = &ins.rect;
            let basis = Au::from_f32_px(PCT_BASIS_PX);
            let corner_px = |c: &style::values::computed::BorderCornerRadius| {
                c.0.width().0.to_pixel_length(basis).px().max(0.0)
            };
            Some(ClipShape::Inset {
                top: lp_to_clip_coord(&r.0),
                right: lp_to_clip_coord(&r.1),
                bottom: lp_to_clip_coord(&r.2),
                left: lp_to_clip_coord(&r.3),
                radii: [
                    corner_px(&ins.round.top_left),
                    corner_px(&ins.round.top_right),
                    corner_px(&ins.round.bottom_right),
                    corner_px(&ins.round.bottom_left),
                ],
            })
        }
        BasicShape::PathOrShape(PathOrShapeFunction::Shape(_)) => None,
    }
}
