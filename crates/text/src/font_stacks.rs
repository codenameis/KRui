//! Isolated vs shared `FontSystem` management, ordered CSS `font-family` per-character
//! fallback (`@font-face` `unicode-range` + variable-font `wght`), strut resolution, and `Buffer` shaping.

use crate::cache::normalize_font_weight;
use crate::font_metrics::{
    extract_face_meta_from_bytes, register_font_face_rules, CachedFaceMeta, RegisteredFontFace,
};
use crate::font_presets::{
    expand_stack_candidates, generic_fallback_family, purge_symbol_and_dingbat_faces,
    resolve_builtin_multilingual_stacks, ResolvedFontStack,
};
use crate::rasterize::LineLayoutConfig;
use crate::text_wrap_ellipsis::{cosmic_wrap_for_config, shape_keep_all_cosmic_buffer};
use cosmic_text::fontdb::ID as FontId;
use cosmic_text::{
    Align, Attrs, AttrsList, AttrsOwned, Buffer, BufferLine, Family, FontSystem, LineEnding,
    Metrics, Shaping, Style, SwashCache, Weight,
};
use krui_style::{
    Color, FontFaceRule, FontFamilyStack, FontStyleSlope, FontVariationList, TextAlign,
    TextDirection, WordBreakMode,
};
use std::collections::HashMap;
use std::ops::Range;
use swash::scale::ScaleContext;

/// Sanitized rich-text span ready for `cosmic-text` `AttrsList` paragraph construction.
pub(crate) struct SanitizedRichSpan {
    pub(crate) text: String,
    pub(crate) font_weight: u16,
    pub(crate) italic: bool,
    pub(crate) font_family: FontFamilyStack,
    pub(crate) color: Option<Color>,
    pub(crate) variations: FontVariationList,
}

#[derive(Clone, Debug)]
struct SystemFaceEntry {
    face_id: FontId,
    family_name: String,
    db_weight: u16,
    db_style: Style,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MatchedFace {
    cosmic_family: String,
    db_weight: u16,
    db_style: Style,
    face_id: FontId,
}

/// Owns a `FontSystem`, `SwashCache`, variable-axis `ScaleContext`, and font-face caches.
pub(crate) struct FontContext {
    pub(crate) font_system: FontSystem,
    pub(crate) swash_cache: SwashCache,
    pub(crate) scale_ctx: ScaleContext,
    pub(crate) resolved_stacks: HashMap<FontFamilyStack, ResolvedFontStack>,
    pub(crate) face_meta_cache: HashMap<FontId, CachedFaceMeta>,
    pub(crate) custom_faces: Vec<RegisteredFontFace>,
    system_families: HashMap<String, Vec<SystemFaceEntry>>,
    cmap_cache: HashMap<(FontId, char), bool>,
    pub(crate) font_face_epoch: u64,
}

impl FontContext {
    pub(crate) fn new() -> Self {
        let mut font_system = FontSystem::new();
        purge_symbol_and_dingbat_faces(&mut font_system);
        let system_families = index_system_families(&font_system);
        let resolved_stacks = resolve_builtin_multilingual_stacks();
        Self {
            font_system,
            swash_cache: SwashCache::new(),
            scale_ctx: ScaleContext::new(),
            resolved_stacks,
            face_meta_cache: HashMap::new(),
            custom_faces: Vec::new(),
            system_families,
            cmap_cache: HashMap::new(),
            font_face_epoch: 0,
        }
    }

    pub(crate) fn register_font_faces(&mut self, rules: &[FontFaceRule]) -> bool {
        if rules.is_empty() {
            return false;
        }
        let changed = register_font_face_rules(
            &mut self.font_system,
            &mut self.face_meta_cache,
            &mut self.custom_faces,
            rules,
        );
        if changed {
            self.font_face_epoch = self.font_face_epoch.wrapping_add(1);
        }
        changed
    }

    pub(crate) fn ensure_stack_resolved(&mut self, font_family: FontFamilyStack) {
        self.resolved_stacks
            .entry(font_family)
            .or_insert_with(|| ResolvedFontStack {
                candidates: expand_stack_candidates(font_family),
            });
    }

