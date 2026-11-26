#![forbid(unsafe_code)]

use krui_style::ColorScheme;
use krui_stylo_bridge::{
    run_taffy_layout, DeviceConfig, DomElementState, ElementAttr, PointerKind, PseudoElementKind,
    StyleSheetOrigin, StyloDamage, StyloDomMutation, StyloNodeId, StyloStyleEngine,
    TaffyLayoutHost,
};
use std::collections::HashMap;
use taffy::{AvailableSpace, Cache, Layout, NodeId, RunMode, Size};

struct TestLayoutHost {
    nodes: Vec<TestLayoutNode>,
}

struct TestLayoutNode {
    stylo: StyloNodeId,
    children: Vec<NodeId>,
    cache: Cache,
    unrounded: Layout,
    final_layout: Layout,
}

impl TestLayoutHost {
    fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    fn alloc(&mut self, stylo: StyloNodeId) -> NodeId {
        let id = NodeId::from(self.nodes.len() as u64);
        self.nodes.push(TestLayoutNode {
            stylo,
            children: Vec::new(),
            cache: Cache::new(),
            unrounded: Layout::new(),
            final_layout: Layout::new(),
        });
        id
    }

    fn link(&mut self, parent: NodeId, child: NodeId) {
        self.nodes[usize::from(parent)].children.push(child);
    }

    fn rect(&self, id: NodeId) -> (f32, f32, f32, f32) {
        let l = self.nodes[usize::from(id)].final_layout;
        (l.location.x, l.location.y, l.size.width, l.size.height)
    }
}

impl TaffyLayoutHost for TestLayoutHost {
    fn stylo_node_for_layout(&self, node: NodeId) -> Option<StyloNodeId> {
        self.nodes.get(usize::from(node)).map(|n| n.stylo)
    }
    fn layout_children(&self, node: NodeId) -> &[NodeId] {
        &self.nodes[usize::from(node)].children
    }
    fn layout_cache_mut(&mut self, node: NodeId) -> &mut Cache {
        &mut self.nodes[usize::from(node)].cache
    }
    fn unrounded_layout(&self, node: NodeId) -> &Layout {
        &self.nodes[usize::from(node)].unrounded
    }
    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Layout) {
        self.nodes[usize::from(node)].unrounded = *layout;
    }
    fn set_final_layout(&mut self, node: NodeId, layout: &Layout) {
        self.nodes[usize::from(node)].final_layout = *layout;
    }
    fn measure_leaf(
        &mut self,
        _: NodeId,
        known: Size<Option<f32>>,
        _: Size<AvailableSpace>,
        _: RunMode,
    ) -> Size<f32> {
        Size {
            width: known.width.unwrap_or(0.0),
            height: known.height.unwrap_or(0.0),
        }
    }
}

fn new_engine(unstyled: bool) -> StyloStyleEngine {
    StyloStyleEngine::new(DeviceConfig {
        viewport_width: 1000.0,
        viewport_height: 800.0,
        dpr: 1.0,
        color_scheme: ColorScheme::Light,
        pointer: PointerKind::FineHover,
        unstyled,
    })
}

#[test]
fn generational_slot_recycling_prevents_leaks_across_100_cycles() {
    let mut engine = new_engine(true);
    let root = engine.alloc_element("div");
    engine.set_root_element(root);
    engine.restyle();
    let baseline = engine.active_slot_count();

    for i in 0..100 {
        let card = engine.alloc_element("div");
        let label = engine.alloc_element("span");
        engine.apply_mutation(StyloDomMutation::AppendChild {
            parent: root,
            child: card,
        });
        engine.apply_mutation(StyloDomMutation::AppendChild {
            parent: card,
            child: label,
        });
        engine.apply_mutation(StyloDomMutation::SetClasses {
            node: card,
            classes: vec![format!("card-{i}")],
        });
        engine.restyle();
        engine.apply_mutation(StyloDomMutation::DisposeSubtree(card));
        assert!(
            engine.paint_style(card).is_none(),
            "stale handle must return None"
        );
        assert!(
            engine.paint_style(label).is_none(),
            "child stale handle must return None"
        );
    }

    assert_eq!(engine.active_slot_count(), baseline);
}

