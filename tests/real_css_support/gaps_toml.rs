//! Parser and bidirectional ratchet evaluator for `KNOWN_GAPS.toml`.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownGap {
    pub id: String,
    pub stage: String,
    pub reason: String,
    pub targets: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CheckOutcome {
    pub key: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct RatchetSummary {
    pub passed_count: usize,
    pub total_count: usize,
    pub known_gap_failures: Vec<(String, String, String)>,
    pub unexpected_regressions: Vec<(String, String)>,
    pub stale_gaps_now_passing: Vec<String>,
}

impl RatchetSummary {
    pub fn is_clean(&self) -> bool {
        self.unexpected_regressions.is_empty() && self.stale_gaps_now_passing.is_empty()
    }
}

pub fn parse_known_gaps_toml(src: &str) -> Vec<KnownGap> {
    let mut out = Vec::new();
    let mut current: Option<KnownGap> = None;
    for raw_line in src.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line == "[[gap]]" {
            if let Some(prev) = current.take() {
                out.push(prev);
            }
            current = Some(KnownGap {
                id: String::new(),
                stage: String::new(),
                reason: String::new(),
                targets: Vec::new(),
            });
            continue;
        }
        let Some(gap) = current.as_mut() else {
            continue;
        };
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let val = v.trim();
        match key {
            "id" => gap.id = unquote(val),
            "stage" => gap.stage = unquote(val),
            "reason" => gap.reason = unquote(val),
            "targets" => gap.targets = parse_str_array(val),
            _ => {}
        }
    }
    if let Some(last) = current {
        out.push(last);
    }
    out
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').to_string()
}

fn parse_str_array(raw: &str) -> Vec<String> {
    let inner = raw.trim().trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(unquote)
        .collect()
}

pub fn pattern_matches(pattern: &str, key: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix(".*") {
        key == prefix
            || key
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('.'))
    } else if let Some(suffix) = pattern.strip_prefix("*.") {
        key.ends_with(&format!(".{suffix}"))
    } else {
        pattern == key
    }
}

pub fn evaluate_bidirectional_ratchet(
    outcomes: &[CheckOutcome],
    gaps: &[KnownGap],
) -> RatchetSummary {
    let total_count = outcomes.len();
    let passed_count = outcomes.iter().filter(|o| o.passed).count();
    let mut gap_hit_counts = vec![0_usize; gaps.len()];
    let mut known_gap_failures = Vec::new();
    let mut unexpected_regressions = Vec::new();

    for o in outcomes {
        if o.passed {
            continue;
        }
        let mut matched_idx = None;
        for (idx, gap) in gaps.iter().enumerate() {
            if gap.targets.iter().any(|pat| pattern_matches(pat, &o.key)) {
                gap_hit_counts[idx] += 1;
                if matched_idx.is_none() {
                    matched_idx = Some(idx);
                }
            }
        }
        if let Some(idx) = matched_idx {
            known_gap_failures.push((
                gaps[idx].id.clone(),
                gaps[idx].stage.clone(),
                format!("{}: {}", o.key, o.detail),
            ));
        } else {
            unexpected_regressions.push((o.key.clone(), o.detail.clone()));
        }
    }

    let mut stale_gaps_now_passing = Vec::new();
    for (idx, gap) in gaps.iter().enumerate() {
        if gap_hit_counts[idx] == 0 {
            stale_gaps_now_passing.push(format!("{} (stage {})", gap.id, gap.stage));
        }
    }

    RatchetSummary {
        passed_count,
        total_count,
        known_gap_failures,
        unexpected_regressions,
        stale_gaps_now_passing,
    }
}
