//! Shared selection, geometry and rendering for panel, tray and preferences.
use super::{
    numbers,
    tray::color,
    weekly::{paint_styled_ring, WeeklyQuota},
};
use crate::config::appearance::{rgb, Appearance, DisplayStyle, QuotaPeriods};
use crate::providers::{MetricWindow, ProviderData};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, PixmapPaint, Transform};

pub(crate) fn selected(providers: &[ProviderData], style: &Appearance) -> Vec<WeeklyQuota> {
    let windows: &[MetricWindow] = match style.periods {
        QuotaPeriods::Weekly => &[MetricWindow::Long],
        QuotaPeriods::FiveHours => &[MetricWindow::Session],
        QuotaPeriods::Both => &[MetricWindow::Session, MetricWindow::Long],
    };
    windows
        .iter()
        .map(|&w| WeeklyQuota::for_window(providers, w))
        .collect()
}

/// The panel and tray deliberately use the same content, order and line breaks.
/// Pro's weekly-only tooltip does not change the indicator's selected metrics.
pub(crate) fn hover_tooltip(providers: &[ProviderData], style: &Appearance) -> String {
    use crate::i18n::t;
    let now = chrono::Utc::now();
    let mut lines = vec!["Codex".to_string()];
    let quotas = if providers.iter().any(ProviderData::is_codex_pro) {
        vec![WeeklyQuota::for_window(providers, MetricWindow::Long)]
    } else {
        selected(providers, style)
    };
    for q in quotas {
        let name = if q.window == MetricWindow::Session {
            "5h"
        } else {
            "Weekly"
        };
        let reset = q
            .reset_at
            .map(|time| {
                let minutes = (time - now).num_minutes().max(0);
                let duration = if minutes >= 1440 {
                    format!("{}d {}h", minutes / 1440, minutes % 1440 / 60)
                } else if minutes >= 60 {
                    format!("{}h {}m", minutes / 60, minutes % 60)
                } else {
                    format!("{minutes}m")
                };
                format!("{} {duration}", t("Reset in"))
            })
            .unwrap_or_else(|| t("Reset time unavailable").into());
        lines.push(format!(
            "{}  {} · {}",
            t(name),
            q.label(),
            if q.muted() {
                t(q.status).to_string()
            } else {
                reset
            }
        ));
    }
    if let Some(data) = providers
        .iter()
        .find(|p| p.id == crate::providers::ProviderId::Codex)
    {
        let age = (now - data.updated_at).num_minutes().max(0);
        lines.push(if age == 0 {
            t("Updated just now").into()
        } else {
            format!("{} {age}m", t("Updated"))
        });
    }
    lines.join("\n")
}

