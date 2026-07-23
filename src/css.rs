use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    /// Win32 COLORREF packs bytes as 0x00BBGGRR.
    pub fn colorref(self) -> u32 {
        (self.r as u32) | ((self.g as u32) << 8) | ((self.b as u32) << 16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Edges {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

/// Horizontal alignment of a module's (possibly multi-line) text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    pub color: Color,
    pub background: Option<Color>,
    pub font_family: String,
    pub font_size: i32,
    pub font_weight: i32,
    pub padding: Edges,
    pub margin: Edges,
    pub text_align: TextAlign,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            color: Color { r: 0xd0, g: 0xd0, b: 0xd0 },
            background: None,
            font_family: "Segoe UI".to_string(),
            font_size: 12,
            font_weight: 400,
            padding: Edges::default(),
            margin: Edges::default(),
            text_align: TextAlign::Center,
        }
    }
}

/// Parse `#rgb` or `#rrggbb`.
pub fn parse_color(s: &str) -> Option<Color> {
    let hex = s.trim().strip_prefix('#')?;
    match hex.len() {
        3 => {
            let n = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok();
            let (r, g, b) = (n(0)?, n(1)?, n(2)?);
            Some(Color { r: r * 17, g: g * 17, b: b * 17 }) // 0xF -> 0xFF
        }
        6 => {
            let n = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            Some(Color { r: n(0)?, g: n(2)?, b: n(4)? })
        }
        _ => None,
    }
}

/// Parse an integer pixel length, with or without a `px` suffix.
pub fn parse_px(s: &str) -> Option<i32> {
    let t = s.trim();
    let num = t.strip_suffix("px").unwrap_or(t).trim();
    num.parse::<i32>().ok()
}

/// Parse a font weight: `normal`, `bold`, or a `100`–`900` number.
pub fn parse_weight(s: &str) -> Option<i32> {
    match s.trim() {
        "normal" => Some(400),
        "bold" => Some(700),
        n => {
            let w = n.parse::<i32>().ok()?;
            (100..=900).contains(&w).then_some(w)
        }
    }
}

/// Parse a horizontal text alignment: `left`, `center`, or `right`.
pub fn parse_text_align(s: &str) -> Option<TextAlign> {
    match s.trim() {
        "left" => Some(TextAlign::Left),
        "center" => Some(TextAlign::Center),
        "right" => Some(TextAlign::Right),
        _ => None,
    }
}

/// Parse 1–4 CSS-shorthand `px` values into T/R/B/L edges.
pub fn parse_edges(s: &str) -> Option<Edges> {
    let parts: Vec<i32> = s
        .split_whitespace()
        .map(parse_px)
        .collect::<Option<Vec<_>>>()?;
    let e = match parts.as_slice() {
        [a] => Edges { top: *a, right: *a, bottom: *a, left: *a },
        [a, b] => Edges { top: *a, right: *b, bottom: *a, left: *b },
        [a, b, c] => Edges { top: *a, right: *b, bottom: *c, left: *b },
        [a, b, c, d] => Edges { top: *a, right: *b, bottom: *c, left: *d },
        _ => return None,
    };
    Some(e)
}

/// Merge top-level defaults then a module's own css into a resolved Style.
/// Module properties win. Invalid values and unknown properties are ignored
/// (with a warning) so one bad line never breaks the bar.
pub fn resolve(default_css: &HashMap<String, String>, module_css: &HashMap<String, String>) -> Style {
    let mut style = Style::default();
    apply(&mut style, default_css);
    apply(&mut style, module_css);
    style
}

fn apply(style: &mut Style, css: &HashMap<String, String>) {
    for (key, value) in css {
        match key.as_str() {
            "color" => set(parse_color(value), |c| style.color = c, key, value),
            "background-color" => set(parse_color(value), |c| style.background = Some(c), key, value),
            "font-family" => style.font_family = value.trim().to_string(),
            "font-size" => set(parse_px(value), |px| style.font_size = px, key, value),
            "font-weight" => set(parse_weight(value), |w| style.font_weight = w, key, value),
            "padding" => set(parse_edges(value), |e| style.padding = e, key, value),
            "margin" => set(parse_edges(value), |e| style.margin = e, key, value),
            "text-align" => set(parse_text_align(value), |a| style.text_align = a, key, value),
            other => eprintln!("Winbar: unknown css property '{other}' (ignored)"),
        }
    }
}

