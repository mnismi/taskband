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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    pub color: Color,
    pub background: Option<Color>,
    pub font_family: String,
    pub font_size: i32,
    pub font_weight: i32,
    pub padding: Edges,
    pub margin: Edges,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
