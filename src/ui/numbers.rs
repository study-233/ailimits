//! Two-line percentage and provider label, measured with the rendering fonts.
use super::fonts::weighted_number_font;
use super::weekly::WeeklyQuota;
use crate::config::appearance::{Appearance, NumberWeight};
use tiny_skia::{Color, Pixmap};

struct Glyph {
    metrics: fontdue::Metrics,
    character: char,
    size: f32,
    x: f32,
}

fn layout(quota: &WeeklyQuota, height: f32, style: &Appearance) -> Vec<Glyph> {
    let Some(font) = weighted_number_font(style.number_weight) else {
        return Vec::new();
    };
    let em = height * 100.0 / font.metrics('0', 100.0).height.max(1) as f32;
    let cell = ('0'..='9')
        .map(|c| font.metrics(c, em).advance_width)
        .fold(0.0, f32::max);
    let mut glyphs = Vec::new();
    let symbol_scale = style.symbol_percent as f32 / 100.0;
    let mut append = |c: char, size: f32, x: f32| {
        let metrics = font.metrics(c, size);
        glyphs.push(Glyph {
            metrics,
            character: c,
            size,
            x,
        });
    };
    if let Some(value) = quota.remaining {
        let text = format!("{}", value.round() as u32);
        // Equal-width digit cells keep readings of the same length steady.
        let start = 0.0;
        if quota.estimated {
            append('≈', em * symbol_scale, start - symbol_scale * height);
        }
        for (i, c) in text.chars().enumerate() {
            let advance = font.metrics(c, em).advance_width;
            append(c, em, start + i as f32 * cell + (cell - advance) / 2.0);
        }
        append(
            '%',
            em * symbol_scale,
            text.len() as f32 * cell + height * 0.12,
        );
    } else {
        append('—', em, cell);
    }
    glyphs
}