#[test]
fn display_none_toggle_emits_relayout_damage_both_ways() {
    let mut engine = new_engine(true);
    let root = engine.alloc_element("div");
    let box_el = engine.alloc_element("div");
    engine.set_root_element(root);
    engine.apply_mutation(StyloDomMutation::AppendChild {
        parent: root,
        child: box_el,
    });
    engine.add_stylesheet(
        StyleSheetOrigin::Author,
        ".panel { width: 100px; height: 40px; } .panel.hidden { display: none; }",
    );
    engine.apply_mutation(StyloDomMutation::SetClasses {
        node: box_el,
        classes: vec!["panel".into()],
    });
    let initial = engine.restyle();
    assert_eq!(initial.nodes.get(&box_el), Some(&StyloDamage::Relayout));

    engine.apply_mutation(StyloDomMutation::SetClasses {
        node: box_el,
        classes: vec!["panel".into(), "hidden".into()],
    });
    let hide = engine.restyle();
    assert_eq!(hide.nodes.get(&box_el), Some(&StyloDamage::Relayout));

    engine.apply_mutation(StyloDomMutation::SetClasses {
        node: box_el,
        classes: vec!["panel".into()],
    });
    let show = engine.restyle();
    assert_eq!(show.nodes.get(&box_el), Some(&StyloDamage::Relayout));
}

#[test]
fn snapshot_invalidation_handles_hover_has_attr_class_and_empty() {
    let mut engine = new_engine(true);
    let root = engine.alloc_element("div");
    let parent = engine.alloc_element("div");
    let child = engine.alloc_element("button");
    engine.set_root_element(root);
    engine.apply_mutation(StyloDomMutation::AppendChild {
        parent: root,
        child: parent,
    });
    engine.apply_mutation(StyloDomMutation::AppendChild { parent, child });
    engine.add_stylesheet(
        StyleSheetOrigin::Author,
        r#"
        [data-theme="dark"] .card { color: rgb(255, 255, 255); }
        .card:has(> [class*="btn"]:hover) { background-color: rgb(10, 20, 30); }
        button:empty { opacity: 0.5; }
        "#,
    );
    engine.apply_mutation(StyloDomMutation::SetAttributes {
        node: root,
        attrs: vec![ElementAttr {
            name: "data-theme".into(),
            value: "dark".into(),
        }],
    });
    engine.apply_mutation(StyloDomMutation::SetClasses {
        node: parent,
        classes: vec!["card".into()],
    });
    engine.apply_mutation(StyloDomMutation::SetClasses {
        node: child,
        classes: vec!["  primary-btn  ".into()],
    });
    engine.restyle();

    let p = engine.paint_style(parent).expect("parent paint");
    assert_eq!(
        (p.text_color.r, p.text_color.g, p.text_color.b),
        (255, 255, 255)
    );
    assert_eq!(engine.paint_style(child).expect("child paint").opacity, 0.5);

    engine.apply_mutation(StyloDomMutation::SetState {
        node: child,
        state: DomElementState::HOVER,
    });
    let dmg = engine.restyle();
    assert_eq!(dmg.nodes.get(&parent), Some(&StyloDamage::Repaint));
    let hovered = engine.paint_style(parent).expect("hovered parent");
    assert_eq!(
        (
            hovered.background_color.r,
            hovered.background_color.g,
            hovered.background_color.b
        ),
        (10, 20, 30)
    );

    let text = engine.alloc_text(true);
    engine.apply_mutation(StyloDomMutation::AppendChild {
        parent: child,
        child: text,
    });
    engine.restyle();
    assert_eq!(
        engine.paint_style(child).expect("non-empty child").opacity,
        1.0
    );
}

#[test]
fn stylesheet_replace_preserves_cascade_order_and_remove_reverts() {
    let mut engine = new_engine(true);
    let root = engine.alloc_element("div");
    engine.set_root_element(root);
    let base = engine.add_stylesheet(StyleSheetOrigin::Author, "div { color: rgb(10, 10, 10); }");
    let override_sheet =
        engine.add_stylesheet(StyleSheetOrigin::Author, "div { color: rgb(200, 0, 0); }");
    engine.restyle();
    assert_eq!(
        engine
            .paint_style(root)
            .expect("override color")
            .text_color
            .r,
        200
    );

    assert!(engine.replace_stylesheet(base, "div { color: rgb(0, 200, 0); }"));
    engine.restyle();
    assert_eq!(
        engine
            .paint_style(root)
            .expect("later sheet still wins")
            .text_color
            .r,
        200
    );

    assert!(engine.remove_stylesheet(override_sheet));
    engine.restyle();
    assert_eq!(
        engine
            .paint_style(root)
            .expect("updated base sheet revealed")
            .text_color
            .g,
        200
    );
}

