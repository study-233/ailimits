//! Display-only Codex weekly remaining quota. Never changes provider metrics.
use crate::i18n::t;
use crate::providers::{MetricWindow, ProviderData, ProviderId, ProviderStatus};
use chrono::{DateTime, Local, Utc};
use tiny_skia::{LineCap, Paint, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WeeklyQuota {
    pub remaining: Option<f32>,
    pub stale: bool,
    pub estimated: bool,
    pub reset_at: Option<DateTime<Utc>>,
    pub status: &'static str,
}

impl WeeklyQuota {
    pub fn from_providers(providers: &[ProviderData]) -> Self {
        let data = providers.iter().find(|d| d.id == ProviderId::Codex);
        let metric = data.and_then(|d| d.metrics.iter().find(|m| m.window == MetricWindow::Long));
        let usable = data
            .is_some_and(|d| matches!(d.status, ProviderStatus::Ok | ProviderStatus::Estimated));
        // Unknown/zero limits must not manufacture a full remaining quota.
        let remaining = metric
            .filter(|m| usable && m.limit.is_some_and(|limit| limit > 0))
            .and_then(|m| m.percentage())
            .map(|p| (100.0 - p).clamp(0.0, 100.0));
        let stale = data.is_some_and(|d| d.stale_age_secs().is_some());
        let estimated = remaining.is_some()
            && data.is_some_and(|d| matches!(d.status, ProviderStatus::Estimated));
        let status = match data.map(|d| &d.status) {
            Some(ProviderStatus::Loading) => "Loading quota",
            Some(ProviderStatus::AuthError(_)) => "Authentication required",
            Some(ProviderStatus::NetworkError(_)) => "Network unavailable",
            None | Some(ProviderStatus::NotConfigured) => "Codex not enabled or configured",
            _ if remaining.is_none() => "Weekly quota unavailable",
            _ if estimated => "Estimated quota",
            _ if stale => "Cached quota (outdated)",
            _ => "Live quota",
        };
        Self {
            remaining,
            stale,
            estimated,
            reset_at: metric.filter(|_| usable).and_then(|m| m.reset_at),
            status,
        }
    }

    pub fn label(&self) -> String {
        match self.remaining {
            Some(p) => format!(
                "{}{} %",
                if self.estimated { "≈" } else { "" },
                p.round() as u32
            ),
            None => "—".to_string(),
        }
    }

    pub fn tooltip(&self) -> String {
        let reset = self
            .reset_at
            .map(|time| {
                format!(
                    "{} {}",
                    t("Weekly reset:"),
                    time.with_timezone(&Local).format("%m-%d %H:%M")
                )
            })
            .unwrap_or_else(|| t("Weekly reset time unavailable").to_string());
        format!(
            "{} {} · {} · {}",
            t("Codex weekly remaining"),
            self.label(),
            t(self.status),
            reset
        )
    }

    pub fn muted(&self) -> bool {
        self.stale || self.estimated || self.remaining.is_none()
    }
}

/// A native-resolution activity-style ring, clockwise from twelve o'clock.
pub(crate) fn paint_styled_ring(
    pm: &mut Pixmap,
    quota: &WeeklyQuota,
    light: bool,
    style: &crate::config::appearance::Appearance,
) {
    use super::tray::{color, ring_fill, RingFill};
    let center = pm.width() as f32 / 2.0;
    let stroke_w = pm.width() as f32 * style.ring_thickness as f32 / style.ring_size as f32;
    let radius = center - stroke_w / 2.0 - 1.0;
    let [r, g, b] = crate::config::appearance::rgb(&style.ring_color).unwrap_or([255, 55, 116]);
    let ink = if quota.muted() {
        if light {
            color(112, 112, 120, 255)
        } else {
            color(158, 158, 168, 255)
        }
    } else {
        color(r, g, b, 255)
    };
    let track = if quota.muted() {
        color(140, 140, 148, 48)
    } else {
        color(r, g, b, if light { 45 } else { 48 })
    };
    let mut paint = Paint::default();
    paint.set_color(track);
    let circle = PathBuilder::from_circle(center, center, radius).unwrap();
    let stroke = Stroke {
        width: stroke_w,
        line_cap: LineCap::Round,
        ..Stroke::default()
    };
    pm.stroke_path(&circle, &paint, &stroke, Transform::identity(), None);
    paint.set_color(ink);
    match ring_fill(quota.remaining.unwrap_or(0.0), radius, stroke_w) {
        RingFill::Empty => {}
        RingFill::Full => pm.stroke_path(&circle, &paint, &stroke, Transform::identity(), None),
        RingFill::Dot => {
            let dot = PathBuilder::from_circle(center, center - radius, stroke_w / 2.0).unwrap();
            pm.fill_path(
                &dot,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        RingFill::Arc { start, end } => {
            let steps = (((end - start) * radius / 0.35).ceil() as usize).max(2);
            let mut path = PathBuilder::new();
            for i in 0..=steps {
                let angle = start + (end - start) * i as f32 / steps as f32;
                let (x, y) = (center + radius * angle.cos(), center + radius * angle.sin());
                if i == 0 {
                    path.move_to(x, y);
                } else {
                    path.line_to(x, y);
                }
            }
            pm.stroke_path(
                &path.finish().unwrap(),
                &paint,
                &stroke,
                Transform::identity(),
                None,
            );
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::providers::{Metric, MetricUnit};

    #[test]
    fn real_weekly_primary_response_is_73_percent_remaining() {
        let mut d = data(0);
        d.metrics = crate::providers::codex::parse_wham_usage(r#"{"rate_limit":{"primary_window":{"used_percent":27,"limit_window_seconds":604800,"reset_at":1789805478},"secondary_window":null}}"#).unwrap();
        let q = WeeklyQuota::from_providers(&[d]);
        assert_eq!(q.remaining, Some(73.0));
        assert_eq!(q.label(), "73 %");
        assert_eq!(q.reset_at.unwrap().timestamp(), 1789805478);
    }

    pub(crate) fn data(used: u64) -> ProviderData {
        ProviderData {
            id: ProviderId::Codex,
            status: ProviderStatus::Ok,
            metrics: vec![
                Metric {
                    label: "Weekly (misleading label)".into(),
                    used: 99,
                    limit: Some(100),
                    unit: MetricUnit::Percent,
                    reset_at: None,
                    window: MetricWindow::Session,
                },
                Metric {
                    label: "Any language".into(),
                    used,
                    limit: Some(100),
                    unit: MetricUnit::Percent,
                    reset_at: Some(Utc::now() + chrono::Duration::days(3)),
                    window: MetricWindow::Long,
                },
            ],
            updated_at: Utc::now(),
            received_at: Some(std::time::Instant::now()),
        }
    }

    #[test]
    fn weekly_only_remaining_and_no_data_mutation() {
        for (used, remaining) in [(0, 100.0), (32, 68.0), (100, 0.0), (120, 0.0)] {
            let d = data(used);
            let before = serde_json::to_string(&d).unwrap();
            let q = WeeklyQuota::from_providers(std::slice::from_ref(&d));
            assert_eq!(q.remaining, Some(remaining));
            assert_eq!(q.reset_at, d.metrics[1].reset_at);
            assert_eq!(before, serde_json::to_string(&d).unwrap());
        }
        let mut d = data(32);
        d.metrics.pop();
        assert_eq!(WeeklyQuota::from_providers(&[d]).remaining, None);
        assert_eq!(WeeklyQuota::from_providers(&[]).label(), "—");
    }

    #[test]
    fn invalid_limits_errors_staleness_and_estimates() {
        for limit in [None, Some(0)] {
            let mut d = data(32);
            d.metrics[1].limit = limit;
            assert_eq!(WeeklyQuota::from_providers(&[d]).remaining, None);
        }
        for status in [
            ProviderStatus::Loading,
            ProviderStatus::AuthError("test".into()),
            ProviderStatus::NetworkError("test".into()),
            ProviderStatus::NotConfigured,
        ] {
            let mut d = data(32);
            d.status = status;
            assert_eq!(WeeklyQuota::from_providers(&[d]).remaining, None);
        }
        let mut d = data(32);
        let fresh = WeeklyQuota::from_providers(std::slice::from_ref(&d));
        d.received_at = None;
        d.updated_at = Utc::now() - chrono::Duration::minutes(10);
        let stale = WeeklyQuota::from_providers(std::slice::from_ref(&d));
        assert!(stale.muted());
        assert_ne!(fresh, stale); // Invalidates the panel cache even if % is unchanged.
        d.status = ProviderStatus::Estimated;
        assert_eq!(WeeklyQuota::from_providers(&[d]).label(), "≈68 %");
    }
}