#[derive(Clone, Copy, Default)]
struct Bounds {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Bounds {
    fn width(self) -> f32 {
        self.right - self.left
    }
    fn height(self) -> f32 {
        self.bottom - self.top
    }
}

fn bounds(glyphs: &[Glyph]) -> Bounds {
    let mut result = Bounds {
        left: f32::INFINITY,
        top: f32::INFINITY,
        right: f32::NEG_INFINITY,
        bottom: f32::NEG_INFINITY,
    };
    for g in glyphs
        .iter()
        .filter(|g| g.metrics.width > 0 && g.metrics.height > 0)
    {
        let left = g.x + g.metrics.xmin as f32;
        let bottom = -g.metrics.ymin as f32;
        result.left = result.left.min(left);
        result.top = result.top.min(bottom - g.metrics.height as f32);
        result.right = result.right.max(left + g.metrics.width as f32);
        result.bottom = result.bottom.max(bottom);
    }
    if result.left.is_finite() {
        result
    } else {
        Bounds::default()
    }
}

fn provider_label(height: f32, window: crate::providers::MetricWindow) -> Vec<Glyph> {
    let Some(font) = weighted_number_font(NumberWeight::Regular) else {
        return Vec::new();
    };
    let mut size = height * 100.0 / font.metrics('C', 100.0).height.max(1) as f32;
    loop {
        let mut x = 0.0;
        let text = if window == crate::providers::MetricWindow::Long {
            "Codex · 7d"
        } else {
            "Codex · 5h"
        };
        let glyphs: Vec<_> = text
            .chars()
            .map(|character| {
                let metrics = font.metrics(character, size);
                let g = Glyph {
                    metrics,
                    character,
                    size,
                    x,
                };
                x += metrics.advance_width;
                g
            })
            .collect();
        if bounds(&glyphs).height() <= height || size <= 1.0 {
            return glyphs;
        }
        size = (size - 0.1).max(1.0);
    }
}

struct Block {
    digits: Vec<Glyph>,
    label: Vec<Glyph>,
    digits_x: f32,
    label_x: f32,
    digits_baseline: f32,
    label_baseline: f32,
    height: f32,
}

fn block(quota: &WeeklyQuota, style: &Appearance, scale: f32, available_height: f32) -> Block {
    let label = provider_label(9.0 * scale, quota.window);
    let label_bounds = bounds(&label);
    let gap = (2.0 * scale).round();
    let available_digits = (available_height - gap - label_bounds.height()).max(1.0);
    let mut requested = style.number_size as f32 * scale;
    // The bound includes the percent sign and estimate marker. Only shrink
    // when the actual ink does not fit; never change the persisted setting.
    let (digits, digit_bounds) = loop {
        let digits = layout(quota, requested, style);
        let b = bounds(&digits);
        if b.height() <= available_digits || requested <= 1.0 {
            break (digits, b);
        }
        requested = (requested - 0.25).max(1.0);
    };
    let width = digit_bounds.width().max(label_bounds.width());
    Block {
        digits,
        label,
        digits_x: (width - digit_bounds.width()) / 2.0 - digit_bounds.left,
        label_x: (width - label_bounds.width()) / 2.0 - label_bounds.left,
        digits_baseline: -digit_bounds.top,
        label_baseline: digit_bounds.height() + gap - label_bounds.top,
        height: digit_bounds.height() + gap + label_bounds.height(),
    }
}

/// Reserve stable space for every valid reading and the provider label.
pub(crate) fn width(style: &Appearance, scale: f32) -> f32 {
    let mut quota = WeeklyQuota::from_providers(&[]);
    quota.estimated = true;
    let h = style.number_size as f32 * scale;
    let number_width = (0..=100)
        .map(|value| {
            quota.remaining = Some(value as f32);
            bounds(&layout(&quota, h, style)).width()
        })
        .fold(0.0, f32::max)
        .max(bounds(&layout(&WeeklyQuota::from_providers(&[]), h, style)).width());
    number_width.max(
        bounds(&provider_label(
            9.0 * scale,
            crate::providers::MetricWindow::Long,
        ))
        .width(),
    ) + 2.0
}

pub(crate) fn draw(
    pm: &mut Pixmap,
    quota: &WeeklyQuota,
    x: f32,
    scale: f32,
    ink: Color,
    style: &Appearance,
) {
    let margin = scale.ceil();
    let block = block(quota, style, scale, pm.height() as f32 - 2.0 * margin);
    let top = ((pm.height() as f32 - block.height) / 2.0 + style.number_y as f32 * scale)
        .clamp(
            margin,
            (pm.height() as f32 - margin - block.height).max(margin),
        )
        .round();
    draw_line(
        pm,
        &block.digits,
        (x + block.digits_x, top + block.digits_baseline),
        ink,
        style.number_weight,
    );
    draw_line(
        pm,
        &block.label,
        (x + block.label_x, top + block.label_baseline),
        {
            let mut secondary = ink;
            secondary.set_alpha(0.72);
            secondary
        },
        NumberWeight::Regular,
    );
}

fn draw_line(
    pm: &mut Pixmap,
    glyphs: &[Glyph],
    origin: (f32, f32),
    ink: Color,
    weight: NumberWeight,
) {
    let Some(font) = weighted_number_font(weight) else {
        return;
    };
    for glyph in glyphs {
        let metrics = glyph.metrics;
        let left = (origin.0 + glyph.x + metrics.xmin as f32).round() as i32;
        let top = (origin.1 - metrics.height as f32 - metrics.ymin as f32).round() as i32;
        let (_, bitmap) = font.rasterize(glyph.character, glyph.size);
        for (i, &coverage) in bitmap.iter().enumerate() {
            let px = left + (i % metrics.width) as i32;
            let py = top + (i / metrics.width) as i32;
            if px < 0 || py < 0 || px >= pm.width() as i32 || py >= pm.height() as i32 {
                continue;
            }
            let offset = ((py as u32 * pm.width() + px as u32) * 4) as usize;
            let a = (coverage as f32 * ink.alpha()).round() as u32;
            let pixel = &mut pm.data_mut()[offset..offset + 4];
            for (channel, component) in [ink.red(), ink.green(), ink.blue()].into_iter().enumerate()
            {
                pixel[channel] = ((component * 255.0) as u32 * a / 255
                    + pixel[channel] as u32 * (255 - a) / 255)
                    as u8;
            }
            pixel[3] = (a + pixel[3] as u32 * (255 - a) / 255).min(255) as u8;
        }
    }
}

/// Native-size tray digits, using the same selected numeric font and color.
pub(crate) fn draw_compact(pm: &mut Pixmap, text: &str, ink: Color, weight: NumberWeight) {
    let Some(font) = weighted_number_font(weight) else {
        return;
    };
    let mut size = pm.height() as f32;
    let glyphs = loop {
        let mut x = 0.;
        let glyphs: Vec<_> = text
            .chars()
            .map(|character| {
                let metrics = font.metrics(character, size);
                let glyph = Glyph {
                    character,
                    metrics,
                    size,
                    x,
                };
                x += metrics.advance_width;
                glyph
            })
            .collect();
        let b = bounds(&glyphs);
        if (b.width() <= pm.width() as f32 - 2. && b.height() <= pm.height() as f32 - 2.)
            || size <= 1.
        {
            break glyphs;
        }
        size = (size - 0.25).max(1.);
    };
    let b = bounds(&glyphs);
    let origin = (
        (pm.width() as f32 - b.width()) / 2. - b.left,
        (pm.height() as f32 - b.height()) / 2. - b.top,
    );
    draw_line(pm, &glyphs, origin, ink, weight);
}

/// A compact fixed-width numeric row with a colored period label.
pub(crate) fn draw_row(
    pm: &mut Pixmap,
    quota: &WeeklyQuota,
    origin: (f32, f32),
    scale: f32,
    inks: (Color, Color),
    style: &Appearance,
) {
    let glyphs = layout(quota, style.number_size as f32 * scale, style);
    let b = bounds(&glyphs);
    // Reserve the estimate marker even when it is absent; digit positions do not jump.
    let marker = style.symbol_percent as f32 / 100.0 * style.number_size as f32 * scale;
    draw_line(
        pm,
        &glyphs,
        (origin.0 + marker, origin.1 - b.top),
        inks.0,
        style.number_weight,
    );
    let Some(font) = weighted_number_font(NumberWeight::Regular) else {
        return;
    };
    let size = 9.0 * scale;
    let label = if quota.window == crate::providers::MetricWindow::Long {
        "7d"
    } else {
        "5h"
    };
    let mut x = 0.0;
    let label: Vec<_> = label
        .chars()
        .map(|character| {
            let metrics = font.metrics(character, size);
            let glyph = Glyph {
                metrics,
                character,
                size,
                x,
            };
            x += metrics.advance_width;
            glyph
        })
        .collect();
    let lb = bounds(&label);
    draw_line(
        pm,
        &label,
        (
            origin.0 + width(style, scale) + 4.0 * scale,
            origin.1 + (b.height() - lb.height()) / 2.0 - lb.top,
        ),
        inks.1,
        NumberWeight::Regular,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_lines_fit_and_share_the_same_center_for_every_style() {
        for weight in [
            NumberWeight::Regular,
            NumberWeight::Semibold,
            NumberWeight::Bold,
        ] {
            for size in [10, 20, 40] {
                for symbols in [40, 60, 100] {
                    let style = Appearance {
                        number_weight: weight,
                        number_size: size,
                        symbol_percent: symbols,
                        ..Default::default()
                    };
                    let saved = style.clone();
                    for scale in [1.0_f32, 1.5, 2.0] {
                        for remaining in [None, Some(0.0), Some(68.0), Some(100.0)] {
                            for estimated in [false, true] {
                                let mut quota = WeeklyQuota::from_providers(&[]);
                                quota.remaining = remaining;
                                quota.estimated = estimated;
                                let available = 48.0 * scale - 2.0 * scale.ceil();
                                let lines = block(&quota, &style, scale, available);
                                let digits = bounds(&lines.digits);
                                let label = bounds(&lines.label);
                                assert_eq!(
                                    lines.label.iter().map(|g| g.character).collect::<String>(),
                                    "Codex · 7d"
                                );
                                assert!(label.height() > 0.0 && label.height() <= 9.0 * scale);
                                assert!(lines.height <= available);
                                assert!(
                                    (lines.digits_x + (digits.left + digits.right) / 2.0
                                        - lines.label_x
                                        - (label.left + label.right) / 2.0)
                                        .abs()
                                        < 0.01
                                );
                                assert!(
                                    lines.label_baseline + label.top
                                        - lines.digits_baseline
                                        - digits.bottom
                                        >= 2.0 * scale - 0.01
                                );
                                assert!(lines.digits_x + digits.right <= width(&style, scale));
                                assert!(lines.label_x + label.right <= width(&style, scale));
                                if size <= 20 {
                                    assert_eq!(
                                        lines.digits[0].size,
                                        layout(&quota, size as f32 * scale, &style)[0].size
                                    );
                                }
                                assert_eq!(style, saved);
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn suffix_and_units_stay_fixed_and_symbols_are_smaller() {
        for scale in [1.0, 1.5, 2.0] {
            let h = 20.0 * scale;
            let mut suffix = None;
            for used in [1, 28, 32, 42] {
                let q = WeeklyQuota::from_providers(&[super::super::weekly::tests::data(used)]);
                let glyphs = layout(&q, h, &Appearance::default());
                let last = glyphs.last().unwrap();
                if let Some(x) = suffix {
                    assert_eq!(x, last.x);
                } else {
                    suffix = Some(last.x);
                }
                assert!(last.metrics.height as f32 <= h * 0.7);
                assert!(last.x + last.metrics.width as f32 + h * 0.6 < 100.0 * scale);
            }
        }
    }
}