#[test]
fn insert_before_container_pseudo_inline_and_taffy_calc_work() {
    let mut engine = new_engine(true);
    let root = engine.alloc_element("div");
    let second = engine.alloc_element("span");
    let first = engine.alloc_element("span");
    engine.set_root_element(root);
    engine.apply_mutation(StyloDomMutation::AppendChild {
        parent: root,
        child: second,
    });
    engine.apply_mutation(StyloDomMutation::InsertBefore {
        parent: root,
        child: first,
        before: Some(second),
    });
    engine.add_stylesheet(
        StyleSheetOrigin::Author,
        r#"
        div { display: flex; flex-direction: column; width: 1000px; container-type: inline-size; }
        span { display: block; height: 20px; width: calc(100% - 40px); }
        span:first-child { color: rgb(11, 22, 33); }
        span::before { content: ""; color: rgb(44, 55, 66); }
        @scope (section) to (.boundary) {
            span { opacity: 0.75; }
        }
        @container (min-width: 500px) {
            span { color: rgb(1, 2, 3); }
        }
        @media (min-width: 700px) {
            span.vp { color: rgb(77, 88, 99); }
        }
        "#,
    );
    engine.apply_mutation(StyloDomMutation::SetClasses {
        node: second,
        classes: vec!["vp".into()],
    });
    engine.apply_mutation(StyloDomMutation::SetInlineStyle {
        node: second,
        css: "opacity: 0.25;".into(),
    });
    engine.set_viewport(400.0, 800.0, 1.0);
    engine.restyle();

    let first_paint = engine.paint_style(first).expect("first child");
    assert_eq!(
        (first_paint.text_color.r, first_paint.text_color.g),
        (11, 22)
    );
    assert_eq!(
        engine.paint_style(second).expect("inline style").opacity,
        0.25
    );
    assert_ne!(
        engine
            .paint_style(second)
            .expect("narrow viewport")
            .text_color
            .r,
        77
    );
    assert_eq!(
        engine
            .pseudo_paint_style(first, PseudoElementKind::Before)
            .expect("before pseudo")
            .text_color
            .r,
        44
    );
    assert_taffy_calc_and_media_revalidation(&mut engine, root, first, second);
}

fn assert_taffy_calc_and_media_revalidation(
    engine: &mut StyloStyleEngine,
    root: StyloNodeId,
    first: StyloNodeId,
    second: StyloNodeId,
) {
    let mut host = TestLayoutHost::new();
    let r = host.alloc(root);
    let s1 = host.alloc(first);
    let s2 = host.alloc(second);
    host.link(r, s1);
    host.link(r, s2);
    run_taffy_layout(
        engine,
        &mut host,
        r,
        Size {
            width: AvailableSpace::Definite(1000.0),
            height: AvailableSpace::Definite(800.0),
        },
        true,
    );
    assert_eq!(host.rect(s1), (0.0, 0.0, 960.0, 20.0));
    assert_eq!(host.rect(s2), (0.0, 20.0, 960.0, 20.0));

    // Verify set_container_size activates @container rules before @media overrides at 800px.
    assert!(engine.set_container_size(root, Some(600.0), Some(800.0)));
    engine.restyle();
    let second_cq = engine
        .paint_style(second)
        .expect("container matched second child");
    assert_eq!(
        (
            second_cq.text_color.r,
            second_cq.text_color.g,
            second_cq.text_color.b
        ),
        (1, 2, 3)
    );
    engine.set_viewport(800.0, 800.0, 1.0);
    engine.restyle();
    assert_eq!(
        engine
            .paint_style(second)
            .expect("media viewport matched")
            .text_color
            .r,
        77
    );
    let _map_check: HashMap<StyloNodeId, StyloDamage> = HashMap::new();
}
