#![forbid(unsafe_code)]

use krui_style::ColorScheme;
use krui_stylo_bridge::{
    run_taffy_layout, DeviceConfig, PointerKind, StyleSheetOrigin, StyloDomMutation, StyloNodeId,
    StyloStyleEngine, TaffyLayoutHost,
};
use taffy::{AvailableSpace, Cache, Layout, NodeId, RunMode, Size};

struct LeafMeasureNode {
    stylo: StyloNodeId,
    children: Vec<NodeId>,
    cache: Cache,
    unrounded: Layout,
    final_layout: Layout,
    measured: Size<f32>,
}

struct LeafMeasureHost {
    nodes: Vec<LeafMeasureNode>,
}

impl LeafMeasureHost {
    fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    fn alloc(&mut self, stylo: StyloNodeId, measured: Size<f32>) -> NodeId {
        let id = NodeId::from(self.nodes.len() as u64);
        self.nodes.push(LeafMeasureNode {
            stylo,
            children: Vec::new(),
            cache: Cache::new(),
            unrounded: Layout::new(),
            final_layout: Layout::new(),
            measured,
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

impl TaffyLayoutHost for LeafMeasureHost {
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
        node: NodeId,
        known: Size<Option<f32>>,
        _: Size<AvailableSpace>,
        _: RunMode,
    ) -> Size<f32> {
        let m = self.nodes[usize::from(node)].measured;
        Size {
            width: known.width.unwrap_or(m.width),
            height: known.height.unwrap_or(m.height),
        }
    }
}

#[test]
fn leaf_with_absolute_child_measures_intrinsic_size_and_positions_oof() {
    let mut engine = StyloStyleEngine::new(DeviceConfig {
        viewport_width: 800.0,
        viewport_height: 600.0,
        dpr: 1.0,
        color_scheme: ColorScheme::Light,
        pointer: PointerKind::FineHover,
        unstyled: true,
    });
    let root = engine.alloc_element("div");
    let button = engine.alloc_element("button");
    let overlay = engine.alloc_element("span");
    engine.set_root_element(root);
    engine.apply_mutation(StyloDomMutation::AppendChild {
        parent: root,
        child: button,
    });
    engine.apply_mutation(StyloDomMutation::AppendChild {
        parent: button,
        child: overlay,
    });
    engine.add_stylesheet(
        StyleSheetOrigin::Author,
        r#"
        div { display: flex; flex-direction: row; align-items: flex-start; }
        button { position: relative; padding: 9px 18px; border: 1px solid #000; }
        span { position: absolute; inset: 0; }
        "#,
    );
    engine.restyle();

    let mut host = LeafMeasureHost::new();
    let r = host.alloc(root, Size::ZERO);
    let btn = host.alloc(
        button,
        Size {
            width: 24.0,
            height: 12.0,
        },
    );
    let ov = host.alloc(overlay, Size::ZERO);
    host.link(r, btn);
    host.link(btn, ov);

    run_taffy_layout(
        &engine,
        &mut host,
        r,
        Size {
            width: AvailableSpace::Definite(800.0),
            height: AvailableSpace::Definite(600.0),
        },
        true,
    );

    assert_eq!(host.rect(btn), (0.0, 0.0, 62.0, 32.0));
    assert_eq!(host.rect(ov), (1.0, 1.0, 60.0, 30.0));
}
