//! CSS color value representation and parser (`#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`, `rgb()`, `rgba()`, `hsl()`, `hsla()`, and standard named colors).

/// RGBA color with 8-bit channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Converts RGBA8 to normalized `[f32; 4]` for GPU shaders.
    pub fn to_f32_array(self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }

    /// Parses `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`, `rgb(...)`, `rgba(...)`, `hsl(...)`, `hsla(...)`, or CSS named colors.
    pub fn parse_css(input: &str) -> Option<Self> {
        let s = input.trim();
        if let Some(hex) = s.strip_prefix('#') {
            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            return match hex.len() {
                3 => {
                    let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                    let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                    let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                    Some(Self::rgb(r, g, b))
                }
                4 => {
                    let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                    let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                    let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                    let a = u8::from_str_radix(&hex[3..4], 16).ok()? * 17;
                    Some(Self::rgba(r, g, b, a))
                }
                6 => {
                    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                    Some(Self::rgb(r, g, b))
                }
                8 => {
                    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                    let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                    Some(Self::rgba(r, g, b, a))
                }
                _ => None,
            };
        }

        let lower = s.to_ascii_lowercase();
        if let Some(inner) = lower
            .strip_prefix("rgba(")
            .or_else(|| lower.strip_prefix("rgb("))
            .and_then(|rest| rest.strip_suffix(')'))
        {
            return parse_rgb_fn(inner);
        }
        if let Some(inner) = lower
            .strip_prefix("hsla(")
            .or_else(|| lower.strip_prefix("hsl("))
            .and_then(|rest| rest.strip_suffix(')'))
        {
            return parse_hsl_fn(inner);
        }

        parse_named_color(&lower)
    }
}

fn split_color_args(inner: &str) -> Vec<&str> {
    if inner.contains(',') {
        inner.split(',').map(str::trim).collect()
    } else {
        inner
            .split(|c: char| c.is_whitespace() || c == '/')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect()
    }
}

fn parse_channel_u8(raw: &str) -> Option<u8> {
    let trimmed = raw.trim();
    if let Some(pct) = trimmed.strip_suffix('%') {
        let v = pct.trim().parse::<f32>().ok()?.clamp(0.0, 100.0);
        Some((v * 2.55).round() as u8)
    } else {
        let v = trimmed.parse::<f32>().ok()?.clamp(0.0, 255.0);
        Some(v.round() as u8)
    }
}

fn parse_alpha_u8(raw: &str) -> Option<u8> {
    let trimmed = raw.trim();
    if let Some(pct) = trimmed.strip_suffix('%') {
        let v = pct.trim().parse::<f32>().ok()?.clamp(0.0, 100.0);
        Some((v * 2.55).round() as u8)
    } else {
        let v = trimmed.parse::<f32>().ok()?.clamp(0.0, 1.0);
        Some((v * 255.0).round() as u8)
    }
}

fn parse_rgb_fn(inner: &str) -> Option<Color> {
    let args = split_color_args(inner);
    match args.len() {
        3 => Some(Color::rgb(
            parse_channel_u8(args[0])?,
            parse_channel_u8(args[1])?,
            parse_channel_u8(args[2])?,
        )),
        4 => Some(Color::rgba(
            parse_channel_u8(args[0])?,
            parse_channel_u8(args[1])?,
            parse_channel_u8(args[2])?,
            parse_alpha_u8(args[3])?,
        )),
        _ => None,
    }
}

fn parse_hsl_fn(inner: &str) -> Option<Color> {
    let args = split_color_args(inner);
    if args.len() != 3 && args.len() != 4 {
        return None;
    }
    let h_str = args[0].strip_suffix("deg").unwrap_or(args[0]).trim();
    let h = h_str.parse::<f32>().ok()?.rem_euclid(360.0);
    let s = args[1]
        .strip_suffix('%')?
        .trim()
        .parse::<f32>()
        .ok()?
        .clamp(0.0, 100.0)
        / 100.0;
    let l = args[2]
        .strip_suffix('%')?
        .trim()
        .parse::<f32>()
        .ok()?
        .clamp(0.0, 100.0)
        / 100.0;
    let a = if args.len() == 4 {
        parse_alpha_u8(args[3])?
    } else {
        255
    };

    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c * 0.5;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let r = ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    let b = ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Some(Color::rgba(r, g, b, a))
}

