#![forbid(unsafe_code)]

use krui_style::Color;
use krui_ui_types::{
    clear_prevent_default_key, prevent_default_key, take_prevent_default_key, CanvasCommand,
    CanvasContext2D, ClickBinding, InputTarget, KeydownOutcome, PointerButton, PointerEvent,
    PointerHandler, PointerTarget, Size, StaticNodeDesc, StaticNodeKind, WheelEvent, WheelHandler,
    WheelTarget,
};

#[test]
fn static_node_desc_defaults_and_anchor_match_expected_flags() {
    assert_eq!(StaticNodeDesc::BLANK.kind, StaticNodeKind::Element);
    assert_eq!(StaticNodeDesc::BLANK.tag, "div");
    assert_eq!(
        StaticNodeDesc::ANCHOR.kind,
        StaticNodeKind::ControlFlowAnchor
    );
    assert_eq!(StaticNodeDesc::ANCHOR.tag, "");
}

#[test]
fn canvas_context_records_shapes_and_transforms() {
    let mut ctx = CanvasContext2D::with_size(200.0, 100.0);
    ctx.translate(10.0, 20.0);
    ctx.fill_rect(0.0, 0.0, 50.0, 30.0, Color::rgba(255, 0, 0, 255));
    let cmds = ctx.into_commands();
    assert_eq!(cmds.len(), 2);
    assert!(matches!(cmds[0], CanvasCommand::SetTransform(_)));
    assert!(matches!(cmds[1], CanvasCommand::FillRect { w, h, .. } if w == 50.0 && h == 30.0));
    assert_eq!(Size::new(200.0, 100.0).width, 200.0);
}

#[test]
fn event_targets_and_prevent_default_work_without_core() {
    clear_prevent_default_key();
    let cb = (|_: String| -> bool { true }).into_input_fn();
    cb("Enter".to_string());
    assert!(take_prevent_default_key());
    assert!(!take_prevent_default_key());

    prevent_default_key();
    assert!(take_prevent_default_key());

    let binding = ClickBinding::from(|| {});
    (binding.action)();
    assert!(binding.can_execute.is_none());

    let ptr = PointerHandler::new(|ev: PointerEvent| {
        assert_eq!(ev.button, PointerButton::Primary);
    });
    (ptr.into_pointer_fn())(PointerEvent::default());

    let wh = WheelHandler::new(|ev: WheelEvent| ev.delta_y > 0.0);
    assert!((wh.into_wheel_fn())(WheelEvent {
        delta_y: 12.0,
        ..WheelEvent::default()
    }));
    assert!(!().into_prevent_default());
}