/// Dedicated notification-area artwork. Never scales the taskbar composition.
pub(crate) fn render_tray_icon(
    side: u32,
    quotas: &[WeeklyQuota],
    light: bool,
    style: &Appearance,
    warning_remaining: f32,
) -> Pixmap {
    use crate::config::appearance::TrayDisplay;
    let Some(q) = quotas.iter().min_by(|a, b| {
        a.remaining
            .unwrap_or(101.)
            .total_cmp(&b.remaining.unwrap_or(101.))
    }) else {
        return Pixmap::new(side, side).unwrap();
    };
    if style.tray_display == TrayDisplay::Number {
        let mut pm = Pixmap::new(side, side).unwrap();
        let label = q
            .remaining
            .map(|v| format!("{:02}", v.round() as u32))
            .unwrap_or_else(|| "—".into());
        numbers::draw_compact(
            &mut pm,
            &label,
            number_ink(q, light, style, warning_remaining),
            style.number_weight,
        );
        return pm;
    }
    // Adapt only geometry to the icon canvas. Shape, period colors, line weight
    // and both selected windows come from the same renderer as the panel.
    let mut pm = render_icon(side, quotas, light, style);
    let low = !q.muted() && q.remaining.is_some_and(|v| v <= warning_remaining);
    if matches!(
        style.tray_display,
        TrayDisplay::Auto | TrayDisplay::RingState
    ) && (low || q.muted())
    {
        let mut paint = Paint::default();
        let state_ink = number_ink(
            q,
            light,
            &Appearance {
                number_color: "auto".into(),
                ..style.clone()
            },
            warning_remaining,
        );
        paint.set_color(state_ink);
        let radius = (side as f32 / 11.).max(1.);
        // Reserve a corner for the state marker so neither quota is covered.
        let dot =
            PathBuilder::from_circle(side as f32 - radius - 1., side as f32 - radius - 1., radius)
                .unwrap();
        pm.fill_path(
            &dot,
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    pm
}

fn period_style(style: &Appearance, q: &WeeklyQuota) -> Appearance {
    let mut s = style.clone();
    if q.window == MetricWindow::Session {
        s.ring_color = s.session_color.clone();
    }
    s
}

fn ink(q: &WeeklyQuota, light: bool, custom: &str) -> Color {
    if q.muted() {
        let c = if light { 112 } else { 158 };
        color(c, c, c, 255)
    } else if let Some([r, g, b]) = rgb(custom) {
        color(r, g, b, 255)
    } else {
        let c = if light { 36 } else { 236 };
        color(c, c, c, 255)
    }
}

fn number_ink(q: &WeeklyQuota, light: bool, style: &Appearance, warning_remaining: f32) -> Color {
    let theme = super::theme::SurfaceTheme::new(light);
    if style.number_color == "auto"
        && !q.muted()
        && q.remaining.is_some_and(|p| p <= warning_remaining)
    {
        if q.remaining.is_some_and(|p| p <= warning_remaining / 2.) {
            theme.danger.to_skia_color()
        } else {
            theme.warning.to_skia_color()
        }
    } else {
        ink(q, light, &style.number_color)
    }
}

fn row_style(style: &Appearance) -> Appearance {
    let mut s = style.clone();
    s.number_size = s.number_size.min(if s.periods == QuotaPeriods::Both {
        14
    } else {
        28
    });
    s
}

fn bar_left(style: &Appearance) -> f32 {
    (style.bar_x as f32)
        .max(style.bar_number_x as f32 + numbers::width(&row_style(style), 1.0) + 26.0)
}

pub(crate) fn width(style: &Appearance) -> f32 {
    match style.display_style {
        DisplayStyle::Bars => bar_left(style) + style.bar_width as f32 + 8.0,
        DisplayStyle::Rings if style.periods == QuotaPeriods::Both => {
            (style.ring_x as f32 + style.ring_size as f32)
                .max(style.number_x as f32 + numbers::width(&row_style(style), 1.0) + 24.0)
                + 8.0
        }
        DisplayStyle::Rings => super::taskbar_panel::single_ring_width(style),
    }
}

fn rings(side: u32, quotas: &[WeeklyQuota], light: bool, style: &Appearance) -> Pixmap {
    let mut pm = Pixmap::new(side, side).expect("ring size");
    for (i, q) in quotas.iter().enumerate() {
        let mut s = period_style(style, q);
        if quotas.len() == 2 {
            // Couple thickness and spacing so even extreme saved styles leave a visible gap.
            let stroke = (side as f32 * s.ring_thickness as f32 / s.ring_size as f32)
                .clamp(1.0, side as f32 / 9.0);
            let inset = if i == 0 {
                0
            } else {
                (stroke + (side as f32 / 16.0).max(1.5)).ceil() as u32
            };
            let inner_side = side.saturating_sub(2 * inset).max(2);
            s.ring_size = inner_side as i32;
            s.ring_thickness = stroke.round().max(1.0) as i32;
            let mut ring = Pixmap::new(inner_side, inner_side).unwrap();
            paint_styled_ring(&mut ring, q, light, &s);
            pm.draw_pixmap(
                inset as i32,
                inset as i32,
                ring.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        } else {
            paint_styled_ring(&mut pm, q, light, &s);
        }
    }
    pm
}

fn capsule(pm: &mut Pixmap, rect: (f32, f32, f32, f32), ink: Color) {
    let (x, y, w, h) = rect;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let radius = h.min(w) / 2.0;
    let mut paint = Paint::default();
    paint.set_color(ink);
    // A closed rounded rectangle also handles a fill narrower than its height.
    let mut p = PathBuilder::new();
    let k = 0.552_284_8;
    p.move_to(x + radius, y);
    p.line_to(x + w - radius, y);
    p.cubic_to(
        x + w - radius + k * radius,
        y,
        x + w,
        y + radius - k * radius,
        x + w,
        y + radius,
    );
    p.line_to(x + w, y + h - radius);
    p.cubic_to(
        x + w,
        y + h - radius + k * radius,
        x + w - radius + k * radius,
        y + h,
        x + w - radius,
        y + h,
    );
    p.line_to(x + radius, y + h);
    p.cubic_to(
        x + radius - k * radius,
        y + h,
        x,
        y + h - radius + k * radius,
        x,
        y + h - radius,
    );
    p.line_to(x, y + radius);
    p.cubic_to(
        x,
        y + radius - k * radius,
        x + radius - k * radius,
        y,
        x + radius,
        y,
    );
    p.close();
    if let Some(path) = p.finish() {
        pm.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn bar(
    pm: &mut Pixmap,
    q: &WeeklyQuota,
    rect: (f32, f32, f32, f32),
    light: bool,
    style: &Appearance,
) {
    let s = period_style(style, q);
    let fill = ink(q, light, &s.ring_color);
    let mut track = fill;
    track.set_alpha(if light { 0.16 } else { 0.20 });
    capsule(pm, rect, track);
    if let Some(remaining) = q.remaining {
        capsule(
            pm,
            (
                rect.0,
                rect.1,
                rect.2 * remaining.clamp(0.0, 100.0) / 100.0,
                rect.3,
            ),
            fill,
        );
    }
}

pub(crate) fn render(
    w: u32,
    h: u32,
    quotas: &[WeeklyQuota],
    light: bool,
    style: &Appearance,
) -> Pixmap {
    render_with_threshold(w, h, quotas, light, style, 20.0)
}

pub(crate) fn render_with_threshold(
    w: u32,
    h: u32,
    quotas: &[WeeklyQuota],
    light: bool,
    style: &Appearance,
    warning_remaining: f32,
) -> Pixmap {
    if style.display_style == DisplayStyle::Rings && quotas.len() == 1 {
        return super::taskbar_panel::render_styled_panel(w, h, &quotas[0], light, &{
            let mut s = period_style(style, &quotas[0]);
            if style.number_color == "auto" {
                let c = number_ink(&quotas[0], light, style, warning_remaining);
                s.number_color = format!(
                    "#{:02X}{:02X}{:02X}",
                    (c.red() * 255.) as u8,
                    (c.green() * 255.) as u8,
                    (c.blue() * 255.) as u8
                );
            }
            s
        });
    }
    let mut pm = Pixmap::new(w, h).expect("panel size");
    pm.fill(color(0, 0, 0, 1));
    let scale = (h as f32 / 48.0).clamp(1.0, 3.0);
    if style.display_style == DisplayStyle::Rings {
        let side = (style.ring_size as f32 * scale).round() as u32;
        let graphic = rings(side, quotas, light, style);
        let y = ((h as i32 - side as i32) / 2 + (style.ring_y as f32 * scale).round() as i32)
            .clamp(0, (h as i32 - side as i32).max(0));
        pm.draw_pixmap(
            (style.ring_x as f32 * scale).round() as i32,
            y,
            graphic.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }
    let row = row_style(style);
    let row_h = h as f32 / quotas.len().max(1) as f32;
    for (i, q) in quotas.iter().enumerate() {
        let center = row_h * (i as f32 + 0.5);
        let text_h = row.number_size as f32 * scale;
        let y = (center - text_h / 2.0 + style.number_y as f32 * scale).clamp(
            row_h * i as f32 + scale,
            (row_h * (i + 1) as f32 - text_h - scale).max(row_h * i as f32 + scale),
        );
        let x = if style.display_style == DisplayStyle::Bars {
            style.bar_number_x
        } else {
            style.number_x
        };
        let s = period_style(style, q);
        numbers::draw_row(
            &mut pm,
            q,
            (x as f32 * scale, y),
            scale,
            (
                number_ink(q, light, style, warning_remaining),
                ink(q, light, &s.ring_color),
            ),
            &row,
        );
        if style.display_style == DisplayStyle::Bars {
            let thickness = (style.bar_thickness as f32 * scale).min(row_h - 4.0 * scale);
            let by = (center - thickness / 2.0 + style.bar_y as f32 * scale).clamp(
                row_h * i as f32 + scale,
                row_h * (i + 1) as f32 - thickness - scale,
            );
            bar(
                &mut pm,
                q,
                (
                    bar_left(style) * scale,
                    by,
                    style.bar_width as f32 * scale,
                    thickness,
                ),
                light,
                style,
            );
        }
    }
    pm
}

pub(crate) fn render_icon(
    side: u32,
    quotas: &[WeeklyQuota],
    light: bool,
    style: &Appearance,
) -> Pixmap {
    if style.display_style == DisplayStyle::Rings {
        return rings(side, quotas, light, style);
    }
    let mut pm = Pixmap::new(side, side).expect("icon size");
    let rows = quotas.len().max(1) as f32;
    for (i, q) in quotas.iter().enumerate() {
        let thickness = (style.bar_thickness as f32 * side as f32 / 48.)
            .clamp(1., (side as f32 / rows - 4.).max(1.));
        let center = side as f32 * (i as f32 + 0.5) / rows;
        bar(
            &mut pm,
            q,
            (2.0, center - thickness / 2.0, side as f32 - 4.0, thickness),
            light,
            style,
        );
    }
    pm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pro_and_prolite_tooltips_keep_only_weekly_including_cached_snapshots() {
        use crate::providers::codex::parse_wham_snapshot;
        for plan in ["pro", "prolite"] {
            // Actual response shape: weekly is primary, no session window.
            let body = format!(
                r#"{{"plan_type":"{plan}","rate_limit":{{"primary_window":{{"limit_window_seconds":604800,"used_percent":60}},"secondary_window":null}}}}"#
            );
            let (metrics, plan_type) = parse_wham_snapshot(&body).unwrap();
            let mut live = super::super::weekly::tests::data(0);
            live.metrics = metrics;
            live.plan_type = plan_type;
            let cached: ProviderData =
                serde_json::from_str(&serde_json::to_string(&live).unwrap()).unwrap();
            for data in [live, cached.aged_for_display()] {
                for periods in [
                    QuotaPeriods::Weekly,
                    QuotaPeriods::FiveHours,
                    QuotaPeriods::Both,
                ] {
                    let style = Appearance {
                        periods,
                        ..Default::default()
                    };
                    let providers = [data.clone()];
                    let before = selected(&providers, &style);
                    let tip = hover_tooltip(&providers, &style);
                    assert_eq!(tip.lines().count(), 3);
                    assert!(tip.contains("40 %"));
                    assert!(
                        !tip.contains("5h") && !tip.contains("5-hour") && !tip.contains("5 小时")
                    );
                    assert_eq!(super::super::tray::tooltip(&providers, &style), tip);
                    assert_eq!(selected(&providers, &style), before);
                }
                let mut missing = data;
                missing.metrics.clear();
                let tip = hover_tooltip(&[missing], &Appearance::default());
                assert!(tip.contains('—'));
                assert!(!tip.contains("5h"));
            }
        }
    }

    #[test]
    fn hover_summary_respects_the_selected_period_and_does_not_guess_a_plan() {
        for plan in [
            None,
            Some("plus"),
            Some("team"),
            Some("unknown"),
            Some("pro_future"),
        ] {
            let mut data = super::super::weekly::tests::data(32);
            data.plan_type = plan.map(str::to_owned);
            for periods in [
                QuotaPeriods::Weekly,
                QuotaPeriods::FiveHours,
                QuotaPeriods::Both,
            ] {
                let style = Appearance {
                    periods,
                    ..Default::default()
                };
                let tip = hover_tooltip(std::slice::from_ref(&data), &style);
                assert_eq!(
                    super::super::tray::tooltip(std::slice::from_ref(&data), &style),
                    tip
                );
                assert_eq!(tip.contains("68 %"), periods != QuotaPeriods::FiveHours);
                assert_eq!(tip.contains("1 %"), periods != QuotaPeriods::Weekly);
                assert_eq!(
                    tip.lines().count(),
                    if periods == QuotaPeriods::Both { 4 } else { 3 }
                );
                assert!(tip.encode_utf16().count() <= 127);
            }
        }
    }

    #[test]
    fn tray_and_panel_keep_period_colors_and_shape_for_all_graphic_modes() {
        use crate::config::appearance::TrayDisplay;
        let blue =
            |p: &tiny_skia::PremultipliedColorU8| p.alpha() > 100 && p.red() < 8 && p.blue() > 100;
        let pink = |p: &tiny_skia::PremultipliedColorU8| {
            p.alpha() > 100 && p.red() > 100 && p.green() < 12
        };
        for display_style in [DisplayStyle::Rings, DisplayStyle::Bars] {
            for periods in [
                QuotaPeriods::Weekly,
                QuotaPeriods::FiveHours,
                QuotaPeriods::Both,
            ] {
                for light in [false, true] {
                    for side in [16, 20, 24, 32] {
                        for mode in [TrayDisplay::Auto, TrayDisplay::Ring, TrayDisplay::RingState] {
                            let style = Appearance {
                                display_style,
                                periods,
                                tray_display: mode,
                                ring_color: "#0080FF".into(),
                                session_color: "#FF0080".into(),
                                ring_thickness: 6,
                                ..Default::default()
                            };
                            let mut data = super::super::weekly::tests::data(60);
                            data.metrics[0].used = 35;
                            let quotas = selected(&[data], &style);
                            let panel =
                                render(width(&style).ceil() as u32, 48, &quotas, light, &style);
                            let tray = render_tray_icon(side, &quotas, light, &style, 20.);
                            for image in [&panel, &tray] {
                                assert_eq!(
                                    image.pixels().iter().any(blue),
                                    periods != QuotaPeriods::FiveHours,
                                    "weekly color missing: {style:?}, side {side}"
                                );
                                assert_eq!(
                                    image.pixels().iter().any(pink),
                                    periods != QuotaPeriods::Weekly,
                                    "session color missing: {style:?}, side {side}"
                                );
                            }
                            let mut other = style.clone();
                            other.display_style = if display_style == DisplayStyle::Rings {
                                DisplayStyle::Bars
                            } else {
                                DisplayStyle::Rings
                            };
                            assert_ne!(
                                tray.data(),
                                render_tray_icon(side, &quotas, light, &other, 20.).data()
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn explicit_tray_numbers_follow_numeric_color_and_weight() {
        use crate::config::appearance::{NumberWeight, TrayDisplay};
        let mut style = Appearance {
            tray_display: TrayDisplay::Number,
            number_color: "#0080FF".into(),
            ..Default::default()
        };
        let quotas = selected(&[super::super::weekly::tests::data(60)], &style);
        let blue = render_tray_icon(32, &quotas, false, &style, 20.);
        assert!(blue
            .pixels()
            .iter()
            .any(|p| p.alpha() > 100 && p.blue() > 100 && p.red() == 0));
        style.number_weight = NumberWeight::Bold;
        assert_ne!(
            blue.data(),
            render_tray_icon(32, &quotas, false, &style, 20.).data()
        );
        style.number_color = "#FF0080".into();
        assert_ne!(
            blue.data(),
            render_tray_icon(32, &quotas, false, &style, 20.).data()
        );
    }

    #[test]
    fn tray_modes_respect_theme_threshold_and_unknown_data_at_native_sizes() {
        use crate::config::appearance::TrayDisplay;
        for side in [16, 20, 24, 32] {
            for light in [true, false] {
                for mode in [
                    TrayDisplay::Auto,
                    TrayDisplay::Ring,
                    TrayDisplay::Number,
                    TrayDisplay::RingState,
                ] {
                    let style = Appearance {
                        tray_display: mode,
                        ..Default::default()
                    };
                    let mut quotas = selected(&[], &style);
                    quotas[0].remaining = Some(18.0);
                    quotas[0].stale = false;
                    let saved = quotas.clone();
                    let normal = render_tray_icon(side, &quotas, light, &style, 10.);
                    let warning = render_tray_icon(side, &quotas, light, &style, 20.);
                    if mode == TrayDisplay::Ring {
                        assert_eq!(
                            normal.data(),
                            warning.data(),
                            "plain graphic keeps its period color"
                        );
                    } else {
                        assert_ne!(
                            normal.data(),
                            warning.data(),
                            "configured threshold changes state/number ink"
                        );
                    }
                    assert_eq!(quotas, saved);
                    for value in [None, Some(0.), Some(7.), Some(51.), Some(100.)] {
                        quotas[0].remaining = value;
                        let image = render_tray_icon(side, &quotas, light, &style, 20.);
                        assert_eq!((image.width(), image.height()), (side, side));
                        assert!(image.pixels().iter().any(|p| p.alpha() > 0));
                    }
                    quotas[0].stale = true;
                    let cached = render_tray_icon(side, &quotas, light, &style, 100.);
                    let not_warned = render_tray_icon(side, &quotas, light, &style, 0.);
                    assert_eq!(
                        cached.data(),
                        not_warned.data(),
                        "stale data must not alarm"
                    );
                }
            }
        }
        let data = super::super::weekly::tests::data(49);
        let tip = hover_tooltip(
            &[data],
            &Appearance {
                periods: QuotaPeriods::Both,
                ..Default::default()
            },
        );
        assert_eq!(tip.lines().count(), 4);
        assert!(tip.contains("51 %"));
        assert!(tip.contains("1 %"));
        assert!(
            tip.encode_utf16().count() <= 127,
            "shell tooltip size limit"
        );
    }

    #[test]
    fn dual_tooltip_uses_separate_lines_and_the_renderer_reserves_both() {
        let style = Appearance {
            periods: QuotaPeriods::Both,
            ..Default::default()
        };
        let text = hover_tooltip(&[super::super::weekly::tests::data(32)], &style);
        assert_eq!(text.lines().count(), 4);
        let single_text = hover_tooltip(
            &[super::super::weekly::tests::data(32)],
            &Appearance::default(),
        );
        let single = super::super::tray::render_tooltip(&single_text, 48.0, false);
        let dual = super::super::tray::render_tooltip(&text, 48.0, false);
        assert!(dual.height() > single.height());
        assert!(dual.width() < single.width() * 2);
    }

    #[test]
    fn selection_uses_windows_not_order_and_never_substitutes() {
        let style = Appearance {
            periods: QuotaPeriods::Both,
            ..Default::default()
        };
        let mut data = super::super::weekly::tests::data(32);
        data.metrics[0].used = 7;
        data.metrics.reverse();
        let q = selected(std::slice::from_ref(&data), &style);
        assert_eq!(
            q.iter().map(|q| q.remaining).collect::<Vec<_>>(),
            vec![Some(93.0), Some(68.0)]
        );
        data.metrics.retain(|m| m.window == MetricWindow::Long);
        let partial = selected(std::slice::from_ref(&data), &style);
        assert_eq!(partial[0].remaining, None);
        assert_eq!(partial[1].remaining, Some(68.0));
        data.metrics.clear();
        assert!(selected(&[data], &style)
            .iter()
            .all(|q| q.remaining.is_none()));
        assert_ne!(q, partial);
    }

    #[test]
    fn all_modes_fit_at_supported_scales_and_keep_source_immutable() {
        for display_style in [DisplayStyle::Rings, DisplayStyle::Bars] {
            for periods in [
                QuotaPeriods::Weekly,
                QuotaPeriods::FiveHours,
                QuotaPeriods::Both,
            ] {
                for light in [false, true] {
                    for scale in [1.0, 1.25, 1.5, 2.0] {
                        let style = Appearance {
                            display_style,
                            periods,
                            ..Default::default()
                        };
                        for remaining in [None, Some(0.0), Some(1.0), Some(99.0), Some(100.0)] {
                            let mut q = selected(&[], &style);
                            for item in &mut q {
                                item.remaining = remaining;
                                item.estimated = true;
                            }
                            let before = q.clone();
                            let w = (width(&style) * scale).ceil() as u32;
                            let h = (48.0 * scale) as u32;
                            let pm = render(w, h, &q, light, &style);
                            assert!(
                                (0..h).all(|y| pm.pixel(w - 1, y).unwrap().alpha() == 1),
                                "right edge {style:?}"
                            );
                            assert!(
                                (0..w).all(|x| pm.pixel(x, h - 1).unwrap().alpha() == 1),
                                "bottom edge {style:?}"
                            );
                            assert_eq!(q, before);
                            let icon = render_icon(32, &q, light, &style);
                            assert!(icon.pixels().iter().any(|p| p.alpha() > 0));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn bar_fill_is_remaining_quota_and_unknown_has_only_a_track() {
        let style = Appearance {
            display_style: DisplayStyle::Bars,
            ..Default::default()
        };
        let mut q = selected(&[], &style);
        q[0].remaining = Some(100.0);
        let full = render_icon(32, &q, false, &style);
        q[0].remaining = Some(0.0);
        let zero = render_icon(32, &q, false, &style);
        q[0].remaining = None;
        let unknown = render_icon(32, &q, false, &style);
        let solid = |pm: &Pixmap| pm.pixels().iter().filter(|p| p.alpha() > 200).count();
        assert!(solid(&full) > 0);
        assert_eq!(solid(&zero), 0);
        assert_eq!(solid(&unknown), 0);
    }

    #[test]
    #[ignore = "exports actual renderer samples for visual review"]
    fn export_quotabar_previews() {
        let dir = std::path::Path::new("target/quotabar-preview");
        std::fs::create_dir_all(dir).unwrap();
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let mut sheet = Pixmap::new((640.0 * scale) as u32, (380.0 * scale) as u32).unwrap();
            sheet.fill(color(245, 245, 247, 255));
            let mut row = 0;
            for display_style in [DisplayStyle::Rings, DisplayStyle::Bars] {
                for periods in [
                    QuotaPeriods::Weekly,
                    QuotaPeriods::FiveHours,
                    QuotaPeriods::Both,
                ] {
                    let style = Appearance {
                        display_style,
                        periods,
                        ..Default::default()
                    };
                    let mut quotas = selected(&[], &style);
                    for q in &mut quotas {
                        q.remaining = Some(if q.window == MetricWindow::Long {
                            68.0
                        } else {
                            93.0
                        });
                    }
                    for (col, light) in [(0, true), (1, false)] {
                        let mut bg =
                            Pixmap::new((308.0 * scale) as u32, (54.0 * scale) as u32).unwrap();
                        bg.fill(if light {
                            color(232, 232, 236, 255)
                        } else {
                            color(28, 28, 32, 255)
                        });
                        let panel = render(
                            (width(&style) * scale).ceil() as u32,
                            (48.0 * scale) as u32,
                            &quotas,
                            light,
                            &style,
                        );
                        bg.draw_pixmap(
                            (8.0 * scale) as i32,
                            (3.0 * scale) as i32,
                            panel.as_ref(),
                            &PixmapPaint::default(),
                            Transform::identity(),
                            None,
                        );
                        sheet.draw_pixmap(
                            ((8 + col * 320) as f32 * scale) as i32,
                            ((8 + row * 62) as f32 * scale) as i32,
                            bg.as_ref(),
                            &PixmapPaint::default(),
                            Transform::identity(),
                            None,
                        );
                    }
                    row += 1;
                }
            }
            sheet
                .save_png(dir.join(format!("styles-{}.png", (scale * 100.0) as u32)))
                .unwrap();
        }
        // Exact-resolution tray samples: modes across columns, icon sizes down rows.
        use crate::config::appearance::TrayDisplay;
        for light in [true, false] {
            let mut sheet = Pixmap::new(360, 240).unwrap();
            sheet.fill(
                super::super::theme::SurfaceTheme::new(light)
                    .background
                    .to_skia_color(),
            );
            for (row, side) in [16, 20, 24, 32].into_iter().enumerate() {
                for (column, mode) in [
                    TrayDisplay::Auto,
                    TrayDisplay::Ring,
                    TrayDisplay::Number,
                    TrayDisplay::RingState,
                ]
                .into_iter()
                .enumerate()
                {
                    for (i, value) in [51., 7.].into_iter().enumerate() {
                        let style = Appearance {
                            tray_display: mode,
                            ..Default::default()
                        };
                        let mut q = selected(&[], &style);
                        q[0].remaining = Some(value);
                        let icon = render_tray_icon(side, &q, light, &style, 20.);
                        sheet.draw_pixmap(
                            (column * 88 + i * 40 + 4) as i32,
                            (row * 56 + 8) as i32,
                            icon.as_ref(),
                            &PixmapPaint::default(),
                            Transform::identity(),
                            None,
                        );
                    }
                }
            }
            sheet
                .save_png(dir.join(format!("tray-{}.png", if light { "light" } else { "dark" })))
                .unwrap();
        }
        // Original vector-based brand icon, reusing the activity-ring geometry.
        let style = Appearance {
            periods: QuotaPeriods::Both,
            ring_size: 44,
            ring_thickness: 4,
            ..Default::default()
        };
        let mut q = selected(&[], &style);
        q[0].remaining = Some(82.0);
        q[1].remaining = Some(62.0);
        let mut icon = Pixmap::new(256, 256).unwrap();
        capsule(&mut icon, (0.0, 0.0, 256.0, 256.0), color(28, 28, 34, 255));
        let graphic = rings(196, &q, false, &style);
        icon.draw_pixmap(
            30,
            30,
            graphic.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        icon.save_png(dir.join("icon.png")).unwrap();

        // Current user color/weight, with both surfaces side by side. These are
        // offscreen production renders; no live windows or repaint are involved.
        for light in [true, false] {
            let mut sheet = Pixmap::new(520, 400).unwrap();
            sheet.fill(
                super::super::theme::SurfaceTheme::new(light)
                    .background
                    .to_skia_color(),
            );
            for (shape_row, display_style) in [DisplayStyle::Rings, DisplayStyle::Bars]
                .into_iter()
                .enumerate()
            {
                for (period_row, periods) in [
                    QuotaPeriods::Weekly,
                    QuotaPeriods::FiveHours,
                    QuotaPeriods::Both,
                ]
                .into_iter()
                .enumerate()
                {
                    let style = Appearance {
                        display_style,
                        periods,
                        ring_color: "#0080FF".into(),
                        ring_thickness: 6,
                        tray_display: TrayDisplay::RingState,
                        ..Default::default()
                    };
                    let mut data = super::super::weekly::tests::data(60);
                    data.metrics[0].used = 35;
                    let quotas = selected(&[data], &style);
                    let y = (shape_row * 3 + period_row) as i32 * 64 + 8;
                    let panel = render(width(&style).ceil() as u32, 48, &quotas, light, &style);
                    sheet.draw_pixmap(
                        8,
                        y,
                        panel.as_ref(),
                        &PixmapPaint::default(),
                        Transform::identity(),
                        None,
                    );
                    for (column, side) in [16, 20, 24, 32].into_iter().enumerate() {
                        let tray = render_tray_icon(side, &quotas, light, &style, 20.);
                        sheet.draw_pixmap(
                            300 + column as i32 * 52,
                            y + (48 - side as i32) / 2,
                            tray.as_ref(),
                            &PixmapPaint::default(),
                            Transform::identity(),
                            None,
                        );
                    }
                }
            }
            sheet
                .save_png(dir.join(format!("synchronized-{light}.png")))
                .unwrap();
            let mut data = super::super::weekly::tests::data(60);
            data.plan_type = Some("prolite".into());
            data.metrics
                .retain(|metric| metric.window == MetricWindow::Long);
            data.metrics[0].reset_at = Some(chrono::Utc::now() + chrono::Duration::hours(51));
            let text = hover_tooltip(&[data], &Appearance::default());
            super::super::tray::render_tooltip(&text, 48., light)
                .save_png(dir.join(format!("synchronized-tooltip-{light}.png")))
                .unwrap();
        }
    }
}