/// Apply a parsed value, or warn and leave the current value unchanged.
fn set<T>(parsed: Option<T>, mut assign: impl FnMut(T), key: &str, value: &str) {
    match parsed {
        Some(v) => assign(v),
        None => eprintln!("Winbar: invalid value '{value}' for css '{key}' (ignored)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn css(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn module_css_overrides_defaults() {
        let defaults = css(&[("color", "#d0d0d0"), ("font-size", "12px"), ("padding", "0 8px")]);
        let module = css(&[("color", "#7fdbb0"), ("font-weight", "bold")]);
        let style = resolve(&defaults, &module);

        assert_eq!(style.color, Color { r: 0x7f, g: 0xdb, b: 0xb0 }); // overridden
        assert_eq!(style.font_size, 12); // from defaults
        assert_eq!(style.font_weight, 700); // from module
        assert_eq!(style.padding, Edges { top: 0, right: 8, bottom: 0, left: 8 });
        assert_eq!(style.background, None); // never set
    }

    #[test]
    fn background_color_is_applied() {
        let style = resolve(&HashMap::new(), &css(&[("background-color", "#303040")]));
        assert_eq!(style.background, Some(Color { r: 0x30, g: 0x30, b: 0x40 }));
    }

    #[test]
    fn invalid_and_unknown_values_are_ignored() {
        // bad color keeps the default; unknown property is dropped
        let style = resolve(&HashMap::new(), &css(&[("color", "notacolor"), ("wobble", "3")]));
        assert_eq!(style.color, Style::default().color);
    }

    #[test]
    fn default_style_is_light_gray_segoe_12() {
        let s = Style::default();
        assert_eq!(s.color, Color { r: 0xd0, g: 0xd0, b: 0xd0 });
        assert_eq!(s.background, None);
        assert_eq!(s.font_family, "Segoe UI");
        assert_eq!(s.font_size, 12);
        assert_eq!(s.font_weight, 400);
        assert_eq!(s.padding, Edges::default());
    }

    #[test]
    fn colorref_is_bgr_packed() {
        // R=0x11 G=0x22 B=0x33 -> 0x00332211
        assert_eq!(Color { r: 0x11, g: 0x22, b: 0x33 }.colorref(), 0x0033_2211);
    }

    #[test]
    fn parses_hex_colors() {
        assert_eq!(parse_color("#ffffff"), Some(Color { r: 255, g: 255, b: 255 }));
        assert_eq!(parse_color("#000000"), Some(Color { r: 0, g: 0, b: 0 }));
        assert_eq!(parse_color("#7fdbb0"), Some(Color { r: 0x7f, g: 0xdb, b: 0xb0 }));
        // 3-digit shorthand expands each nibble
        assert_eq!(parse_color("#fff"), Some(Color { r: 255, g: 255, b: 255 }));
        assert_eq!(parse_color("#123"), Some(Color { r: 0x11, g: 0x22, b: 0x33 }));
        assert_eq!(parse_color("nope"), None);
        assert_eq!(parse_color("#12"), None);
    }

    #[test]
    fn parses_px_lengths() {
        assert_eq!(parse_px("12px"), Some(12));
        assert_eq!(parse_px("  8 "), Some(8));
        assert_eq!(parse_px("0"), Some(0));
        assert_eq!(parse_px("abc"), None);
    }

    #[test]
    fn parses_font_weight() {
        assert_eq!(parse_weight("normal"), Some(400));
        assert_eq!(parse_weight("bold"), Some(700));
        assert_eq!(parse_weight("600"), Some(600));
        assert_eq!(parse_weight("50"), None);
        assert_eq!(parse_weight("999"), None);
    }

    #[test]
    fn parses_text_align() {
        assert_eq!(parse_text_align("left"), Some(TextAlign::Left));
        assert_eq!(parse_text_align(" center "), Some(TextAlign::Center));
        assert_eq!(parse_text_align("right"), Some(TextAlign::Right));
        assert_eq!(parse_text_align("justify"), None);
        assert_eq!(parse_text_align(""), None);
    }

    #[test]
    fn text_align_defaults_to_center_and_overrides() {
        assert_eq!(Style::default().text_align, TextAlign::Center);
        let style = resolve(&HashMap::new(), &css(&[("text-align", "right")]));
        assert_eq!(style.text_align, TextAlign::Right);
        // an invalid value keeps the default
        let style = resolve(&HashMap::new(), &css(&[("text-align", "sideways")]));
        assert_eq!(style.text_align, TextAlign::Center);
    }

    #[test]
    fn parses_edges_shorthand() {
        assert_eq!(parse_edges("4px"), Some(Edges { top: 4, right: 4, bottom: 4, left: 4 }));
        assert_eq!(parse_edges("0 8px"), Some(Edges { top: 0, right: 8, bottom: 0, left: 8 }));
        assert_eq!(
            parse_edges("1 2 3 4"),
            Some(Edges { top: 1, right: 2, bottom: 3, left: 4 })
        );
        assert_eq!(parse_edges("1 2 3 4 5"), None);
        assert_eq!(parse_edges("bad"), None);
    }
}
