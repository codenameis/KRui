//! Stylo device configuration, font metrics provider, and CSS parse error collector.

use cssparser::SourceLocation;
use euclid::{Scale, Size2D};
use krui_style::{ColorScheme, DEFAULT_FONT_SIZE_PX};
use selectors::matching::QuirksMode;
use std::sync::{Mutex, Once};
use style::context::{RegisteredSpeculativePainter, RegisteredSpeculativePainters};
use style::device::servo::FontMetricsProvider;
use style::device::Device;
use style::error_reporting::{ContextualParseError, ParseErrorReporter};
use style::font_metrics::FontMetrics;
use style::media_queries::MediaType;
use style::properties::style_structs::Font as StyleFont;
use style::properties::ComputedValues;
use style::queries::values::PrefersColorScheme;
use style::servo::media_features::PointerCapabilities;
use style::stylesheets::UrlExtraData;
use style::values::computed::font::GenericFontFamily;
use style::values::computed::{CSSPixelLength, Length};
use style::Atom;

use crate::{CssDiagnostic, DeviceConfig, PointerKind};

const X_HEIGHT_RATIO: f32 = 0.5;
const ZERO_ADVANCE_RATIO: f32 = 0.5;
const CAP_HEIGHT_RATIO: f32 = 0.72;
const ASCENT_RATIO: f32 = 0.8;
const MIN_VIEWPORT_PX: f32 = 1.0;
const MIN_DPR: f32 = 0.5;

#[derive(Debug)]
pub(crate) struct KruiFontMetricsProvider;

impl FontMetricsProvider for KruiFontMetricsProvider {
    fn query_font_metrics(
        &self,
        _vertical: bool,
        _font: &StyleFont,
        base_size: CSSPixelLength,
        _flags: style::values::computed::font::QueryFontMetricsFlags,
    ) -> FontMetrics {
        let px = base_size.px();
        FontMetrics {
            x_height: Some(Length::new(px * X_HEIGHT_RATIO)),
            zero_advance_measure: Some(Length::new(px * ZERO_ADVANCE_RATIO)),
            cap_height: Some(Length::new(px * CAP_HEIGHT_RATIO)),
            ic_width: Some(Length::new(px)),
            ascent: Length::new(px * ASCENT_RATIO),
            script_percent_scale_down: None,
            script_script_percent_scale_down: None,
        }
    }

    fn base_size_for_generic(&self, _generic: GenericFontFamily) -> Length {
        Length::new(DEFAULT_FONT_SIZE_PX)
    }
}

pub(crate) struct NoopPainters;

impl RegisteredSpeculativePainters for NoopPainters {
    fn get(&self, _name: &Atom) -> Option<&dyn RegisteredSpeculativePainter> {
        None
    }
}

pub(crate) struct DiagnosticCollector<'a>(pub(crate) &'a Mutex<Vec<CssDiagnostic>>);

impl ParseErrorReporter for DiagnosticCollector<'_> {
    fn report_error(
        &self,
        _url: &UrlExtraData,
        location: SourceLocation,
        error: ContextualParseError,
    ) {
        if let Ok(mut list) = self.0.lock() {
            list.push(CssDiagnostic {
                line: location.line,
                column: location.column,
                message: error.to_string(),
            });
        }
    }
}

static PREFS_INIT: Once = Once::new();

pub(crate) fn init_stylo_prefs() {
    PREFS_INIT.call_once(|| {
        stylo_static_prefs::set_pref!("layout.container-queries.enabled", false);
        stylo_static_prefs::set_pref!("layout.css.at-scope.enabled", true);
        stylo_static_prefs::set_pref!("layout.css.has-selector.enabled", true);
        stylo_static_prefs::set_pref!("layout.css.nth-child-of.enabled", true);
        stylo_static_prefs::set_pref!("layout.grid.enabled", true);
        stylo_static_prefs::set_pref!("layout.flexbox.balance", true);
        stylo_static_prefs::set_pref!("layout.css.line-clamp.enabled", true);
        stylo_static_prefs::set_pref!("layout.variable_fonts.enabled", true);
        stylo_static_prefs::set_pref!("layout.css.style-queries.enabled", true);
        stylo_static_prefs::set_pref!("layout.css.ellipse-corners.enabled", true);
        stylo_static_prefs::set_pref!("layout.unimplemented", true);
    });
}

pub(crate) fn build_device(config: &DeviceConfig) -> Device {
    let prefers = match config.color_scheme {
        ColorScheme::Light => PrefersColorScheme::Light,
        ColorScheme::Dark => PrefersColorScheme::Dark,
    };
    let pointer = match config.pointer {
        PointerKind::FineHover => PointerCapabilities::FINE | PointerCapabilities::HOVER,
        PointerKind::CoarseTouch => PointerCapabilities::COARSE,
    };
    let default_values =
        ComputedValues::initial_values_with_font_override(StyleFont::initial_values());
    let w = config.viewport_width.max(MIN_VIEWPORT_PX);
    let h = config.viewport_height.max(MIN_VIEWPORT_PX);
    let dpr = config.dpr.max(MIN_DPR);
    Device::new(
        MediaType::screen(),
        QuirksMode::NoQuirks,
        Size2D::new(w, h),
        Size2D::new(
            (w * dpr).max(MIN_VIEWPORT_PX),
            (h * dpr).max(MIN_VIEWPORT_PX),
        ),
        Scale::new(dpr),
        Box::new(KruiFontMetricsProvider),
        default_values,
        prefers,
        pointer,
        pointer,
    )
}
