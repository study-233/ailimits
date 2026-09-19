//! Live observations drive independent cooldowns for each Codex quota window.
use crate::{
    config::schema::NotificationConfig,
    providers::{MetricWindow, ProviderData, ProviderId, ProviderStatus},
};
use chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    Headline,
    FiveHours,
    Weekly,
}
impl Scope {
    pub fn window_id(self) -> Option<&'static str> {
        match self {
            Self::Headline => None,
            Self::FiveHours => Some("5h"),
            Self::Weekly => Some("7d"),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Headline => "Limit",
            Self::FiveHours => "5-hour quota",
            Self::Weekly => "Weekly quota",
        }
    }
}
pub struct Alert {
    pub scope: Scope,
    pub percent: f32,
    pub reset_at: Option<DateTime<Utc>>,
    pub threshold: bool,
    pub reset: bool,
}
#[derive(Default)]
struct State {
    percent: Option<f32>,
    reset_at: Option<DateTime<Utc>>,
    notified: Option<Instant>,
}
#[derive(Default)]
pub struct AlertTracker {
    accounts: HashMap<ProviderId, Option<String>>,
    states: HashMap<(ProviderId, Scope), State>,
}
impl AlertTracker {
    pub fn observe(
        &mut self,
        data: &ProviderData,
        config: &NotificationConfig,
        fallback: u8,
        now: Instant,
    ) -> Vec<Alert> {
        if self.accounts.get(&data.id) != Some(&data.account_key) {
            self.states.retain(|(id, _), _| *id != data.id);
            self.accounts
                .insert(data.id.clone(), data.account_key.clone());
        }
        if !matches!(data.status, ProviderStatus::Ok)
            || data.received_at.is_none()
            || data.stale_age_secs().is_some()
        {
            return Vec::new();
        }
        let metrics: Vec<_> = if data.id == ProviderId::Codex {
            [MetricWindow::Session, MetricWindow::Long]
                .into_iter()
                .filter_map(|w| {
                    data.metrics.iter().find(|m| m.window == w).map(|m| {
                        (
                            if w == MetricWindow::Session {
                                Scope::FiveHours
                            } else {
                                Scope::Weekly
                            },
                            m,
                            config.codex_threshold(w, fallback),
                        )
                    })
                })
                .collect()
        } else {
            data.headline_metric()
                .map(|m| (Scope::Headline, m, fallback.min(100)))
                .into_iter()
                .collect()
        };
        let mut alerts = Vec::new();
        for (scope, metric, threshold) in metrics {
            if !metric.limit.is_some_and(|n| n > 0)
                || metric.observed_at.is_some_and(|at| at != data.updated_at)
            {
                continue;
            }
            let Some(percent) = metric.percentage() else {
                continue;
            };
            let state = self.states.entry((data.id.clone(), scope)).or_default();
            let reset = state
                .percent
                .is_some_and(|previous| previous >= threshold as f32 && percent + 30. < previous)
                && (scope == Scope::Headline
                    || (metric.reset_at.is_some() && state.reset_at != metric.reset_at));
            if reset {
                state.notified = None;
            }
            let cooled = state.notified.is_none_or(|at| {
                now.saturating_duration_since(at)
                    >= Duration::from_secs(config.cooldown_minutes.max(1) as u64 * 60)
            });
            let crossed = percent >= threshold as f32 && cooled;
            if crossed {
                state.notified = Some(now);
            }
            state.percent = Some(percent);
            state.reset_at = metric.reset_at;
            if crossed || reset {
                alerts.push(Alert {
                    scope,
                    percent,
                    reset_at: metric.reset_at.filter(|t| *t > Utc::now()),
                    threshold: crossed,
                    reset,
                });
            }
        }
        alerts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn data(session: u64, weekly: u64) -> ProviderData {
        let now = Utc::now();
        let mut metrics = crate::providers::codex::parse_wham_usage(&format!(r#"{{"rate_limit":{{"primary_window":{{"used_percent":{session}}},"secondary_window":{{"used_percent":{weekly}}}}}}}"#)).unwrap();
        for metric in &mut metrics {
            metric.observed_at = Some(now);
            metric.reset_at = Some(now + chrono::Duration::days(1));
        }
        ProviderData {
            id: ProviderId::Codex,
            account_key: Some("a".into()),
            plan_type: None,
            status: ProviderStatus::Ok,
            metrics,
            updated_at: now,
            received_at: Some(Instant::now()),
        }
    }
    #[test]
    fn weekly_alert_is_independent_of_session_and_its_cooldown() {
        let mut tracker = AlertTracker::default();
        let config = NotificationConfig::default();
        let now = Instant::now();
        let alerts = tracker.observe(&data(10, 95), &config, 80, now);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].scope, Scope::Weekly);
        let alerts = tracker.observe(&data(90, 96), &config, 80, now + Duration::from_secs(1));
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].scope, Scope::FiveHours);
        assert!(tracker
            .observe(&data(90, 96), &config, 80, now + Duration::from_secs(2))
            .is_empty());
    }
    #[test]
    fn thresholds_accounts_and_nonlive_data_are_isolated() {
        let mut tracker = AlertTracker::default();
        let config = NotificationConfig {
            codex_session_threshold: Some(95),
            codex_weekly_threshold: Some(70),
            ..Default::default()
        };
        let now = Instant::now();
        let mut sample = data(90, 75);
        assert_eq!(
            tracker.observe(&sample, &config, 80, now)[0].scope,
            Scope::Weekly
        );
        sample.account_key = Some("b".into());
        let alerts = tracker.observe(&sample, &config, 80, now);
        assert_eq!(alerts.len(), 1);
        assert!(!alerts[0].reset);
        for status in [
            ProviderStatus::Estimated,
            ProviderStatus::NetworkError("offline".into()),
        ] {
            sample.status = status;
            assert!(tracker
                .observe(&sample, &config, 80, now + Duration::from_secs(3600))
                .is_empty());
        }
        sample.status = ProviderStatus::Ok;
        sample.received_at = None;
        assert!(tracker
            .observe(&sample, &config, 80, now + Duration::from_secs(3600))
            .is_empty());
    }

    #[test]
    fn reset_requires_a_new_cycle_and_cached_metrics_never_alert() {
        let config = NotificationConfig::default();
        let now = Instant::now();
        let mut tracker = AlertTracker::default();
        let mut sample = data(90, 10);
        assert_eq!(tracker.observe(&sample, &config, 80, now).len(), 1);
        sample.metrics[0].used = 5;
        // A correction inside the same cycle is not a reset.
        assert!(tracker.observe(&sample, &config, 80, now).is_empty());
        sample.metrics[0].used = 90;
        tracker.observe(&sample, &config, 80, now);
        sample.metrics[0].used = 5;
        sample.metrics[0].reset_at = Some(Utc::now() + chrono::Duration::days(2));
        let alerts = tracker.observe(&sample, &config, 80, now);
        assert_eq!(alerts.len(), 1);
        assert!(alerts[0].reset);
        assert!(!alerts[0].threshold);
        sample.metrics[0].used = 90;
        sample.metrics[0].observed_at = Some(sample.updated_at - chrono::Duration::minutes(10));
        assert!(tracker.observe(&sample, &config, 80, now).is_empty());
    }
}
