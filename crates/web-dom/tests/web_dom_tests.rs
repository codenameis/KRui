#![forbid(unsafe_code)]

use krui_signals::{ReactiveSession, Signal};
use krui_style::Color;
use krui_ui_types::{
    prevent_default_wheel, CanvasCommand, CanvasDraw, StaticDocumentDesc, StaticNodeDesc,
    WheelEvent, WheelHandler,
};
use krui_web::static_ir::{mount_static_document, mount_static_nodes};
use krui_web::{MockDomBackend, WebDomTree, WebMount, WebViewFragment};
use std::cell::RefCell;
use std::rc::Rc;

fn with_mock<R>(tree: &WebDomTree, f: impl FnOnce(&MockDomBackend) -> R) -> R {
    tree.with_backend_ref(|b| {
        let mock = b
            .as_any()
            .and_then(|a| a.downcast_ref::<MockDomBackend>())
            .expect("expected MockDomBackend");
        f(mock)
    })
}

fn with_mock_mut<R>(tree: &mut WebDomTree, f: impl FnOnce(&mut MockDomBackend) -> R) -> R {
    tree.with_backend_mut(|b| {
        let mock = b
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<MockDomBackend>())
            .expect("expected MockDomBackend");
        f(mock)
    })
}

#[test]
fn reactive_signals_update_mock_dom_and_stop_after_dispose() {
    let session = ReactiveSession::new();
    session.enter(|| {
        let count = Signal::new(1_i32);
        let mut mount = WebMount::new();
        static NODES: [StaticNodeDesc; 1] = [StaticNodeDesc::BLANK];
        mount.mount(WebViewFragment::new(move |tree, parent| {
            let ids = mount_static_nodes(tree, parent, &NODES);
            tree.bind_dynamic_text(ids[0], move || format!("count={}", count.get()));
            ids
        }));
        assert!(mount.mutations().iter().any(|m| {
            matches!(m, krui_web::DomMutation::SetText { text, .. } if text == "count=1")
        }));
        count.set(42);
        mount.dispose();
        count.set(99);
    });
}

#[test]
fn synthetic_text_run_mounts_text_node_and_injects_ua_layers() {
    let session = ReactiveSession::new();
    session.enter(|| {
        static NODES: [StaticNodeDesc; 2] = [
            StaticNodeDesc {
                tag: "button",
                is_button: true,
                ..StaticNodeDesc::BLANK
            },
            StaticNodeDesc {
                parent_slot: Some(0),
                is_synthetic_text_run: true,
                static_text: Some("확인"),
                ..StaticNodeDesc::BLANK
            },
        ];
        static DOC: StaticDocumentDesc = StaticDocumentDesc {
            unstyled: true,
            stylesheets: &[".krui-ui-btn { color: red; }"],
            nodes: &NODES,
        };
        let mut tree = WebDomTree::with_backend(Box::new(MockDomBackend::new()));
        let ids = mount_static_document(&mut tree, None, &DOC, true);
        with_mock(&tree, |mock| {
            assert_eq!(mock.node(ids[0]).unwrap().tag, "button");
            assert_eq!(mock.node(ids[1]).unwrap().tag, "#text");
            assert_eq!(mock.node(ids[1]).unwrap().text, "확인");
            assert!(mock.stylesheets()[0].0);
            assert!(mock.stylesheets()[0]
                .1
                .contains("@layer krui-ua, preset, component, micl;"));
            assert!(mock.stylesheets()[0].1.contains("@layer krui-ua {"));
        });
    });
}