    pub(crate) fn ensure_face_meta(&mut self, face_id: FontId) -> CachedFaceMeta {
        if let Some(m) = self.face_meta_cache.get(&face_id) {
            return m.clone();
        }
        let meta = if let Some(font) = self.font_system.get_font(face_id) {
            let idx = self
                .font_system
                .db()
                .face(face_id)
                .map_or(0, |info| info.index);
            extract_face_meta_from_bytes(font.data(), idx)
        } else {
            CachedFaceMeta::default()
        };
        self.face_meta_cache.insert(face_id, meta.clone());
        meta
    }

    fn face_covers_char(&mut self, face_id: FontId, ch: char) -> bool {
        if let Some(&hit) = self.cmap_cache.get(&(face_id, ch)) {
            return hit;
        }
        let covers = self
            .font_system
            .get_font(face_id)
            .is_some_and(|f| f.rustybuzz().glyph_index(ch).is_some());
        self.cmap_cache.insert((face_id, ch), covers);
        covers
    }

    /// Resolves the CSS strut font (first available font in `font_family`) and its vertical metrics.
    pub(crate) fn resolve_strut(
        &mut self,
        font_family: FontFamilyStack,
        weight: u16,
        italic: bool,
    ) -> (AttrsOwned, CachedFaceMeta) {
        self.ensure_stack_resolved(font_family);
        let candidates = self
            .resolved_stacks
            .get(&font_family)
            .map(|s| s.candidates.clone())
            .unwrap_or_default();
        let req_w = normalize_font_weight(weight);
        for cand in &candidates {
            if let Some(m) = self.match_first_in_family(cand, None, req_w, italic) {
                let meta = self.ensure_face_meta(m.face_id);
                let attrs = Attrs::new()
                    .family(Family::Name(&m.cosmic_family))
                    .weight(Weight(m.db_weight))
                    .style(m.db_style);
                return (AttrsOwned::new(attrs), meta);
            }
        }
        let fallback = Attrs::new()
            .family(generic_fallback_family(font_family))
            .weight(Weight(req_w));
        (AttrsOwned::new(fallback), CachedFaceMeta::default())
    }

    fn match_first_in_family(
        &mut self,
        cand_name: &str,
        ch_opt: Option<char>,
        req_weight: u16,
        italic: bool,
    ) -> Option<MatchedFace> {
        let cand_low = cand_name.to_ascii_lowercase();
        if let Some(m) = self.match_custom_face(&cand_low, ch_opt, req_weight, italic) {
            return Some(m);
        }
        self.match_system_face(&cand_low, ch_opt, req_weight, italic)
    }

    fn match_custom_face(
        &mut self,
        cand_low: &str,
        ch_opt: Option<char>,
        req_weight: u16,
        italic: bool,
    ) -> Option<MatchedFace> {
        let mut matches: Vec<(u16, usize)> = self
            .custom_faces
            .iter()
            .enumerate()
            .filter(|(_, rf)| {
                rf.family_lower == cand_low && ch_opt.is_none_or(|c| rf.matches_char(c))
            })
            .map(|(idx, rf)| {
                let want_italic = italic;
                let got_italic = !matches!(rf.slope, FontStyleSlope::Normal);
                let slope_pen = u16::from(want_italic != got_italic) * 2000;
                (slope_pen + rf.weight_distance(req_weight), idx)
            })
            .collect();
        matches.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)));
        for (_, idx) in matches {
            let rf = self.custom_faces[idx].clone();
            if ch_opt.is_none_or(|ch| self.face_covers_char(rf.face_id, ch)) {
                return Some(MatchedFace {
                    cosmic_family: rf.cosmic_family_name,
                    db_weight: rf.db_weight,
                    db_style: rf.db_style,
                    face_id: rf.face_id,
                });
            }
        }
        None
    }

    fn match_system_face(
        &mut self,
        cand_low: &str,
        ch_opt: Option<char>,
        req_weight: u16,
        italic: bool,
    ) -> Option<MatchedFace> {
        let entries = self.system_families.get(cand_low)?.clone();
        let mut scored: Vec<(u16, SystemFaceEntry)> = Vec::with_capacity(entries.len());
        for entry in entries {
            let meta = self.ensure_face_meta(entry.face_id);
            let got_italic = entry.db_style != Style::Normal;
            let slope_pen = u16::from(italic != got_italic) * 2000;
            let w_pen = if let Some((wmin, wmax)) = meta.wght_range() {
                let rw = req_weight as f32;
                if rw < wmin {
                    (wmin - rw).round() as u16
                } else if rw > wmax {
                    (rw - wmax).round() as u16
                } else {
                    0
                }
            } else {
                entry.db_weight.abs_diff(req_weight)
            };
            scored.push((slope_pen + w_pen, entry));
        }
        scored.sort_by_key(|(s, _)| *s);
        for (_, entry) in scored {
            if ch_opt.is_none_or(|ch| self.face_covers_char(entry.face_id, ch)) {
                return Some(MatchedFace {
                    cosmic_family: entry.family_name,
                    db_weight: entry.db_weight,
                    db_style: entry.db_style,
                    face_id: entry.face_id,
                });
            }
        }
        None
    }

    fn resolve_char_face(
        &mut self,
        candidates: &[&'static str],
        ch: char,
        req_weight: u16,
        italic: bool,
    ) -> Option<MatchedFace> {
        for &cand in candidates {
            if let Some(m) = self.match_first_in_family(cand, Some(ch), req_weight, italic) {
                return Some(m);
            }
        }
        None
    }
}

