//! Conversion from Stylo CSS timing functions, `transition-*`, and `animation-*` properties
//! into KRui [`TransitionCurve`], [`TransitionSpec`], and [`AnimationListRef`].

use krui_style::transition::paint_property_bit;
use krui_style::transition_layout::layout_property_bit;
use krui_style::{
    AnimationDirection, AnimationFillMode, AnimationListRef, AnimationSpec, LayoutTransitionMask,
    LinearCurveRef, LinearStop, StepPosition, TransitionClause, TransitionCurve, TransitionMask,
    TransitionSpec,
};
use style::properties::style_structs::UI;
use style::values::computed::TimingFunction as StyloTimingFunction;
use style::values::generics::easing::{
    StepPosition as StyloStepPosition, TimingFunction, TimingKeyword,
};
use style::values::specified::animation::{
    AnimationDirection as StyloAnimDir, AnimationFillMode as StyloAnimFill,
    AnimationPlayState as StyloAnimPlay, TransitionProperty,
};

fn seconds_to_u16_ms(secs: f32) -> u16 {
    if !secs.is_finite() || secs <= 0.0 {
        return 0;
    }
    (secs * 1000.0).round().clamp(0.0, u16::MAX as f32) as u16
}

fn seconds_to_i32_ms(secs: f32) -> i32 {
    if !secs.is_finite() {
        return 0;
    }
    (secs * 1000.0).round().clamp(-3_600_000.0, 3_600_000.0) as i32
}

/// Converts a Stylo computed [`TimingFunction`] into KRui [`TransitionCurve`].
#[must_use]
pub(crate) fn convert_timing_function(tf: &StyloTimingFunction) -> TransitionCurve {
    match tf {
        TimingFunction::Keyword(TimingKeyword::Linear) => TransitionCurve::Linear,
        TimingFunction::Keyword(TimingKeyword::Ease) => TransitionCurve::Ease,
        TimingFunction::Keyword(TimingKeyword::EaseIn) => TransitionCurve::EaseIn,
        TimingFunction::Keyword(TimingKeyword::EaseOut) => TransitionCurve::EaseOut,
        TimingFunction::Keyword(TimingKeyword::EaseInOut) => TransitionCurve::EaseInOut,
        TimingFunction::CubicBezier { x1, y1, x2, y2 } => TransitionCurve::CubicBezier {
            x1: *x1,
            y1: *y1,
            x2: *x2,
            y2: *y2,
        },
        TimingFunction::Steps(count, pos) => TransitionCurve::Steps {
            count: (*count).max(1).min(u16::MAX as i32) as u16,
            pos: match pos {
                StyloStepPosition::JumpStart | StyloStepPosition::Start => StepPosition::JumpStart,
                StyloStepPosition::JumpEnd | StyloStepPosition::End => StepPosition::JumpEnd,
                StyloStepPosition::JumpNone => StepPosition::JumpNone,
                StyloStepPosition::JumpBoth => StepPosition::JumpBoth,
            },
        },
        TimingFunction::LinearFunction(plf) => {
            let stops: Vec<LinearStop> = plf
                .iter()
                .map(|entry| LinearStop {
                    t: entry.x,
                    y: entry.y,
                })
                .collect();
            if stops.is_empty() {
                TransitionCurve::Linear
            } else {
                TransitionCurve::LinearStops(LinearCurveRef::intern(stops))
            }
        }
    }
}

fn transition_prop_masks(prop: &TransitionProperty) -> (TransitionMask, LayoutTransitionMask) {
    if prop.is_all() {
        return (TransitionMask::ALL, LayoutTransitionMask::ALL);
    }
    if prop.is_none() {
        return (TransitionMask::NONE, LayoutTransitionMask::NONE);
    }
    match prop {
        TransitionProperty::NonCustom(id) => {
            let name = id.name();
            let pb = paint_property_bit(name).unwrap_or(0);
            let lb = layout_property_bit(name).unwrap_or(0);
            (TransitionMask(pb), LayoutTransitionMask(lb))
        }
        TransitionProperty::Custom(_) | TransitionProperty::Unsupported(_) => {
            (TransitionMask::NONE, LayoutTransitionMask::NONE)
        }
    }
}

/// Extracts [`TransitionSpec`] (Tier-1 paint + Tier-2 layout) from Stylo `UI` style struct.
#[must_use]
pub(crate) fn extract_transition_spec(ui: &UI) -> TransitionSpec {
    let count = ui.transition_property_count();
    let mut clauses = Vec::with_capacity(count);
    for i in 0..count {
        let prop = ui.transition_property_at(i);
        let (mask, layout_mask) = transition_prop_masks(&prop);
        let duration_ms = seconds_to_u16_ms(ui.transition_duration_mod(i).seconds());
        let delay_ms = seconds_to_i32_ms(ui.transition_delay_mod(i).seconds());
        if (mask.0 == 0 && layout_mask.0 == 0) || (duration_ms == 0 && delay_ms <= 0) {
            continue;
        }
        let curve = convert_timing_function(&ui.transition_timing_function_mod(i));
        clauses.push(TransitionClause {
            mask,
            layout_mask,
            duration_ms,
            delay_ms,
            curve,
        });
    }
    if clauses.is_empty() {
        return TransitionSpec::NONE;
    }
    TransitionSpec::from_clauses(&clauses)
}

/// Extracts CSS `animation-*` specifications into KRui [`AnimationListRef`].
#[must_use]
pub(crate) fn extract_animations(ui: &UI) -> AnimationListRef {
    let count = ui.animation_name_count();
    let mut specs = Vec::new();
    for i in 0..count {
        let name = ui.animation_name_at(i);
        let Some(atom) = name.as_atom() else {
            continue;
        };
        let duration_ms = seconds_to_u16_ms(ui.animation_duration_mod(i).seconds()) as u32;
        if duration_ms == 0 {
            continue;
        }
        let iterations = ui.animation_iteration_count_mod(i).0;
        let direction = match ui.animation_direction_mod(i) {
            StyloAnimDir::Normal => AnimationDirection::Normal,
            StyloAnimDir::Reverse => AnimationDirection::Reverse,
            StyloAnimDir::Alternate => AnimationDirection::Alternate,
            StyloAnimDir::AlternateReverse => AnimationDirection::AlternateReverse,
        };
        let fill_mode = match ui.animation_fill_mode_mod(i) {
            StyloAnimFill::None => AnimationFillMode::None,
            StyloAnimFill::Forwards => AnimationFillMode::Forwards,
            StyloAnimFill::Backwards => AnimationFillMode::Backwards,
            StyloAnimFill::Both => AnimationFillMode::Both,
        };
        specs.push(AnimationSpec {
            name: atom.to_string(),
            duration_ms,
            delay_ms: seconds_to_i32_ms(ui.animation_delay_mod(i).seconds()),
            timing: convert_timing_function(&ui.animation_timing_function_mod(i)),
            iterations: if iterations.is_infinite() {
                None
            } else {
                Some(iterations.max(0.0))
            },
            direction,
            fill_mode,
            paused: matches!(ui.animation_play_state_mod(i), StyloAnimPlay::Paused),
        });
    }
    AnimationListRef::from_vec(specs)
}