fn attach_test_if_and_for(
    tree: &mut WebDomTree,
    ul_id: krui_web::DomNodeId,
    if_anchor: krui_web::DomNodeId,
    for_anchor: krui_web::DomNodeId,
    show: Signal<bool>,
    items: Signal<Vec<(i32, &'static str)>>,
    created_ids: Rc<RefCell<Vec<krui_web::DomNodeId>>>,
) {
    tree.attach_if_to_anchor(
        Some(ul_id),
        if_anchor,
        move || show.get().then_some(0),
        vec![Rc::new(|t, p| {
            static LI: [StaticNodeDesc; 1] = [StaticNodeDesc {
                tag: "li",
                id: Some("if-slot"),
                ..StaticNodeDesc::BLANK
            }];
            let ids = mount_static_nodes(t, p, &LI);
            t.set_on_click(ids[0], || {});
            ids
        })],
    );
    tree.attach_for_to_anchor(
        Some(ul_id),
        for_anchor,
        move || items.get(),
        |&(k, _)| k,
        move |t, p, item_sig| {
            static LI: [StaticNodeDesc; 1] = [StaticNodeDesc {
                tag: "li",
                ..StaticNodeDesc::BLANK
            }];
            let ids = mount_static_nodes(t, p, &LI);
            t.bind_dynamic_text(ids[0], move || item_sig.get().1.to_string());
            t.set_on_click(ids[0], || {});
            created_ids.borrow_mut().push(ids[0]);
            ids
        },
        None,
    );
}

#[test]
fn control_flow_if_and_for_preserve_order_reuse_keys_and_leak_zero_listeners() {
    let session = ReactiveSession::new();
    session.enter(|| {
        let show = Signal::new(true);
        let items = Signal::new(vec![(1, "Alpha"), (2, "Beta")]);
        static ROOT_AND_ANCHORS: [StaticNodeDesc; 3] = [
            StaticNodeDesc {
                tag: "ul",
                ..StaticNodeDesc::BLANK
            },
            StaticNodeDesc {
                parent_slot: Some(0),
                ..StaticNodeDesc::ANCHOR
            },
            StaticNodeDesc {
                parent_slot: Some(0),
                ..StaticNodeDesc::ANCHOR
            },
        ];
        let mut tree = WebDomTree::with_backend(Box::new(MockDomBackend::new()));
        let created_ids = Rc::new(RefCell::new(Vec::new()));
        let root_ids = mount_static_nodes(&mut tree, None, &ROOT_AND_ANCHORS);
        let (ul_id, if_anchor, for_anchor) = (root_ids[0], root_ids[1], root_ids[2]);
        attach_test_if_and_for(
            &mut tree,
            ul_id,
            if_anchor,
            for_anchor,
            show,
            items,
            Rc::clone(&created_ids),
        );

        assert_eq!(with_mock(&tree, |m| m.active_listener_count()), 3);
        let first_children = with_mock(&tree, |m| m.children_of(ul_id).to_vec());
        assert_eq!(first_children.len(), 5);
        assert_eq!(first_children[1], if_anchor);
        assert_eq!(first_children[4], for_anchor);

        show.set(false);
        let count_before_reorder = created_ids.borrow().len();
        items.set(vec![(2, "Beta-v2"), (1, "Alpha"), (3, "Gamma")]);
        assert_eq!(created_ids.borrow().len(), count_before_reorder + 1);
        assert_eq!(with_mock(&tree, |m| m.active_listener_count()), 3);

        items.set(Vec::new());
        assert_eq!(with_mock(&tree, |m| m.active_listener_count()), 0);
    });
}

#[test]
fn ime_composition_and_wheel_work() {
    let session = ReactiveSession::new();
    session.enter(|| {
        let committed_inputs = Rc::new(RefCell::new(Vec::new()));
        let committed_ref = Rc::clone(&committed_inputs);
        let wheel_vals = Rc::new(RefCell::new(Vec::new()));
        let wheel_ref = Rc::clone(&wheel_vals);
        static NODES: [StaticNodeDesc; 3] = [
            StaticNodeDesc {
                tag: "div",
                ..StaticNodeDesc::BLANK
            },
            StaticNodeDesc {
                parent_slot: Some(0),
                tag: "input",
                is_input: true,
                ..StaticNodeDesc::BLANK
            },
            StaticNodeDesc {
                parent_slot: Some(0),
                tag: "canvas",
                is_canvas: true,
                ..StaticNodeDesc::BLANK
            },
        ];
        let mut tree = WebDomTree::with_backend(Box::new(MockDomBackend::new()));
        let ids = mount_static_nodes(&mut tree, None, &NODES);
        tree.set_on_input(ids[1], move |s: String| committed_ref.borrow_mut().push(s));
        tree.set_on_wheel(
            ids[2],
            WheelHandler::new(move |ev: WheelEvent| {
                wheel_ref.borrow_mut().push(ev.delta_y);
                prevent_default_wheel();
                true
            }),
        );
        with_mock_mut(&mut tree, |m| {
            m.dispatch_ime_composition_input(ids[1], "ㅎ", "한");
            assert!(m.dispatch_wheel(
                ids[2],
                WheelEvent {
                    delta_y: 120.0,
                    ..WheelEvent::default()
                }
            ));
        });
        assert_eq!(&*committed_inputs.borrow(), &["한".to_string()]);
        assert_eq!(&*wheel_vals.borrow(), &[120.0]);
    });
}

#[test]
fn canvas_2d_and_slot_lifecycle_hooks_work() {
    let session = ReactiveSession::new();
    session.enter(|| {
        let radius_sig = Signal::new(10.0_f32);
        let slot_events = Rc::new(RefCell::new(Vec::new()));
        let (slot_m, slot_u) = (Rc::clone(&slot_events), Rc::clone(&slot_events));
        let show_slot = Signal::new(true);
        static NODES: [StaticNodeDesc; 3] = [
            StaticNodeDesc { tag: "div", ..StaticNodeDesc::BLANK },
            StaticNodeDesc { parent_slot: Some(0), tag: "canvas", is_canvas: true, ..StaticNodeDesc::BLANK },
            StaticNodeDesc { parent_slot: Some(0), ..StaticNodeDesc::ANCHOR },
        ];
        let mut tree = WebDomTree::with_backend(Box::new(MockDomBackend::new()));
        tree.register_slot_host(
            "dev-3d-viewport-slot",
            move |_| slot_m.borrow_mut().push("mount"),
            move |_| slot_u.borrow_mut().push("unmount"),
        );
        let ids = mount_static_nodes(&mut tree, None, &NODES);
        tree.set_canvas_draw(ids[1], CanvasDraw::new(move |ctx, _| {
            ctx.fill_circle(50.0, 50.0, radius_sig.get(), Color::BLACK);
        }));
        tree.attach_if_to_anchor(Some(ids[0]), ids[2], move || show_slot.get().then_some(0), vec![Rc::new(|t, p| {
            static SLOT: [StaticNodeDesc; 1] = [StaticNodeDesc { id: Some("dev-3d-viewport-slot"), ..StaticNodeDesc::BLANK }];
            mount_static_nodes(t, p, &SLOT)
        })]);
        radius_sig.set(24.0);
        with_mock(&tree, |m| {
            let cmds = &m.node(ids[1]).unwrap().canvas_commands;
            assert!(matches!(cmds.first(), Some(CanvasCommand::FillCircle { radius, .. }) if (*radius - 24.0).abs() < 1e-4));
        });
        show_slot.set(false);
        assert_eq!(&*slot_events.borrow(), &["mount", "unmount"]);
    });
}