fn index_system_families(font_system: &FontSystem) -> HashMap<String, Vec<SystemFaceEntry>> {
    let mut map: HashMap<String, Vec<SystemFaceEntry>> = HashMap::new();
    for face in font_system.db().faces() {
        for (fam_name, _) in &face.families {
            map.entry(fam_name.to_ascii_lowercase())
                .or_default()
                .push(SystemFaceEntry {
                    face_id: face.id,
                    family_name: fam_name.clone(),
                    db_weight: face.weight.0,
                    db_style: face.style,
                });
        }
    }
    map
}

/// Returns `true` if `ch` belongs to Korean Hangul Syllables/Jamo or CJK Unified Ideographs.
pub const fn is_cjk_or_hangul(ch: char) -> bool {
    matches!(
        ch,
        '\u{AC00}'..='\u{D7AF}'
            | '\u{1100}'..='\u{11FF}'
            | '\u{3130}'..='\u{318F}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{3040}'..='\u{30FF}'
    )
}

fn resolve_cosmic_align(
    align: TextAlign,
    direction: TextDirection,
    has_max_width: bool,
) -> Option<Align> {
    if !has_max_width {
        return None;
    }
    match align {
        TextAlign::Left => (direction == TextDirection::Rtl).then_some(Align::Right),
        TextAlign::Center => Some(Align::Center),
        TextAlign::Right => Some(Align::Right),
    }
}

fn build_ranged_span_attrs(
    ctx: &mut FontContext,
    spans: &[SanitizedRichSpan],
) -> Vec<(Range<usize>, AttrsOwned)> {
    let mut ranged = Vec::new();
    let mut cursor_b = 0_usize;
    for (span_idx, span) in spans.iter().enumerate() {
        ctx.ensure_stack_resolved(span.font_family);
        if span.text.is_empty() {
            continue;
        }
        let candidates = ctx
            .resolved_stacks
            .get(&span.font_family)
            .map(|s| s.candidates.clone())
            .unwrap_or_default();
        let req_w = normalize_font_weight(span.font_weight);
        append_span_runs(
            ctx,
            span_idx,
            span,
            &candidates,
            req_w,
            &mut cursor_b,
            &mut ranged,
        );
    }
    ranged
}

