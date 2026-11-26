//! `DRM_IOCTL_MODE_DIRTYFB` damage-rect union/split planner tailored to `hx8357d` (`drm_mipi_dbi`).
//!
//! Because mainline `drm_mipi_dbi` collapses all rects passed in a single `DIRTYFB` ioctl into
//! one enclosing bounding box over SPI, nearby damage rects are merged while distant small rects
//! are split into separate `DIRTYFB` commits whenever union area exceeds `1.5x` the sum of areas.

use krui_style::DamageRect;

/// Union-bounding-box bloat threshold (`15` => `1.5x` individual rect area sum).
pub const UNION_BLOAT_SPLIT_RATIO_X10: u64 = 15;

/// Output of [`plan_dirtyfb_commits`] describing sequential `DIRTYFB` batches for one frame.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DrmCommitPlan {
    /// Non-empty, viewport-clamped rectangles submitted as individual `DIRTYFB` commits.
    pub commits: Vec<DamageRect>,
}

/// Groups damaged rectangles into bounded `DIRTYFB` commits for `hx8357d` SPI transmission.
///
/// Nearby or overlapping rectangles merge into a single commit when their union bounding box
/// satisfies `union_area * 10 <= (a_area + b_area) * split_ratio_x10`; distant rectangles stay
/// separated so a tiny badge update does not force an entire SPI panel flush.
pub fn plan_dirtyfb_commits(
    rects: &[DamageRect],
    viewport_w: u32,
    viewport_h: u32,
    split_ratio_x10: u64,
) -> DrmCommitPlan {
    let ratio = split_ratio_x10.max(10);
    let mut clusters: Vec<DamageRect> = Vec::with_capacity(rects.len().min(16));
    for &raw in rects {
        let Some(clamped) = clamp_rect(raw, viewport_w, viewport_h) else {
            continue;
        };
        if let Some(target_idx) = find_merge_cluster(&clusters, clamped, ratio) {
            clusters[target_idx] = union_rect(clusters[target_idx], clamped);
        } else {
            clusters.push(clamped);
        }
    }
    DrmCommitPlan { commits: clusters }
}

fn find_merge_cluster(clusters: &[DamageRect], rect: DamageRect, ratio_x10: u64) -> Option<usize> {
    let rect_area = rect.area();
    let mut best: Option<(u64, usize)> = None;
    for (idx, &cluster) in clusters.iter().enumerate() {
        let union = union_rect(cluster, rect);
        let union_area = union.area();
        let sum_area = cluster.area().saturating_add(rect_area);
        let fits = union_area.saturating_mul(10) <= sum_area.saturating_mul(ratio_x10);
        if fits && best.is_none_or(|(best_area, _)| union_area < best_area) {
            best = Some((union_area, idx));
        }
    }
    best.map(|(_, idx)| idx)
}

fn clamp_rect(r: DamageRect, vw: u32, vh: u32) -> Option<DamageRect> {
    if vw == 0 || vh == 0 || r.x >= vw || r.y >= vh || r.width == 0 || r.height == 0 {
        return None;
    }
    let max_x = r.x.saturating_add(r.width).min(vw);
    let max_y = r.y.saturating_add(r.height).min(vh);
    let w = max_x.saturating_sub(r.x);
    let h = max_y.saturating_sub(r.y);
    if w == 0 || h == 0 {
        None
    } else {
        Some(DamageRect::new(r.x, r.y, w, h))
    }
}

fn union_rect(a: DamageRect, b: DamageRect) -> DamageRect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = a.x.saturating_add(a.width).max(b.x.saturating_add(b.width));
    let y1 =
        a.y.saturating_add(a.height)
            .max(b.y.saturating_add(b.height));
    DamageRect::new(x0, y0, x1 - x0, y1 - y0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_nearby_widgets_and_splits_distant_spi_commits() {
        let near_a = DamageRect::new(10, 10, 40, 20);
        let near_b = DamageRect::new(45, 10, 35, 20);
        let merged = plan_dirtyfb_commits(&[near_a, near_b], 480, 320, UNION_BLOAT_SPLIT_RATIO_X10);
        assert_eq!(merged.commits, vec![DamageRect::new(10, 10, 70, 20)]);

        let top_left = DamageRect::new(0, 0, 24, 24);
        let bot_right = DamageRect::new(440, 290, 40, 30);
        let split = plan_dirtyfb_commits(
            &[top_left, bot_right],
            480,
            320,
            UNION_BLOAT_SPLIT_RATIO_X10,
        );
        assert_eq!(split.commits, vec![top_left, bot_right]);
    }
}
