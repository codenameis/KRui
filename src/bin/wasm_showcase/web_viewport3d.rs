//! Lazy WebGPU / WebGL2 3D teapot renderer mounted into `#dev-3d-viewport-slot` under `web-gpu-3d`.

#[path = "web_viewport3d_gpu.rs"]
mod gpu;
#[path = "../../../examples/component_gallery/viewport3d/mesh_obj.rs"]
mod mesh_obj;

use crate::controller::viewport3d_ctrl::StudioController;
use crate::controller::GalleryController;
use gpu::{init_wgpu_state, WebGpu3dState};
use krui::{Effect, NodeId, ReactiveSession, ViewTree};
use mesh_obj::{load_shared_open_model, LoadedMesh, SHARED_OPEN_MODEL_NAME};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

struct SlotSession {
    alive: Rc<Cell<bool>>,
    gpu: Rc<RefCell<Option<WebGpu3dState>>>,
}

/// Registers lazy mount/unmount hooks for `#dev-3d-viewport-slot` on `tree`.
pub fn register(tree: &mut ViewTree, controller: &GalleryController) {
    let session = ReactiveSession::current();
    let sessions: Rc<RefCell<Option<SlotSession>>> = Rc::new(RefCell::new(None));
    let (sessions_mount, sessions_unmount) = (Rc::clone(&sessions), Rc::clone(&sessions));
    let tree_for_mount = tree.clone();
    let (ctrl_mount, ctrl_unmount) = (controller.clone(), controller.clone());
    let (sess_mount, sess_unmount) = (session.clone(), session);

    tree.register_slot_host(
        "dev-3d-viewport-slot",
        move |node_id: NodeId| {
            if let Some(slot_el) = tree_for_mount.browser_element(node_id) {
                mount_3d_slot(&slot_el, &ctrl_mount, &sessions_mount, &sess_mount);
            }
        },
        move |_node_id: NodeId| {
            if let Some(sess) = sessions_unmount.borrow_mut().take() {
                sess.alive.set(false);
                *sess.gpu.borrow_mut() = None;
            }
            sess_unmount.enter(|| {
                ctrl_unmount
                    .w4
                    .studio
                    .ecs_telemetry
                    .set("WebGPU / WebGL2 언마운트됨 (유휴 GPU 0%)".to_string());
            });
        },
    );
    bind_camera_render_effect(controller, &sessions);
}

fn bind_camera_render_effect(
    controller: &GalleryController,
    sessions: &Rc<RefCell<Option<SlotSession>>>,
) {
    let studio = controller.w4.studio.clone();
    let sessions_ref = Rc::clone(sessions);
    let _ = Effect::new(move || {
        let yaw = studio.orbit_yaw.get();
        let pitch = studio.orbit_pitch.get();
        let dist = studio.orbit_dist.get();
        let scale = studio.model_scale.get();
        if let Some(sess) = sessions_ref.borrow().as_ref() {
            if let Some(gpu) = sess.gpu.borrow_mut().as_mut() {
                gpu.render(yaw, pitch, dist, scale);
            }
        }
    });
}

fn mount_3d_slot(
    slot_el: &web_sys::Element,
    controller: &GalleryController,
    sessions: &Rc<RefCell<Option<SlotSession>>>,
    session: &ReactiveSession,
) {
    if let Some(prev) = sessions.borrow_mut().take() {
        prev.alive.set(false);
        *prev.gpu.borrow_mut() = None;
    }
    let Some(canvas) = create_slot_canvas(slot_el) else {
        return;
    };
    let alive = Rc::new(Cell::new(true));
    let gpu_slot = Rc::new(RefCell::new(None));
    *sessions.borrow_mut() = Some(SlotSession {
        alive: Rc::clone(&alive),
        gpu: Rc::clone(&gpu_slot),
    });

    let mesh = load_shared_open_model();
    let (vcount, tcount) = (mesh.vertices.len(), mesh.indices.len() / 3);
    session.enter(|| {
        controller.w4.studio.model_stats.set(format!(
            "Model: {SHARED_OPEN_MODEL_NAME} (Utah Teapot · {vcount}v / {tcount} tris)"
        ));
        controller
            .w4
            .studio
            .ecs_telemetry
            .set("WebGPU / WebGL2 어댑터 초기화 중...".to_string());
    });
    spawn_gpu_mount(
        canvas,
        mesh,
        alive,
        gpu_slot,
        controller.w4.studio.clone(),
        session.clone(),
    );
}

fn create_slot_canvas(slot_el: &web_sys::Element) -> Option<web_sys::HtmlCanvasElement> {
    let doc = web_sys::window()?.document()?;
    let canvas = doc
        .create_element("canvas")
        .ok()?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .ok()?;
    let _ = canvas.set_attribute("style", "width:100%;height:100%;display:block;");
    slot_el.set_inner_html("");
    let _ = slot_el.append_child(&canvas);
    Some(canvas)
}

fn spawn_gpu_mount(
    canvas: web_sys::HtmlCanvasElement,
    mesh: LoadedMesh,
    alive: Rc<Cell<bool>>,
    gpu_slot: Rc<RefCell<Option<WebGpu3dState>>>,
    studio: StudioController,
    sess_async: ReactiveSession,
) {
    wasm_bindgen_futures::spawn_local(async move {
        let Some((mut state, backend_label)) = init_wgpu_state(canvas, &mesh).await else {
            if alive.get() {
                sess_async.enter(|| {
                    studio
                        .ecs_telemetry
                        .set("3D GPU 컨텍스트 불가 (WebGPU/WebGL2 미지원)".to_string());
                });
            }
            return;
        };
        if !alive.get() {
            return;
        }
        sess_async.enter(|| {
            studio
                .ecs_telemetry
                .set(format!("{backend_label} 활성 (Hardware Depth24Plus)"));
        });
        attach_resize_observer(&mut state, &gpu_slot, &studio, &sess_async);
        sess_async.enter(|| {
            state.render(
                studio.orbit_yaw.get_untracked(),
                studio.orbit_pitch.get_untracked(),
                studio.orbit_dist.get_untracked(),
                studio.model_scale.get_untracked(),
            );
        });
        *gpu_slot.borrow_mut() = Some(state);
    });
}

fn attach_resize_observer(
    state: &mut WebGpu3dState,
    gpu_slot: &Rc<RefCell<Option<WebGpu3dState>>>,
    studio: &StudioController,
    session: &ReactiveSession,
) {
    let gpu_ref = Rc::clone(gpu_slot);
    let studio_ref = studio.clone();
    let sess_ref = session.clone();
    let cb = Closure::wrap(Box::new(move |_: js_sys::Array| {
        if let Some(gpu) = gpu_ref.borrow_mut().as_mut() {
            gpu.resize_to_canvas();
            sess_ref.enter(|| {
                gpu.render(
                    studio_ref.orbit_yaw.get_untracked(),
                    studio_ref.orbit_pitch.get_untracked(),
                    studio_ref.orbit_dist.get_untracked(),
                    studio_ref.model_scale.get_untracked(),
                );
            });
        }
    }) as Box<dyn FnMut(js_sys::Array)>);
    if let Ok(obs) = web_sys::ResizeObserver::new(cb.as_ref().unchecked_ref()) {
        obs.observe(state.canvas.as_ref());
        state.observer = Some(obs);
        state._observer_cb = Some(cb);
    }
}