fn append_span_runs(
    ctx: &mut FontContext,
    span_idx: usize,
    span: &SanitizedRichSpan,
    candidates: &[&'static str],
    req_w: u16,
    cursor_b: &mut usize,
    ranged: &mut Vec<(Range<usize>, AttrsOwned)>,
) {
    let base_b = *cursor_b;
    *cursor_b += span.text.len();
    let mut run_start = 0usize;
    let mut active_face: Option<Option<MatchedFace>> = None;
    for (idx, ch) in span.text.char_indices() {
        let next_face = if ch.is_whitespace() {
            active_face.clone().unwrap_or(None)
        } else {
            ctx.resolve_char_face(candidates, ch, req_w, span.italic)
        };
        match &active_face {
            None => {
                active_face = Some(next_face);
                run_start = idx;
            }
            Some(prev) if *prev != next_face && !ch.is_whitespace() => {
                ranged.push((
                    (base_b + run_start)..(base_b + idx),
                    make_span_attrs(span_idx, span, req_w, prev.as_ref()),
                ));
                active_face = Some(next_face);
                run_start = idx;
            }
            _ => {}
        }
    }
    if let Some(last) = active_face {
        ranged.push((
            (base_b + run_start)..(base_b + span.text.len()),
            make_span_attrs(span_idx, span, req_w, last.as_ref()),
        ));
    }
}

fn make_span_attrs(
    span_idx: usize,
    span: &SanitizedRichSpan,
    req_w: u16,
    matched: Option<&MatchedFace>,
) -> AttrsOwned {
    let mut attrs = match matched {
        Some(m) => Attrs::new()
            .family(Family::Name(&m.cosmic_family))
            .weight(Weight(m.db_weight))
            .style(m.db_style),
        None => Attrs::new()
            .family(generic_fallback_family(span.font_family))
            .weight(Weight(req_w))
            .style(if span.italic {
                Style::Italic
            } else {
                Style::Normal
            }),
    }
    .metadata(span_idx);
    if let Some(c) = span.color {
        attrs = attrs.color(cosmic_text::Color::rgba(c.r, c.g, c.b, c.a));
    }
    AttrsOwned::new(attrs)
}

pub(crate) fn shape_rich_cosmic_buffer(
    ctx: &mut FontContext,
    spans: &[SanitizedRichSpan],
    full_text: &str,
    cfg: LineLayoutConfig,
) -> ((Buffer, Vec<usize>), CachedFaceMeta) {
    let safe_cosmic_lh = (cfg.font_size * 4.0).max(16.0);
    let (base_fam, base_w, base_it) = spans
        .first()
        .map_or((FontFamilyStack::FluentUI, 400, false), |s| {
            (s.font_family, s.font_weight, s.italic)
        });
    let (strut_attrs, strut_meta) = ctx.resolve_strut(base_fam, base_w, base_it);
    let ranged_attrs = build_ranged_span_attrs(ctx, spans);
    let line_align = resolve_cosmic_align(cfg.align, cfg.direction, cfg.max_width.is_some());

    if cfg.white_space.allows_wrap()
        && cfg.word_break == WordBreakMode::KeepAll
        && cfg.max_width.is_some()
    {
        let res = shape_keep_all_cosmic_buffer(
            ctx,
            full_text,
            cfg,
            safe_cosmic_lh,
            &strut_attrs,
            &ranged_attrs,
            line_align,
        );
        return (res, strut_meta);
    }

    let mut buffer = Buffer::new(
        &mut ctx.font_system,
        Metrics::new(cfg.font_size, safe_cosmic_lh),
    );
    buffer.set_wrap(&mut ctx.font_system, cosmic_wrap_for_config(&cfg));
    buffer.set_size(&mut ctx.font_system, cfg.max_width, None);
    let offsets = populate_buffer_lines(
        &mut buffer,
        full_text,
        None,
        &strut_attrs,
        &ranged_attrs,
        line_align,
    );
    buffer.shape_until_scroll(&mut ctx.font_system, false);
    ((buffer, offsets), strut_meta)
}

pub(crate) fn populate_buffer_lines(
    buffer: &mut Buffer,
    full_text: &str,
    explicit_slices: Option<&[(usize, usize)]>,
    strut_attrs: &AttrsOwned,
    ranged_attrs: &[(Range<usize>, AttrsOwned)],
    line_align: Option<Align>,
) -> Vec<usize> {
    let mut lines = Vec::new();
    let mut offsets = Vec::new();
    let mut push_slice = |s_b: usize, e_b: usize| {
        offsets.push(s_b);
        let mut attrs_list = AttrsList::new(strut_attrs.as_attrs());
        for (range, owned) in ranged_attrs {
            let s = range.start.max(s_b);
            let e = range.end.min(e_b);
            if s < e {
                attrs_list.add_span((s - s_b)..(e - s_b), owned.as_attrs());
            }
        }
        let mut buf_line = BufferLine::new(
            &full_text[s_b..e_b],
            LineEnding::Lf,
            attrs_list,
            Shaping::Advanced,
        );
        buf_line.set_align(line_align);
        lines.push(buf_line);
    };
    if let Some(slices) = explicit_slices {
        for &(s_b, e_b) in slices {
            push_slice(s_b, e_b);
        }
    } else {
        let mut line_start_b = 0_usize;
        for raw_line in full_text.split('\n') {
            let line_end_b = line_start_b + raw_line.len();
            push_slice(line_start_b, line_end_b);
            line_start_b = line_end_b + 1;
        }
    }
    buffer.lines = lines;
    offsets
}
