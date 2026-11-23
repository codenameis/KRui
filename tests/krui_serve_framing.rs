//! Extended `B-fix-3` verification tests for `krui serve` TCP framing and device frame simulation:
//! - Fragmented/coalesced TCP frames, initial overlay replay on accept, and IR version mismatch detection
//! - Device frame screen layout size, DPI scaling, safe-area clipping, and pointer coordinate mapping

#![cfg(feature = "preview")]
#![forbid(unsafe_code)]

use krui::*;
use krui_preview::{
    compile_krui_source, decorate_preview_paint_scene, mount_compiled_document_with_registry,
    DeviceFramePreset, PreviewCommand, PreviewOverlayKind, PreviewResponse, PreviewTcpClient,
    PreviewTcpServer,
};
use std::rc::Rc;
use std::time::{Duration, Instant};

fn wait_for_responses(server: &mut PreviewTcpServer, min_count: usize) -> Vec<PreviewResponse> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut collected = Vec::new();
    while Instant::now() < deadline && collected.len() < min_count {
        collected.extend(server.poll_responses());
        if collected.len() < min_count {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    collected
}

#[test]
fn b_fix_3_fragmented_and_coalesced_tcp_frames_version_mismatch_and_initial_overlay_replay() {
    verify_fragmented_and_coalesced_tcp_frames();
    verify_initial_overlay_replay_and_version_mismatch();
}

fn verify_fragmented_and_coalesced_tcp_frames() {
    use krui_preview::{encode_length_prefixed_vec, FramedTcpConnection};
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind raw listener");
    let addr = listener.local_addr().unwrap();
    let mut raw_writer = TcpStream::connect(addr).expect("connect raw writer");
    let (accepted, _) = listener.accept().expect("accept reader");
    let mut framed_reader = FramedTcpConnection::new(accepted);

    let cmd1 = PreviewCommand::SetOverlay {
        kind: PreviewOverlayKind::Fps,
        enabled: true,
    };
    let cmd2 = PreviewCommand::SetOverlay {
        kind: PreviewOverlayKind::Hit,
        enabled: true,
    };
    let bytes1 = encode_length_prefixed_vec(&cmd1).unwrap();
    let bytes2 = encode_length_prefixed_vec(&cmd2).unwrap();

    raw_writer.write_all(&bytes1[..2]).unwrap();
    raw_writer.flush().unwrap();
    std::thread::sleep(Duration::from_millis(10));
    assert_eq!(framed_reader.try_recv::<PreviewCommand>().unwrap(), None);

    raw_writer.write_all(&bytes1[2..5]).unwrap();
    raw_writer.flush().unwrap();
    std::thread::sleep(Duration::from_millis(10));
    assert_eq!(framed_reader.try_recv::<PreviewCommand>().unwrap(), None);

    let mut coalesced = Vec::new();
    coalesced.extend_from_slice(&bytes1[5..]);
    coalesced.extend_from_slice(&bytes2);
    raw_writer.write_all(&coalesced).unwrap();
    raw_writer.flush().unwrap();
    std::thread::sleep(Duration::from_millis(15));

    assert_eq!(
        framed_reader.try_recv::<PreviewCommand>().unwrap(),
        Some(cmd1)
    );
    assert_eq!(
        framed_reader.try_recv::<PreviewCommand>().unwrap(),
        Some(cmd2)
    );
    assert_eq!(framed_reader.try_recv::<PreviewCommand>().unwrap(), None);
}

fn verify_initial_overlay_replay_and_version_mismatch() {
    let mut server = PreviewTcpServer::bind("127.0.0.1:0").expect("bind server");
    server.set_device_frame(Some(DeviceFramePreset::Pixel8));
    server.broadcast(&PreviewCommand::SetOverlay {
        kind: PreviewOverlayKind::Relayout,
        enabled: true,
    });
    server.broadcast(&PreviewCommand::SetOverlay {
        kind: PreviewOverlayKind::Hit,
        enabled: true,
    });

    let srv_addr = server.local_addr().to_string();
    let mut client = PreviewTcpClient::connect(&srv_addr).expect("connect client after broadcast");
    std::thread::sleep(Duration::from_millis(15));
    server.accept_pending();
    std::thread::sleep(Duration::from_millis(15));

    let mut tree = ViewTree::new();
    let mut err = None;
    let mut reloads = 0;
    assert!(client.poll(&mut tree, &mut err, &mut reloads));
    assert_eq!(client.device_frame(), Some(DeviceFramePreset::Pixel8));
    assert!(client.overlays().relayout && client.overlays().hit && !client.overlays().fps);
    let _ = wait_for_responses(&mut server, 3);

    let doc = compile_krui_source("<div><span>버전 테스트</span></div>").unwrap();
    let mismatched_ir = preview::encode_compiled_document_with_version(&doc, 999).unwrap();
    server.broadcast(&PreviewCommand::Reload(mismatched_ir));
    std::thread::sleep(Duration::from_millis(15));
    let _ = client.poll(&mut tree, &mut err, &mut reloads);
    let resps = wait_for_responses(&mut server, 1);
    assert!(resps.iter().any(|r| matches!(
        r,
        PreviewResponse::VersionMismatch {
            expected: preview::IR_FORMAT_VERSION,
            found: 999
        }
    )));
}

#[test]
fn b_fix_3_device_frame_simulates_screen_layout_dpi_safe_area_clip_and_pointer_mapping() {
    let doc = compile_krui_source(
        r#"
        <div style="display: flex; flex-direction: column; width: 393px; height: 900px">
            <button id="top-btn" width="160" height="44">상단 버튼</button>
        </div>
        "#,
    )
    .unwrap();
    let reg: RegistryRef = Rc::new(ScopeRegistry::new());
    let mut tree = ViewTree::new();
    mount_compiled_document_with_registry(&mut tree, &doc, reg);

    let preset = DeviceFramePreset::Iphone15;
    tree.set_scale_factor(preset.scale_factor());
    let (layout_w, layout_h) = preset.safe_area_layout_size();
    assert_eq!((layout_w, layout_h), (393, 852 - 59 - 34));

    let (mut scene, _) = tree.build_paint_scene(layout_w, layout_h);
    let app_prim_count = scene.primitives.len();
    let mut text_engine = TextEngine::new();
    decorate_preview_paint_scene(
        &mut scene,
        &mut text_engine,
        &[],
        None,
        preview::PreviewOverlayFlags::default(),
        Some(preset),
        1,
        300,
    );

    assert_device_frame_clips_primitives(&scene, preset, app_prim_count);
    assert_eq!(preset.map_window_to_screen_point(10.0, 10.0), None);
    let (ox, oy) = preset.content_origin();
    assert_eq!(
        preset.map_window_to_screen_point(ox + 20.0, oy + 15.0),
        Some((20.0, 15.0))
    );
}

fn assert_device_frame_clips_primitives(
    scene: &PaintScene,
    preset: DeviceFramePreset,
    app_prim_count: usize,
) {
    assert_eq!((scene.width, scene.height), preset.outer_window_size());
    assert!((scene.scale_factor - 3.0).abs() < 1e-4);
    let bezel = preset.bezel_padding();
    let (sw, sh) = preset.screen_size();
    for prim in &scene.primitives[..app_prim_count] {
        let clip = match prim {
            DrawPrimitive::StyledBox { clip, .. }
            | DrawPrimitive::TextGlyphs { clip, .. }
            | DrawPrimitive::CanvasBatch { clip, .. }
            | DrawPrimitive::Image { clip, .. }
            | DrawPrimitive::PushLayer { clip, .. } => {
                clip.expect("device frame must clip app primitives")
            }
            DrawPrimitive::PopLayer => continue,
        };
        assert!(clip.x >= bezel && clip.y >= bezel);
        assert!(clip.x + clip.width <= bezel + sw as f32 + 0.01);
        assert!(clip.y + clip.height <= bezel + sh as f32 + 0.01);
    }
}
