//! Arbitrary-length `TransitionClause` interned overflow storage and W3C list-cycling helpers.

use crate::transition::{
    TransitionClause, TransitionCurve, TransitionMask, TransitionSpec, MAX_TRANSITION_CLAUSES,
};
use crate::transition_layout::{layout_property_bit, LayoutTransitionMask};
use std::sync::{Arc, OnceLock, RwLock};

static EXTRA_CLAUSES: OnceLock<RwLock<Vec<Arc<[TransitionClause]>>>> = OnceLock::new();

fn clause_store() -> &'static RwLock<Vec<Arc<[TransitionClause]>>> {
    EXTRA_CLAUSES.get_or_init(|| RwLock::new(Vec::new()))
}

fn intern_extra_clauses(items: Vec<TransitionClause>) -> u32 {
    if let Ok(read) = clause_store().read() {
        for (idx, existing) in read.iter().enumerate() {
            if existing.as_ref() == items.as_slice() {
                return idx as u32;
            }
        }
    }
    let mut write = clause_store()
        .write()
        .unwrap_or_else(|poison| poison.into_inner());
    for (idx, existing) in write.iter().enumerate() {
        if existing.as_ref() == items.as_slice() {
            return idx as u32;
        }
    }
    let idx = write.len() as u32;
    write.push(Arc::from(items));
    idx
}

/// `Copy` + `const`-constructible overflow storage for CSS `transition` specs with `> 4` clauses.
#[derive(Clone, Copy, Debug, Default)]
pub enum TransitionExtraClauses {
    #[default]
    Empty,
    Static(&'static [TransitionClause]),
    Interned(u32),
}

impl TransitionExtraClauses {
    /// Creates a `const` static overflow clause slice (used by `krui-macros` codegen).
    pub const fn static_ref(items: &'static [TransitionClause]) -> Self {
        if items.is_empty() {
            Self::Empty
        } else {
            Self::Static(items)
        }
    }

    /// Interns dynamic overflow clauses (`items` starting from clause index 4).
    pub fn from_vec(items: Vec<TransitionClause>) -> Self {
        if items.is_empty() {
            Self::Empty
        } else {
            Self::Interned(intern_extra_clauses(items))
        }
    }

    pub fn is_empty(self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Visits the overflow clause slice without nesting RwLock read guards.
    pub fn with_extra<R>(self, f: impl FnOnce(&[TransitionClause]) -> R) -> R {
        match self {
            Self::Empty => f(&[]),
            Self::Static(s) => f(s),
            Self::Interned(idx) => {
                let arc = clause_store()
                    .read()
                    .ok()
                    .and_then(|v| v.get(idx as usize).cloned());
                match arc {
                    Some(a) => f(&a),
                    None => f(&[]),
                }
            }
        }
    }
}

impl PartialEq for TransitionExtraClauses {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Empty, Self::Empty) => true,
            (Self::Static(a), Self::Static(b)) => a == b,
            (Self::Interned(a), Self::Interned(b)) if a == b => true,
            _ => {
                let b_copy = other.with_extra(|s| s.to_vec());
                self.with_extra(|a| a == b_copy.as_slice())
            }
        }
    }
}
impl Eq for TransitionExtraClauses {}

#[cfg(feature = "preview")]
impl serde::Serialize for TransitionExtraClauses {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.with_extra(|slice| slice.serialize(s))
    }
}

#[cfg(feature = "preview")]
impl<'de> serde::Deserialize<'de> for TransitionExtraClauses {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let items = Vec::<TransitionClause>::deserialize(d)?;
        Ok(Self::from_vec(items))
    }
}

impl TransitionSpec {
    /// Returns all active clauses into a temporary `Vec`, honoring >4 overflow clauses.
    pub fn collect_clauses(self) -> Vec<TransitionClause> {
        if self.clause_count == 0 {
            if self.mask.0 == 0 && !self.layout_mask.is_any() && self.duration_ms == 0 {
                return Vec::new();
            }
            return vec![TransitionClause {
                mask: self.mask,
                layout_mask: self.layout_mask,
                duration_ms: self.duration_ms,
                delay_ms: self.delay_ms,
                curve: self.curve,
            }];
        }
        let inline_n = (self.clause_count as usize).min(MAX_TRANSITION_CLAUSES);
        let mut out = self.clauses[..inline_n].to_vec();
        self.extra.with_extra(|ex| out.extend_from_slice(ex));
        out
    }