fn parse_named_color(lower: &str) -> Option<Color> {
    Some(match lower {
        "transparent" => Color::TRANSPARENT,
        "black" => Color::BLACK,
        "white" => Color::WHITE,
        "red" => Color::rgb(255, 0, 0),
        "green" => Color::rgb(0, 128, 0),
        "lime" => Color::rgb(0, 255, 0),
        "blue" => Color::rgb(0, 0, 255),
        "yellow" => Color::rgb(255, 255, 0),
        "cyan" | "aqua" => Color::rgb(0, 255, 255),
        "magenta" | "fuchsia" => Color::rgb(255, 0, 255),
        "gray" | "grey" => Color::rgb(128, 128, 128),
        "silver" => Color::rgb(192, 192, 192),
        "maroon" => Color::rgb(128, 0, 0),
        "olive" => Color::rgb(128, 128, 0),
        "purple" => Color::rgb(128, 0, 128),
        "teal" => Color::rgb(0, 128, 128),
        "navy" => Color::rgb(0, 0, 128),
        "orange" => Color::rgb(255, 165, 0),
        "orangered" => Color::rgb(255, 69, 0),
        "coral" => Color::rgb(255, 127, 80),
        "tomato" => Color::rgb(255, 99, 71),
        "gold" => Color::rgb(255, 215, 0),
        "pink" => Color::rgb(255, 192, 203),
        "hotpink" => Color::rgb(255, 105, 180),
        "deeppink" => Color::rgb(255, 20, 147),
        "violet" => Color::rgb(238, 130, 238),
        "indigo" => Color::rgb(75, 0, 130),
        "rebeccapurple" => Color::rgb(102, 51, 153),
        other => return parse_extended_named_color(other),
    })
}

fn parse_extended_named_color(lower: &str) -> Option<Color> {
    Some(match lower {
        "skyblue" => Color::rgb(135, 206, 235),
        "deepskyblue" => Color::rgb(0, 191, 255),
        "dodgerblue" => Color::rgb(30, 144, 255),
        "royalblue" => Color::rgb(65, 105, 225),
        "steelblue" => Color::rgb(70, 130, 180),
        "turquoise" => Color::rgb(64, 224, 208),
        "aquamarine" => Color::rgb(127, 255, 212),
        "springgreen" => Color::rgb(0, 255, 127),
        "limegreen" => Color::rgb(50, 205, 50),
        "forestgreen" => Color::rgb(34, 139, 34),
        "seagreen" => Color::rgb(46, 139, 87),
        "crimson" => Color::rgb(220, 20, 60),
        "firebrick" => Color::rgb(178, 34, 34),
        "darkred" => Color::rgb(139, 0, 0),
        "darkblue" => Color::rgb(0, 0, 139),
        "darkgreen" => Color::rgb(0, 100, 0),
        "darkgray" | "darkgrey" => Color::rgb(169, 169, 169),
        "dimgray" | "dimgrey" => Color::rgb(105, 105, 105),
        "lightgray" | "lightgrey" => Color::rgb(211, 211, 211),
        "slategray" | "slategrey" => Color::rgb(112, 128, 144),
        "darkslategray" | "darkslategrey" => Color::rgb(47, 79, 79),
        "whitesmoke" => Color::rgb(245, 245, 245),
        "aliceblue" => Color::rgb(240, 248, 255),
        "ivory" => Color::rgb(255, 255, 240),
        "beige" => Color::rgb(245, 245, 220),
        "wheat" => Color::rgb(245, 222, 179),
        "tan" => Color::rgb(210, 180, 140),
        "brown" => Color::rgb(165, 42, 42),
        "chocolate" => Color::rgb(210, 105, 30),
        "salmon" => Color::rgb(250, 128, 114),
        "khaki" => Color::rgb(240, 230, 140),
        "plum" => Color::rgb(221, 160, 221),
        "orchid" => Color::rgb(218, 112, 214),
        "lavender" => Color::rgb(230, 230, 250),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_css_colors_correctly() {
        assert_eq!(Color::parse_css("#ff0080"), Some(Color::rgb(255, 0, 128)));
        assert_eq!(
            Color::parse_css("#f088"),
            Some(Color::rgba(255, 0, 136, 136))
        );
        assert_eq!(
            Color::parse_css("#11223344"),
            Some(Color::rgba(0x11, 0x22, 0x33, 0x44))
        );
        assert_eq!(
            Color::parse_css("rgb(15, 108, 189)"),
            Some(Color::rgb(15, 108, 189))
        );
        assert_eq!(
            Color::parse_css("rgba(255, 0, 0, 0.5)"),
            Some(Color::rgba(255, 0, 0, 128))
        );
        assert_eq!(
            Color::parse_css("hsl(0, 100%, 50%)"),
            Some(Color::rgb(255, 0, 0))
        );
        assert_eq!(
            Color::parse_css("hsla(120, 100%, 50%, 0.5)"),
            Some(Color::rgba(0, 255, 0, 128))
        );
        assert_eq!(Color::parse_css("white"), Some(Color::WHITE));
        assert_eq!(
            Color::parse_css("rebeccapurple"),
            Some(Color::rgb(102, 51, 153))
        );
        assert_eq!(Color::parse_css("#가"), None);
        assert_eq!(Color::parse_css("#가나다"), None);
    }
}
