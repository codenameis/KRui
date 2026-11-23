//! Minimal, dependency-free JSON parser for `tests/fixtures/real_css/tailwind_v4/oracle.json`.

use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct ShadowOracle {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug)]
pub struct NodeOracle {
    pub display: String,
    pub rect: [f32; 4],
    pub background_color: [u8; 4],
    pub color: [u8; 4],
    pub border_top_width: f32,
    pub border_top_color: [u8; 4],
    pub border_radius: f32,
    pub font_size: f32,
    pub font_weight: u16,
    pub opacity: f32,
    pub has_gradient: bool,
    pub shadows: Vec<ShadowOracle>,
    pub placeholder_color: [u8; 4],
    pub filter: String,
}

#[derive(Clone, Debug)]
pub struct OracleDocument {
    pub viewport: [u32; 2],
    pub tailwind_normal: BTreeMap<String, NodeOracle>,
    pub tailwind_optimize: BTreeMap<String, NodeOracle>,
    pub probes: BTreeMap<String, NodeOracle>,
}

#[derive(Clone, Debug)]
enum JsonVal {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<JsonVal>),
    Obj(BTreeMap<String, JsonVal>),
}

impl JsonVal {
    fn as_obj(&self) -> &BTreeMap<String, JsonVal> {
        match self {
            Self::Obj(m) => m,
            other => panic!("expected JSON object, got {other:?}"),
        }
    }

    fn as_arr(&self) -> &[JsonVal] {
        match self {
            Self::Arr(a) => a,
            other => panic!("expected JSON array, got {other:?}"),
        }
    }

    fn as_f32(&self) -> f32 {
        match self {
            Self::Num(n) => *n as f32,
            other => panic!("expected JSON number, got {other:?}"),
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Str(s) => s,
            other => panic!("expected JSON string, got {other:?}"),
        }
    }

    fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            other => panic!("expected JSON bool, got {other:?}"),
        }
    }
}

struct JsonCursor<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> JsonCursor<'a> {
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn parse_val(&mut self) -> JsonVal {
        self.skip_ws();
        match self.s[self.i] {
            b'{' => self.parse_obj(),
            b'[' => self.parse_arr(),
            b'"' => JsonVal::Str(self.parse_str()),
            b't' => {
                self.i += 4;
                JsonVal::Bool(true)
            }
            b'f' => {
                self.i += 5;
                JsonVal::Bool(false)
            }
            b'n' => {
                self.i += 4;
                JsonVal::Null
            }
            _ => self.parse_num(),
        }
    }

    fn parse_str(&mut self) -> String {
        assert_eq!(self.s[self.i], b'"');
        self.i += 1;
        let mut out = String::new();
        while self.i < self.s.len() {
            let b = self.s[self.i];
            self.i += 1;
            if b == b'"' {
                return out;
            }
            if b == b'\\' {
                let esc = self.s[self.i];
                self.i += 1;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'n' => out.push('\n'),
                    b't' => out.push('\t'),
                    other => out.push(other as char),
                }
            } else {
                out.push(b as char);
            }
        }
        panic!("unterminated JSON string");
    }

    fn parse_num(&mut self) -> JsonVal {
        let start = self.i;
        while self.i < self.s.len()
            && matches!(
                self.s[self.i],
                b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E'
            )
        {
            self.i += 1;
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).expect("num utf8");
        JsonVal::Num(text.parse::<f64>().expect("valid JSON float"))
    }

    fn parse_arr(&mut self) -> JsonVal {
        self.i += 1;
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            if self.s[self.i] == b']' {
                self.i += 1;
                return JsonVal::Arr(out);
            }
            out.push(self.parse_val());
            self.skip_ws();
            if self.s[self.i] == b',' {
                self.i += 1;
            }
        }
    }

    fn parse_obj(&mut self) -> JsonVal {
        self.i += 1;
        let mut out = BTreeMap::new();
        loop {
            self.skip_ws();
            if self.s[self.i] == b'}' {
                self.i += 1;
                return JsonVal::Obj(out);
            }
            let k = self.parse_str();
            self.skip_ws();
            assert_eq!(self.s[self.i], b':');
            self.i += 1;
            let v = self.parse_val();
            out.insert(k, v);
            self.skip_ws();
            if self.s[self.i] == b',' {
                self.i += 1;
            }
        }
    }
}

fn decode_rgba(v: &JsonVal) -> [u8; 4] {
    let a = v.as_arr();
    [
        a[0].as_f32().round() as u8,
        a[1].as_f32().round() as u8,
        a[2].as_f32().round() as u8,
        a[3].as_f32().round() as u8,
    ]
}

fn decode_node_map(v: &JsonVal) -> BTreeMap<String, NodeOracle> {
    let mut map = BTreeMap::new();
    for (id, raw) in v.as_obj() {
        let o = raw.as_obj();
        let r = o["rect"].as_arr();
        let shadows = o["shadows"]
            .as_arr()
            .iter()
            .map(|sv| {
                let sm = sv.as_obj();
                ShadowOracle {
                    offset_x: sm["offset_x"].as_f32(),
                    offset_y: sm["offset_y"].as_f32(),
                    blur: sm["blur"].as_f32(),
                    spread: sm["spread"].as_f32(),
                    color: decode_rgba(&sm["color"]),
                }
            })
            .collect();
        map.insert(
            id.clone(),
            NodeOracle {
                display: o["display"].as_str().to_string(),
                rect: [r[0].as_f32(), r[1].as_f32(), r[2].as_f32(), r[3].as_f32()],
                background_color: decode_rgba(&o["background_color"]),
                color: decode_rgba(&o["color"]),
                border_top_width: o["border_top_width"].as_f32(),
                border_top_color: decode_rgba(&o["border_top_color"]),
                border_radius: o["border_radius"].as_f32(),
                font_size: o["font_size"].as_f32(),
                font_weight: o["font_weight"].as_f32().round() as u16,
                opacity: o["opacity"].as_f32(),
                has_gradient: o["has_gradient"].as_bool(),
                shadows,
                placeholder_color: decode_rgba(&o["placeholder_color"]),
                filter: o["filter"].as_str().to_string(),
            },
        );
    }
    map
}

pub fn parse_oracle_json(text: &str) -> OracleDocument {
    let mut cur = JsonCursor {
        s: text.as_bytes(),
        i: 0,
    };
    let root = cur.parse_val();
    let _ = JsonVal::Null;
    let obj = root.as_obj();
    let vp = obj["viewport"].as_arr();
    OracleDocument {
        viewport: [vp[0].as_f32() as u32, vp[1].as_f32() as u32],
        tailwind_normal: decode_node_map(&obj["tailwind_normal"]),
        tailwind_optimize: decode_node_map(&obj["tailwind_optimize"]),
        probes: decode_node_map(&obj["probes"]),
    }
}
