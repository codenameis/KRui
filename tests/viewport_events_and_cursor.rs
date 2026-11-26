//! Integration tests for Wave 2 viewport interaction payloads (`PointerEvent`, `WheelEvent`),
//! `oncontextmenu`, `onwheel` scroll interception, OS pointer lock + relative motion,
//! dynamic CSS `cursor` resolution, and `krui-preview` IR parity.

#![forbid(unsafe_code)]

use krui::{
    krui, HoveredCursorKind, KeyInfo, Mods, PointerButton, PointerEvent, PointerHandler, ViewTree,
    WheelEvent, WheelHandler,
};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn pointer_buttons_local_coords_mods_and_context_menu() {
    let events: Rc<RefCell<Vec<(&'static str, PointerEvent)>>> = Rc::new(RefCell::new(Vec::new()));
    let down_log = events.clone();
    let move_log = events.clone();
    let up_log = events.clone();
    let ctx_log = events.clone();

    let on_down = PointerHandler::new(move |ev| down_log.borrow_mut().push(("down", ev)));
    let on_move = PointerHandler::new(move |ev| move_log.borrow_mut().push(("move", ev)));
    let on_up = PointerHandler::new(move |ev| up_log.borrow_mut().push(("up", ev)));
    let on_ctx = PointerHandler::new(move |ev| ctx_log.borrow_mut().push(("ctx", ev)));

    let mut tree = ViewTree::new();
    krui!(
        r#"
        <div style="width: 300px; height: 200px; padding: 20px;">
            <canvas
                id="vp"
                style="width: 120px; height: 80px;"
                onpointerdown="{on_down}"
                onpointermove="{on_move}"
                onpointerup="{on_up}"
                oncontextmenu="{on_ctx}"
            />
        </div>
        "#
    )
    .mount(&mut tree, None);
    let _ = tree.build_paint_scene(300, 200);

    let mods = Mods {
        ctrl: true,
        shift: true,
        ..Mods::default()
    };
    assert!(tree.dispatch_pointer_button(50.0, 45.0, PointerButton::Secondary, true, mods, 10));
    assert!(tree.dispatch_pointer_move_with_mods(62.0, 55.0, mods));
    assert!(tree.dispatch_pointer_button(62.0, 55.0, PointerButton::Secondary, false, mods, 40));

    let log = events.borrow();
    assert_eq!(log.len(), 4, "expected down/move/up/ctx, got {}", log.len());
    assert_eq!(log[0].0, "down");
    assert_eq!(log[0].1.button, PointerButton::Secondary);
    assert_eq!(log[0].1.buttons, PointerButton::Secondary.mask());
    assert!(
        (log[0].1.local_x - 30.0).abs() < 0.5,
        "local_x={}",
        log[0].1.local_x
    );
    assert!(
        (log[0].1.local_y - 25.0).abs() < 0.5,
        "local_y={}",
        log[0].1.local_y
    );
    assert!(log[0].1.mods.ctrl && log[0].1.mods.shift);

    assert_eq!(log[1].0, "move");
    assert!((log[1].1.delta_x - 12.0).abs() < 0.1);
    assert!((log[1].1.delta_y - 10.0).abs() < 0.1);
    assert_eq!(log[1].1.buttons, PointerButton::Secondary.mask());

    assert_eq!(log[2].0, "up");
    assert_eq!(log[2].1.button, PointerButton::Secondary);
    assert_eq!(log[2].1.buttons, 0);

    assert_eq!(log[3].0, "ctx");
    assert!((log[3].1.local_x - 42.0).abs() < 0.5);
    assert!((log[3].1.local_y - 35.0).abs() < 0.5);
}

#[test]
fn wheel_handler_intercepts_ancestor_scroll_and_passes_delta() {
    let wheel_log: Rc<RefCell<Vec<WheelEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let intercept = Rc::new(RefCell::new(true));

    let log_cb = wheel_log.clone();
    let int_cb = intercept.clone();
    let on_wheel = WheelHandler::new(move |ev: WheelEvent| {
        log_cb.borrow_mut().push(ev);
        *int_cb.borrow()
    });

    let mut tree = ViewTree::new();
    krui!(
        r#"
        <div id="scroller" style="width: 200px; height: 100px; overflow-y: scroll; flex-direction: column;">
            <canvas id="vp" style="width: 180px; height: 80px;" onwheel="{on_wheel}" />
            <div style="width: 180px; height: 220px;" />
        </div>
        "#
    )
    .mount(&mut tree, None);
    let _ = tree.build_paint_scene(200, 100);
    let scroller = tree.find_by_id("scroller").expect("scroller");

    assert!(tree.dispatch_wheel(40.0, 30.0, 4.0, -40.0, Mods::default()));
    assert_eq!(tree.scroll_offset_y(scroller), Some(0.0));
    assert_eq!(wheel_log.borrow().len(), 1);
    assert!((wheel_log.borrow()[0].delta_x - 4.0).abs() < 0.01);
    assert!((wheel_log.borrow()[0].delta_y - (-40.0)).abs() < 0.01);

    *intercept.borrow_mut() = false;
    assert!(tree.dispatch_wheel(40.0, 30.0, 0.0, -40.0, Mods::default()));
    assert!(
        tree.scroll_offset_y(scroller).unwrap_or(0.0) > 10.0,
        "ancestor must scroll when onwheel returns false"
    );
}

#[test]
fn pointer_lock_routes_relative_motion_and_exits_on_escape_and_blur() {
    let locked_deltas: Rc<RefCell<Vec<(f32, f32, bool)>>> = Rc::new(RefCell::new(Vec::new()));
    let log_cb = locked_deltas.clone();
    let on_move = PointerHandler::new(move |ev| {
        if ev.pointer_locked {
            log_cb
                .borrow_mut()
                .push((ev.delta_x, ev.delta_y, ev.pointer_locked));
        }
    });

    let mut tree = ViewTree::new();
    krui!(r#"<canvas id="vp3d" style="width: 160px; height: 120px;" onpointermove="{on_move}" />"#)
        .mount(&mut tree, None);
    let _ = tree.build_paint_scene(160, 120);
    let vp = tree.find_by_id("vp3d").expect("vp3d");

    assert!(tree.request_pointer_lock(vp));
    assert_eq!(tree.pointer_lock_node(), Some(vp));
    assert!(tree.dispatch_relative_pointer_motion(18.5, -7.25, Mods::default()));
    assert_eq!(*locked_deltas.borrow(), vec![(18.5, -7.25, true)]);

    let _ = tree.dispatch_routed_key(&KeyInfo::Escape, Mods::default(), false);
    assert_eq!(tree.pointer_lock_node(), None);
    assert!(!tree.dispatch_relative_pointer_motion(5.0, 5.0, Mods::default()));

    assert!(tree.request_pointer_lock(vp));
    let _ = tree.dispatch_window_focus_lost();
    assert_eq!(tree.pointer_lock_node(), None);
}

#[test]
fn css_cursor_property_and_active_capture_override_semantic_fallback() {
    let noop = PointerHandler::new(|_| {});
    let mut tree = ViewTree::new();
    krui!(
        r#"
        <style>
            #tool { width: 100px; height: 40px; cursor: crosshair; }
            #tool:active { cursor: grabbing; }
            #splitter { width: 10px; height: 40px; cursor: ew-resize; }
            #fallback { width: 80px; height: 40px; }
        </style>
        <div style="display: flex; flex-direction: row;">
            <div id="tool" onpointerdown="{noop}" />
            <div id="splitter" />
            <button id="fallback">OK</button>
        </div>
        "#
    )
    .mount(&mut tree, None);
    let _ = tree.build_paint_scene(300, 100);

    assert!(tree.dispatch_pointer_move(20.0, 20.0));
    assert_eq!(tree.hovered_cursor_kind(), HoveredCursorKind::Crosshair);

    assert!(tree.dispatch_pointer_down(20.0, 20.0));
    assert_eq!(tree.hovered_cursor_kind(), HoveredCursorKind::Grabbing);
    assert!(tree.dispatch_pointer_move(105.0, 20.0));
    assert_eq!(tree.hovered_cursor_kind(), HoveredCursorKind::Crosshair);
    assert!(tree.dispatch_pointer_up_at(20.0, 20.0));

    assert!(tree.dispatch_pointer_move(105.0, 20.0));
    assert_eq!(tree.hovered_cursor_kind(), HoveredCursorKind::EwResize);

    assert!(tree.dispatch_pointer_move(130.0, 20.0));
    assert_eq!(tree.hovered_cursor_kind(), HoveredCursorKind::Pointer);
}

#[cfg(feature = "preview")]
#[test]
fn preview_interpreter_binds_pointer_and_wheel_handlers() {
    use krui::{
        CallError, DynSignal, PointerTarget, Registry, RegistryFieldExt, ScopeRegistry, Value,
    };

    struct PreviewVm {
        on_down: PointerHandler,
        on_ctx: PointerHandler,
        on_wheel: WheelHandler,
    }

    impl Registry for PreviewVm {
        fn get(&self, name: &str) -> Option<Value> {
            match name {
                "on_down" => self.on_down.registry_field_get(),
                "on_ctx" => self.on_ctx.registry_field_get(),
                "on_wheel" => self.on_wheel.registry_field_get(),
                _ => None,
            }
        }

        fn signal(&self, _path: &str) -> Option<DynSignal> {
            None
        }

        fn call(&self, name: &str, _args: &[Value]) -> Result<Value, CallError> {
            Err(CallError::UnknownMethod(name.to_string()))
        }
    }

    let _ = <PointerHandler as PointerTarget>::into_pointer_fn;
    let hits: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let h1 = hits.clone();
    let h2 = hits.clone();
    let h3 = hits.clone();
    let vm = PreviewVm {
        on_down: PointerHandler::new(move |_| h1.borrow_mut().push("down")),
        on_ctx: PointerHandler::new(move |_| h2.borrow_mut().push("ctx")),
        on_wheel: WheelHandler::new(move |_| {
            h3.borrow_mut().push("wheel");
            true
        }),
    };

    let src = r#"
        <canvas
            id="pvp"
            style="width: 120px; height: 80px;"
            onpointerdown="{vm.on_down}"
            oncontextmenu="{vm.on_ctx}"
            onwheel="{vm.on_wheel}"
        />
    "#;
    let doc = krui_compiler::compile_krui_source(src).expect("compile");
    let mut tree = ViewTree::new();
    let mut scope = ScopeRegistry::new();
    scope.insert_registry("vm", vm);
    krui_preview::mount_compiled_document_with_registry(&mut tree, &doc, Rc::new(scope));
    let _ = tree.build_paint_scene(160, 120);

    assert!(tree.dispatch_pointer_button(
        20.0,
        20.0,
        PointerButton::Secondary,
        true,
        Mods::default(),
        0
    ));
    assert!(tree.dispatch_pointer_button(
        20.0,
        20.0,
        PointerButton::Secondary,
        false,
        Mods::default(),
        20
    ));
    assert!(tree.dispatch_wheel(20.0, 20.0, 0.0, -20.0, Mods::default()));
    assert_eq!(*hits.borrow(), vec!["down", "ctx", "wheel"]);
}
