use cosmic_text::fontdb::{self, FaceInfo, Language, Source, Style, Weight, ID as FontId};
use cosmic_text::ttf_parser::{Face as TtfFace, Tag as TtfTag};
use cosmic_text::FontSystem;
use krui_style::{
    FontFaceMetricsOverride, FontFaceRule, FontFaceSource, FontStyleSlope, LineHeight, TextBoxEdge,
    TextBoxTrim, TextOverEdge, TextUnderEdge, UnicodeRange,
};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FvarAxisInfo {
    pub(crate) tag: [u8; 4],
    pub(crate) min: f32,
    pub(crate) def: f32,
    pub(crate) max: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct CachedFaceMeta {
    pub(crate) ascent_em: f32,
    pub(crate) descent_em: f32,
    pub(crate) line_gap_em: f32,
    pub(crate) cap_height_em: f32,
    pub(crate) x_height_em: f32,
    pub(crate) ideo_over_em: f32,
    pub(crate) ideo_under_em: f32,
    pub(crate) fvar_axes: Vec<FvarAxisInfo>,
}

impl Default for CachedFaceMeta {
    fn default() -> Self {
        Self {
            ascent_em: 0.88,
            descent_em: 0.20,
            line_gap_em: 0.0,
            cap_height_em: 0.72,
            x_height_em: 0.50,
            ideo_over_em: 0.88,
            ideo_under_em: 0.12,
            fvar_axes: Vec::new(),
        }
    }
}

impl CachedFaceMeta {
    pub(crate) fn with_overrides(&self, ov: FontFaceMetricsOverride) -> Self {
        let mut out = self.clone();
        if let Some(asc) = ov.ascent_pct {
            out.ascent_em = asc.max(0.0);
        }
        if let Some(desc) = ov.descent_pct {
            out.descent_em = desc.max(0.0);
        }
        if let Some(gap) = ov.line_gap_pct {
            out.line_gap_em = gap.max(0.0);
        }
        out
    }

    pub(crate) fn wght_range(&self) -> Option<(f32, f32)> {
        self.fvar_axes
            .iter()
            .find(|a| a.tag == *b"wght")
            .map(|a| (a.min, a.max))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RegisteredFontFace {
    pub(crate) dedup_hash: u64,
    pub(crate) family_lower: String,
    pub(crate) cosmic_family_name: String,
    pub(crate) face_id: FontId,
    pub(crate) db_weight: u16,
    pub(crate) db_style: Style,
    pub(crate) weight_min: u16,
    pub(crate) weight_max: u16,
    pub(crate) slope: FontStyleSlope,
    pub(crate) unicode_ranges: Vec<UnicodeRange>,
}

impl RegisteredFontFace {
    #[inline]
    pub(crate) fn matches_char(&self, ch: char) -> bool {
        let cp = ch as u32;
        self.unicode_ranges.is_empty() || self.unicode_ranges.iter().any(|r| r.contains(cp))
    }

    #[inline]
    pub(crate) fn weight_distance(&self, req_weight: u16) -> u16 {
        if req_weight < self.weight_min {
            self.weight_min - req_weight
        } else {
            req_weight.saturating_sub(self.weight_max)
        }
    }
}

pub(crate) fn extract_face_meta_from_bytes(data: &[u8], index: u32) -> CachedFaceMeta {
    let Ok(face) = TtfFace::parse(data, index) else {
        return CachedFaceMeta::default();
    };
    let upem = (face.units_per_em().max(1)) as f32;
    let use_typo = has_os2_use_typo_metrics_bit(&face);
    let os2 = face.tables().os2;
    let hhea = face.tables().hhea;
    let (ascent_em, descent_em, line_gap_em) = if use_typo {
        if let Some(o) = os2 {
            (
                (o.typographic_ascender() as f32 / upem).max(0.0),
                (-(o.typographic_descender() as f32) / upem).max(0.0),
                (o.typographic_line_gap() as f32 / upem).max(0.0),
            )
        } else {
            (
                (hhea.ascender as f32 / upem).max(0.0),
                (-(hhea.descender as f32) / upem).max(0.0),
                (hhea.line_gap as f32 / upem).max(0.0),
            )
        }
    } else {
        extract_legacy_win_or_hhea_metrics(os2, hhea, upem)
    };
    let cap_height_em = os2
        .and_then(|o| o.capital_height())
        .filter(|&c| c > 0)
        .map_or(0.72, |c| c as f32 / upem);
    let x_height_em = os2
        .and_then(|o| o.x_height())
        .filter(|&x| x > 0)
        .map_or(0.50, |x| x as f32 / upem);
    let (ideo_over_em, ideo_under_em) = os2.map_or((0.88, 0.12), |o| {
        (
            (o.typographic_ascender() as f32 / upem).clamp(0.70, 0.95),
            (-(o.typographic_descender() as f32) / upem).clamp(0.05, 0.30),
        )
    });
    let fvar_axes = extract_fvar_axes(&face);
    CachedFaceMeta {
        ascent_em,
        descent_em,
        line_gap_em,
        cap_height_em,
        x_height_em,
        ideo_over_em,
        ideo_under_em,
        fvar_axes,
    }
}

fn has_os2_use_typo_metrics_bit(face: &TtfFace<'_>) -> bool {
    if let Some(raw_os2) = face.raw_face().table(TtfTag::from_bytes(b"OS/2")) {
        if raw_os2.len() >= 64 {
            let fs_selection = u16::from_be_bytes([raw_os2[62], raw_os2[63]]);
            return (fs_selection & 0x0080) != 0;
        }
    }
    face.tables()
        .os2
        .is_some_and(|o| o.use_typographic_metrics())
}

fn extract_legacy_win_or_hhea_metrics(
    os2: Option<cosmic_text::ttf_parser::os2::Table<'_>>,
    hhea: cosmic_text::ttf_parser::hhea::Table,
    upem: f32,
) -> (f32, f32, f32) {
    if let Some(o) = os2 {
        let win_asc = (o.windows_ascender() as i32).abs() as f32;
        let win_desc = (o.windows_descender() as i32).abs() as f32;
        if win_asc > 0.0 || win_desc > 0.0 {
            let gap = if (hhea.ascender as f32 - win_asc).abs() < 2.0 {
                (hhea.line_gap as f32 / upem).max(0.0)
            } else {
                0.0
            };
            return (win_asc / upem, win_desc / upem, gap);
        }
    }
    (
        (hhea.ascender as f32 / upem).max(0.0),
        (-(hhea.descender as f32) / upem).max(0.0),
        (hhea.line_gap as f32 / upem).max(0.0),
    )
}

fn extract_fvar_axes(face: &TtfFace<'_>) -> Vec<FvarAxisInfo> {
    let Some(fvar) = face.tables().fvar else {
        return Vec::new();
    };
    fvar.axes
        .into_iter()
        .map(|ax| FvarAxisInfo {
            tag: ax.tag.to_bytes(),
            min: ax.min_value,
            def: ax.def_value,
            max: ax.max_value,
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LineBoxMetrics {
    pub(crate) line_height: f32,
    pub(crate) baseline_offset: f32,
    pub(crate) ascent: f32,
    pub(crate) descent: f32,
}

pub(crate) fn compute_w3c_line_box(
    meta: &CachedFaceMeta,
    font_size: f32,
    line_height: LineHeight,
    trim: TextBoxTrim,
    edge: TextBoxEdge,
    is_first_line: bool,
    is_last_line: bool,
) -> LineBoxMetrics {
    let ascent = meta.ascent_em * font_size;
    let descent = meta.descent_em * font_size;
    let gap = meta.line_gap_em * font_size;
    let normal_px = ascent + descent + gap;
    let raw_lh = line_height.resolve_px(font_size, normal_px);
    let half_leading = (raw_lh - (ascent + descent)) * 0.5;
    let mut over = half_leading + ascent;
    let mut under = descent + half_leading;

    if is_first_line && matches!(trim, TextBoxTrim::TrimStart | TextBoxTrim::TrimBoth) {
        over = match edge.over {
            TextOverEdge::Auto | TextOverEdge::Text => ascent,
            TextOverEdge::Cap => meta.cap_height_em * font_size,
            TextOverEdge::Ex => meta.x_height_em * font_size,
            TextOverEdge::Ideographic | TextOverEdge::IdeographicInk => {
                meta.ideo_over_em * font_size
            }
        };
    }
    if is_last_line && matches!(trim, TextBoxTrim::TrimEnd | TextBoxTrim::TrimBoth) {
        under = match edge.under {
            TextUnderEdge::Auto | TextUnderEdge::Text => descent,
            TextUnderEdge::Alphabetic => 0.0,
            TextUnderEdge::Ideographic | TextUnderEdge::IdeographicInk => {
                meta.ideo_under_em * font_size
            }
        };
    }
    LineBoxMetrics {
        line_height: (over + under).max(0.0),
        baseline_offset: over,
        ascent,
        descent,
    }
}

pub(crate) fn register_font_face_rules(
    font_system: &mut FontSystem,
    face_meta_cache: &mut HashMap<FontId, CachedFaceMeta>,
    custom_faces: &mut Vec<RegisteredFontFace>,
    rules: &[FontFaceRule],
) -> bool {
    let mut added_any = false;
    for rule in rules {
        let dedup = hash_font_face_rule(rule);
        if custom_faces.iter().any(|f| f.dedup_hash == dedup) {
            continue;
        }
        if register_single_rule(font_system, face_meta_cache, custom_faces, rule, dedup) {
            added_any = true;
        }
    }
    added_any
}

fn register_single_rule(
    font_system: &mut FontSystem,
    face_meta_cache: &mut HashMap<FontId, CachedFaceMeta>,
    custom_faces: &mut Vec<RegisteredFontFace>,
    rule: &FontFaceRule,
    dedup_hash: u64,
) -> bool {
    let slot_idx = custom_faces.len();
    for src in &rule.sources {
        match src {
            FontFaceSource::Local(local_name) => {
                if let Some((local_src, local_idx, stretch, mono)) =
                    find_local_face_source(font_system, local_name, rule)
                {
                    if let Some(reg) = push_aliased_face(
                        font_system,
                        face_meta_cache,
                        slot_idx,
                        rule,
                        (local_src, local_idx, stretch, mono),
                        dedup_hash,
                    ) {
                        custom_faces.push(reg);
                        return true;
                    }
                }
            }
            FontFaceSource::Embedded(bytes) => {
                if TtfFace::parse(bytes, 0).is_ok() {
                    let bin = Source::Binary(Arc::new(bytes.to_vec()));
                    if let Some(reg) = push_aliased_face(
                        font_system,
                        face_meta_cache,
                        slot_idx,
                        rule,
                        (bin, 0, fontdb::Stretch::Normal, false),
                        dedup_hash,
                    ) {
                        custom_faces.push(reg);
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn find_local_face_source(
    font_system: &FontSystem,
    local_name: &str,
    rule: &FontFaceRule,
) -> Option<(Source, u32, fontdb::Stretch, bool)> {
    let target_style = match rule.style {
        FontStyleSlope::Normal => Style::Normal,
        FontStyleSlope::Italic => Style::Italic,
        FontStyleSlope::Oblique => Style::Oblique,
    };
    let mid_w = rule.weight_min.midpoint(rule.weight_max);
    font_system
        .db()
        .faces()
        .filter(|f| {
            f.post_script_name.eq_ignore_ascii_case(local_name)
                || f.families
                    .iter()
                    .any(|(n, _)| n.eq_ignore_ascii_case(local_name))
        })
        .min_by_key(|f| {
            let style_pen = u16::from(f.style != target_style) * 1000;
            style_pen + f.weight.0.abs_diff(mid_w)
        })
        .map(|f| (f.source.clone(), f.index, f.stretch, f.monospaced))
}

fn push_aliased_face(
    font_system: &mut FontSystem,
    face_meta_cache: &mut HashMap<FontId, CachedFaceMeta>,
    slot_idx: usize,
    rule: &FontFaceRule,
    (source, index, stretch, monospaced): (Source, u32, fontdb::Stretch, bool),
    dedup_hash: u64,
) -> Option<RegisteredFontFace> {
    let alias = format!("__krui_ff_{slot_idx}");
    let db_style = match rule.style {
        FontStyleSlope::Normal => Style::Normal,
        FontStyleSlope::Italic => Style::Italic,
        FontStyleSlope::Oblique => Style::Oblique,
    };
    let db_weight = rule.weight_min.midpoint(rule.weight_max);
    font_system.db_mut().push_face_info(FaceInfo {
        id: fontdb::ID::dummy(),
        source,
        index,
        families: vec![
            (alias.clone(), Language::English_UnitedStates),
            (rule.family.clone(), Language::English_UnitedStates),
        ],
        post_script_name: alias.clone(),
        style: db_style,
        weight: Weight(db_weight),
        stretch,
        monospaced,
    });
    let real_id = font_system
        .db()
        .faces()
        .find(|f| f.post_script_name == alias)?
        .id;
    let base_meta = font_system
        .get_font(real_id)
        .map_or_else(CachedFaceMeta::default, |f| {
            extract_face_meta_from_bytes(f.data(), index)
        });
    face_meta_cache.insert(real_id, base_meta.with_overrides(rule.metrics_override));
    Some(RegisteredFontFace {
        dedup_hash,
        family_lower: rule.family.to_ascii_lowercase(),
        cosmic_family_name: alias,
        face_id: real_id,
        db_weight,
        db_style,
        weight_min: rule.weight_min,
        weight_max: rule.weight_max,
        slope: rule.style,
        unicode_ranges: rule.unicode_ranges.clone(),
    })
}

fn hash_font_face_rule(r: &FontFaceRule) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |bytes: &[u8]| {
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    mix(r.family.as_bytes());
    mix(&r.weight_min.to_le_bytes());
    mix(&r.weight_max.to_le_bytes());
    mix(&[r.style as u8]);
    for ur in &r.unicode_ranges {
        mix(&ur.start.to_le_bytes());
        mix(&ur.end.to_le_bytes());
    }
    for opt in [
        r.metrics_override.ascent_pct,
        r.metrics_override.descent_pct,
        r.metrics_override.line_gap_pct,
    ] {
        mix(&opt.map_or(u32::MAX, f32::to_bits).to_le_bytes());
    }
    for s in &r.sources {
        match s {
            FontFaceSource::Local(n) => mix(n.as_bytes()),
            FontFaceSource::Embedded(b) => {
                mix(&(b.len() as u64).to_le_bytes());
                let sample_end = b.len().min(256);
                mix(&b[..sample_end]);
                if b.len() > 256 {
                    mix(&b[b.len() - 256..]);
                }
            }
        }
    }
    h
}
