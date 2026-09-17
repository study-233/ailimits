//! Shared font selection for measurement and rasterization on every surface.
use fontdue::layout::{CoordinateSystem, GlyphPosition, Layout, TextStyle};
use fontdue::{Font, FontSettings};
use std::sync::OnceLock;

pub(crate) struct Fonts {
    latin: Font,
    cjk: OnceLock<Option<Font>>,
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn chinese_glyphs_share_measurement_and_render_fonts() {
        let fonts = fonts().expect("Windows UI fonts");
        for character in "中文用量会话每周重置小时分钟前网络代理".chars() {
            assert!(fonts.supports(character), "missing glyph: {character}");
        }
        let layout = fonts.layout("Codex 会话 42%", 14.0);
        assert!(layout.glyphs().iter().any(|g| g.font_index == 1));
        for glyph in layout.glyphs().iter().filter(|g| !g.parent.is_whitespace()) {
            let (metrics, bitmap) = fonts.rasterize(glyph);
            assert_eq!(bitmap.len(), metrics.width * metrics.height);
            assert!(bitmap.iter().any(|&pixel| pixel > 0));
        }
        for width in [0.0, 1.0, 30.0, 60.0, 150.0] {
            let text = fonts.fit("每周用量：12小时30分钟后重置", 12.0, width);
            assert!(fonts.width(&text, 12.0) <= width);
        }
    }
}

fn load(names: &[&str]) -> Option<Font> {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let dir = std::path::Path::new(&root).join("Fonts");
    names.iter().find_map(|name| {
        let bytes = std::fs::read(dir.join(name)).ok()?;
        Font::from_bytes(bytes, FontSettings::default()).ok()
    })
}

pub(crate) fn fonts() -> Option<&'static Fonts> {
    static FONTS: OnceLock<Option<Fonts>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            Some(Fonts {
                latin: load(&[
                    "SegUIVar.ttf",
                    "segoeui.ttf",
                    "arial.ttf",
                    "tahoma.ttf",
                    "verdana.ttf",
                    "msyh.ttc",
                ])?,
                cjk: OnceLock::new(),
            })
        })
        .as_ref()
}

/// Shared numeric face; retain the regular face as the platform fallback.
pub(crate) fn number_font() -> Option<&'static Font> {
    static NUMBER: OnceLock<Option<Font>> = OnceLock::new();
    NUMBER
        .get_or_init(|| load(&["seguisb.ttf"]))
        .as_ref()
        .or_else(|| fonts().map(|f| &f.latin))
}

pub(crate) fn weighted_number_font(
    weight: crate::config::appearance::NumberWeight,
) -> Option<&'static Font> {
    use crate::config::appearance::NumberWeight;
    static BOLD: OnceLock<Option<Font>> = OnceLock::new();
    match weight {
        NumberWeight::Regular => fonts().map(|f| &f.latin),
        NumberWeight::Semibold => number_font(),
        NumberWeight::Bold => BOLD
            .get_or_init(|| load(&["segoeuib.ttf"]))
            .as_ref()
            .or_else(number_font),
    }
}

impl Fonts {
    fn fallback(&self) -> Option<&Font> {
        self.cjk
            .get_or_init(|| load(&["msyh.ttc", "msyh.ttf", "simsun.ttc", "msjh.ttc"]))
            .as_ref()
    }

    #[cfg(test)]
    pub(crate) fn supports(&self, c: char) -> bool {
        self.latin.lookup_glyph_index(c) != 0
            || self
                .fallback()
                .is_some_and(|f| f.lookup_glyph_index(c) != 0)
    }

    pub(crate) fn layout(&self, text: &str, size: f32) -> Layout {
        let fallback = text
            .chars()
            .any(|c| self.latin.lookup_glyph_index(c) == 0)
            .then(|| self.fallback())
            .flatten();
        let mut fonts = vec![&self.latin];
        if let Some(font) = fallback {
            fonts.push(font);
        }
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        // Append runs with a shared baseline and the same font index used at
        // rasterization time; measuring Latin and Chinese separately drifts.
        let mut start = 0;
        let mut index = 0;
        for (offset, c) in text.char_indices() {
            let selected = usize::from(self.latin.lookup_glyph_index(c) == 0 && fallback.is_some());
            if selected != index {
                if start < offset {
                    layout.append(&fonts, &TextStyle::new(&text[start..offset], size, index));
                }
                start = offset;
                index = selected;
            }
        }
        if start < text.len() {
            layout.append(&fonts, &TextStyle::new(&text[start..], size, index));
        }
        layout
    }

    pub(crate) fn rasterize(&self, glyph: &GlyphPosition) -> (fontdue::Metrics, Vec<u8>) {
        let font = if glyph.font_index == 1 {
            self.fallback().unwrap_or(&self.latin)
        } else {
            &self.latin
        };
        font.rasterize_config(glyph.key)
    }

    #[cfg(test)]
    pub(crate) fn width(&self, text: &str, size: f32) -> f32 {
        self.layout(text, size)
            .glyphs()
            .iter()
            .map(|g| g.x + g.width as f32)
            .fold(0.0, f32::max)
    }

    #[cfg(test)]
    pub(crate) fn fit(&self, text: &str, size: f32, width: f32) -> String {
        if self.width(text, size) <= width {
            return text.to_string();
        }
        if self.width("…", size) > width {
            return String::new();
        }
        // Search UTF-8 boundaries instead of repeatedly laying out a long
        // external error one character shorter (quadratic work per frame).
        let ends: Vec<usize> = std::iter::once(0)
            .chain(text.char_indices().skip(1).map(|(offset, _)| offset))
            .collect();
        let (mut low, mut high) = (0, ends.len());
        while low + 1 < high {
            let mid = (low + high) / 2;
            if self.width(&format!("{}…", &text[..ends[mid]]), size) <= width {
                low = mid;
            } else {
                high = mid;
            }
        }
        format!("{}…", &text[..ends[low]])
    }
}
