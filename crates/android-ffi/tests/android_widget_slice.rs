//! End-to-end tests for Android home-screen widget snapshot rendering, hotspot extraction,
//! tap action dispatch -> pixel diff, dark-mode theme switch, and handle registry lifecycle.

#![forbid(unsafe_code)]

use krui_android_ffi::{
    create_registered_widget_host, destroy_registered_widget_host, with_registered_widget_host,
    AndroidWidgetHost, WIDGET_ACTION_INCREMENT, WIDGET_ACTION_RESET,
};

#[test]
fn android_widget_renders_hotspots_and_updates_pixels_on_action_and_dark_mode() {
    let mut host = AndroidWidgetHost::new_default_widget(640, 320, 2.0, false);
    assert_eq!(host.revision(), 0);
    assert_eq!(host.packed_snapshot_size(), 0);

    let snap1 = host.render_snapshot().expect("initial widget render");
    assert_eq!((snap1.width_px, snap1.height_px), (640, 320));
    assert_eq!((snap1.logical_width, snap1.logical_height), (320, 160));
    assert_eq!(snap1.revision, 1);
    assert_eq!(snap1.rgba8.len(), 640 * 320 * 4);
    assert_eq!(snap1.hotspots.len(), 2);
    assert_eq!(snap1.hotspots[0].action_id, WIDGET_ACTION_INCREMENT);
    assert_eq!(snap1.hotspots[1].action_id, WIDGET_ACTION_RESET);
    assert!(
        snap1.hotspots[0].x + snap1.hotspots[0].width <= snap1.hotspots[1].x,
        "expected non-overlapping row buttons, got {:?}",
        snap1.hotspots
    );

    let base_pixels = snap1.rgba8.clone();
    let distinct_first_px: std::collections::HashSet<[u8; 4]> = base_pixels
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .step_by(17)
        .collect();
    assert!(
        distinct_first_px.len() >= 3,
        "widget bitmap must render non-blank styled surface, borders, buttons, and text"
    );

    assert!(host.dispatch_action(WIDGET_ACTION_INCREMENT));
    assert_eq!(host.revision(), 2);
    let inc_pixels = host.last_snapshot().unwrap().rgba8.clone();
    assert_ne!(
        inc_pixels, base_pixels,
        "incrementing counter signal must change rendered widget pixels"
    );

    assert!(host.dispatch_action(WIDGET_ACTION_RESET));
    assert_eq!(host.revision(), 3);
    assert_eq!(
        host.last_snapshot().unwrap().rgba8,
        base_pixels,
        "resetting counter back to 0 must reproduce identical widget pixels"
    );

    assert!(host.resize(640, 320, 2.0, true));
    let dark_pixels = host.render_snapshot().unwrap().rgba8.clone();
    assert_ne!(
        dark_pixels, base_pixels,
        "dark_mode=true resize must re-render dark surface palette"
    );
}

#[test]
fn widget_handle_registry_tracks_lifecycle_and_rejects_stale_ids() {
    let wid = create_registered_widget_host(480, 240, 1.5, false);
    assert!(wid > 0);
    assert_eq!(
        with_registered_widget_host(wid, |w| {
            assert!(w.render_snapshot().is_some());
            w.hotspot_count()
        }),
        Some(2)
    );
    let argb_len = with_registered_widget_host(wid, |w| {
        w.with_snapshot_argb(|_w, _h, px| px.len()).unwrap()
    });
    assert_eq!(argb_len, Some(480 * 240));
    assert!(destroy_registered_widget_host(wid));
    assert!(!destroy_registered_widget_host(wid));
    assert!(with_registered_widget_host(wid, |_| ()).is_none());
}