    /// Rebuilds `self` from `clauses` with merged summary fields.
    pub fn from_clause_vec(clauses: &[TransitionClause]) -> Self {
        let mut out = Self::NONE;
        if clauses.is_empty() {
            return out;
        }
        for &cl in clauses.iter().take(MAX_TRANSITION_CLAUSES) {
            out.push_clause(cl);
        }
        if clauses.len() > MAX_TRANSITION_CLAUSES {
            for cl in &clauses[MAX_TRANSITION_CLAUSES..] {
                out.mask.0 |= cl.mask.0;
                out.layout_mask.0 |= cl.layout_mask.0;
                out.duration_ms = out.duration_ms.max(cl.duration_ms);
                out.delay_ms = out.delay_ms.max(cl.delay_ms);
                out.curve = cl.curve;
            }
            out.extra =
                TransitionExtraClauses::from_vec(clauses[MAX_TRANSITION_CLAUSES..].to_vec());
            out.clause_count = clauses.len().min(u8::MAX as usize) as u8;
        }
        out
    }

    /// Applies `transition-property` list (`(TransitionMask, LayoutTransitionMask)` pairs) using W3C list cycling.
    pub fn apply_property_list(&mut self, props: &[(TransitionMask, LayoutTransitionMask)]) {
        if props.is_empty() {
            *self = Self::NONE;
            return;
        }
        let existing = self.collect_clauses();
        let template = if existing.is_empty() {
            vec![TransitionClause {
                mask: self.mask,
                layout_mask: self.layout_mask,
                duration_ms: self.duration_ms,
                delay_ms: self.delay_ms,
                curve: self.curve,
            }]
        } else {
            existing
        };
        let next: Vec<_> = props
            .iter()
            .enumerate()
            .map(|(i, &(pm, lm))| {
                let t = template[i % template.len()];
                TransitionClause {
                    mask: pm,
                    layout_mask: lm,
                    duration_ms: t.duration_ms,
                    delay_ms: t.delay_ms,
                    curve: t.curve,
                }
            })
            .collect();
        let mut rebuilt = Self::from_clause_vec(&next);
        rebuilt.explicit_props = true;
        *self = rebuilt;
    }

    /// Applies W3C cycled `transition-duration` list (`u16` ms values).
    pub fn apply_durations_list(&mut self, durs: &[u16]) {
        if durs.is_empty() {
            return;
        }
        let mut clauses = ensure_clauses_for_list(self, durs.len());
        let explicit = self.explicit_props;
        for (i, cl) in clauses.iter_mut().enumerate() {
            cl.duration_ms = durs[i % durs.len()];
        }
        *self = Self::from_clause_vec(&clauses);
        self.explicit_props = explicit;
    }

    /// Applies W3C cycled `transition-delay` list (`i32` ms values, supports negative delays).
    pub fn apply_delays_list(&mut self, delays: &[i32]) {
        if delays.is_empty() {
            return;
        }
        let mut clauses = ensure_clauses_for_list(self, delays.len());
        let explicit = self.explicit_props;
        for (i, cl) in clauses.iter_mut().enumerate() {
            cl.delay_ms = delays[i % delays.len()];
        }
        *self = Self::from_clause_vec(&clauses);
        self.explicit_props = explicit;
    }

    /// Applies W3C cycled `transition-timing-function` list.
    pub fn apply_timings_list(&mut self, curves: &[TransitionCurve]) {
        if curves.is_empty() {
            return;
        }
        let mut clauses = ensure_clauses_for_list(self, curves.len());
        let explicit = self.explicit_props;
        for (i, cl) in clauses.iter_mut().enumerate() {
            cl.curve = curves[i % curves.len()];
        }
        *self = Self::from_clause_vec(&clauses);
        self.explicit_props = explicit;
    }
}

fn ensure_clauses_for_list(spec: &TransitionSpec, list_len: usize) -> Vec<TransitionClause> {
    let mut clauses = spec.collect_clauses();
    if clauses.is_empty() {
        let count = list_len.max(1);
        clauses.resize(
            count,
            TransitionClause {
                mask: TransitionMask::ALL,
                layout_mask: LayoutTransitionMask::ALL,
                duration_ms: spec.duration_ms,
                delay_ms: spec.delay_ms,
                curve: spec.curve,
            },
        );
    } else if !spec.explicit_props && clauses.len() < list_len {
        let first = clauses[0];
        clauses.resize(list_len, first);
    }
    clauses
}

/// Parses comma-separated CSS `transition-property` tokens into combined Tier-1 + Tier-2 masks.
pub fn parse_transition_property_list(
    raw: &str,
    paint_bit_fn: impl Fn(&str) -> Option<u16>,
) -> Option<Vec<(TransitionMask, LayoutTransitionMask)>> {
    let mut out = Vec::new();
    for part in crate::transition::split_top_level_commas(raw) {
        let p = part.trim().to_ascii_lowercase();
        if p == "none" {
            out.push((TransitionMask::NONE, LayoutTransitionMask::NONE));
            continue;
        }
        let pb = paint_bit_fn(&p).unwrap_or(0);
        let lb = layout_property_bit(&p).unwrap_or(0);
        if pb == 0 && lb == 0 {
            return None;
        }
        out.push((TransitionMask(pb), LayoutTransitionMask(lb)));
    }
    (!out.is_empty()).then_some(out)
}
