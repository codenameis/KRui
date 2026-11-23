//! Real external CSS framework conformance harness (`@tailwindcss/cli@4.3.5` + Chromium oracle)
//! with a bidirectional `KNOWN_GAPS.toml` ratchet.

#![forbid(unsafe_code)]

mod real_css_support;

use real_css_support::gaps_toml::{
    evaluate_bidirectional_ratchet, parse_known_gaps_toml, CheckOutcome, KnownGap,
};
use real_css_support::json_oracle::parse_oracle_json;
use real_css_support::probes::run_all_probe_checks;
use real_css_support::tailwind_eval::evaluate_tailwind_suite;
use std::path::Path;

#[test]
fn real_tailwind_v4_and_w3c_conformance_bidirectional_ratchet() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fix_dir = root.join("tests/fixtures/real_css");
    let tw_dir = fix_dir.join("tailwind_v4");

    let oracle_text =
        std::fs::read_to_string(tw_dir.join("oracle.json")).expect("read oracle.json");
    let normal_css =
        std::fs::read_to_string(tw_dir.join("tailwind_v4.css")).expect("read tailwind_v4.css");
    let opt_css = std::fs::read_to_string(tw_dir.join("tailwind_v4.optimize.css"))
        .expect("read tailwind_v4.optimize.css");
    let gaps_text =
        std::fs::read_to_string(fix_dir.join("KNOWN_GAPS.toml")).expect("read KNOWN_GAPS.toml");

    let oracle = parse_oracle_json(&oracle_text);
    let gaps = parse_known_gaps_toml(&gaps_text);

    let mut outcomes = Vec::new();
    run_all_probe_checks(&oracle.probes, &mut outcomes);
    evaluate_tailwind_suite(
        "tailwind_normal",
        &normal_css,
        oracle.viewport,
        &oracle.tailwind_normal,
        &mut outcomes,
    );
    evaluate_tailwind_suite(
        "tailwind_optimize",
        &opt_css,
        oracle.viewport,
        &oracle.tailwind_optimize,
        &mut outcomes,
    );

    let summary = evaluate_bidirectional_ratchet(&outcomes, &gaps);
    let pct = if summary.total_count == 0 {
        0.0
    } else {
        (summary.passed_count as f64 / summary.total_count as f64) * 100.0
    };
    println!(
        "속성 일치율 {}/{} ({pct:.1}%) | 알려진 갭 {}건 | 회귀 {}건 | 해소된 갭 {}건",
        summary.passed_count,
        summary.total_count,
        summary.known_gap_failures.len(),
        summary.unexpected_regressions.len(),
        summary.stale_gaps_now_passing.len()
    );

    assert!(
        summary.is_clean(),
        "Real CSS conformance ratchet violation!\n  Unexpected regressions (not in KNOWN_GAPS.toml): {:#?}\n  Stale gaps now passing (REMOVE from KNOWN_GAPS.toml!): {:#?}",
        summary.unexpected_regressions,
        summary.stale_gaps_now_passing
    );
}

#[test]
fn bidirectional_ratchet_rejects_unlisted_regressions_and_stale_fixed_gaps() {
    let gaps = vec![
        KnownGap {
            id: "gap_still_failing".into(),
            stage: "E2".into(),
            reason: "still broken".into(),
            targets: vec!["suite.node.color".into()],
        },
        KnownGap {
            id: "gap_already_fixed".into(),
            stage: "E1".into(),
            reason: "fixed now".into(),
            targets: vec!["suite.node.rect_y".into()],
        },
    ];
    let outcomes = vec![
        CheckOutcome {
            key: "suite.node.color".into(),
            passed: false,
            detail: "mismatch".into(),
        },
        CheckOutcome {
            key: "suite.node.rect_y".into(),
            passed: true,
            detail: "ok".into(),
        },
        CheckOutcome {
            key: "suite.node.unlisted_prop".into(),
            passed: false,
            detail: "surprise regression".into(),
        },
    ];
    let summary = evaluate_bidirectional_ratchet(&outcomes, &gaps);
    assert!(!summary.is_clean());
    assert_eq!(summary.unexpected_regressions.len(), 1);
    assert_eq!(
        summary.unexpected_regressions[0].0,
        "suite.node.unlisted_prop"
    );
    assert_eq!(summary.stale_gaps_now_passing.len(), 1);
    assert!(summary.stale_gaps_now_passing[0].contains("gap_already_fixed"));
}
