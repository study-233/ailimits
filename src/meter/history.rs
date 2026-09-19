//! Real observations only. A missing date/sample is not a zero.
use crate::providers::{MetricWindow, ProviderData, ProviderId, ProviderStatus};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub at: DateTime<Utc>,
    pub remaining: f32,
    pub reset: Option<DateTime<Utc>>,
    pub seconds: Option<u64>,
    pub window: MetricWindow,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AccountHistory {
    pub quota: Vec<Observation>,
    pub tokens: BTreeMap<NaiveDate, u64>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    #[serde(default)]
    pub accounts: BTreeMap<String, AccountHistory>,
    #[serde(default)]
    compacted: Option<NaiveDate>,
}
impl History {
    pub fn record(&mut self, data: &ProviderData) -> bool {
        if data.id != ProviderId::Codex
            || !matches!(data.status, ProviderStatus::Ok)
            || data.received_at.is_none()
        {
            return false;
        }
        let Some(key) = data.account_key.as_ref() else {
            return false;
        };
        let account = self.accounts.entry(key.clone()).or_default();
        let mut changed = false;
        for m in &data.metrics {
            if !m.limit.is_some_and(|n| n > 0)
                || m.observed_at.is_some_and(|at| at != data.updated_at)
            {
                continue;
            }
            let Some(pct) = m.percentage() else {
                continue;
            };
            let point = Observation {
                at: data.updated_at,
                remaining: 100. - pct,
                reset: m.reset_at,
                seconds: m.window_seconds,
                window: m.window,
            };
            let last = account.quota.iter().rev().find(|p| p.window == m.window);
            if last.is_some_and(|p| {
                point.at <= p.at
                    || (p.remaining == point.remaining
                        && p.reset == point.reset
                        && p.seconds == point.seconds
                        && point.at - p.at < Duration::minutes(15))
            }) {
                continue;
            }
            account.quota.push(point);
            changed = true;
        }
        changed | self.compact(data.updated_at)
    }
    pub fn update_tokens(
        &mut self,
        key: &str,
        buckets: &BTreeMap<NaiveDate, u64>,
        now: DateTime<Utc>,
    ) -> bool {
        let account = self.accounts.entry(key.to_owned()).or_default();
        let mut changed = false;
        for (&date, &tokens) in buckets {
            if date >= now.date_naive() - Duration::days(29)
                && date <= now.date_naive()
                && account.tokens.get(&date) != Some(&tokens)
            {
                account.tokens.insert(date, tokens);
                changed = true;
            }
        }
        changed | self.compact(now)
    }
    pub fn compact(&mut self, now: DateTime<Utc>) -> bool {
        if self.compacted == Some(now.date_naive()) {
            return false;
        }
        self.compacted = Some(now.date_naive());
        for h in self.accounts.values_mut() {
            h.quota
                .retain(|p| p.at >= now - Duration::days(30) && p.at <= now + Duration::minutes(5));
            h.quota.sort_by_key(|p| p.at);
            h.tokens.retain(|d, _| {
                *d >= now.date_naive() - Duration::days(29) && *d <= now.date_naive()
            });
        }
        self.accounts
            .retain(|_, a| !a.quota.is_empty() || !a.tokens.is_empty());
        true
    }
    pub fn account(&self, key: Option<&str>) -> AccountHistory {
        key.and_then(|k| self.accounts.get(k))
            .cloned()
            .unwrap_or_default()
    }
}
pub fn connects(a: &Observation, b: &Observation, interval: u64) -> bool {
    a.window == b.window
        && a.reset.is_some()
        && a.reset == b.reset
        && b.at > a.at
        && (b.at - a.at).num_seconds()
            <= interval.saturating_mul(2).max(1800).min(i64::MAX as u64) as i64
}
pub fn daily_values(
    h: &AccountHistory,
    end: NaiveDate,
    days: u32,
) -> Vec<(NaiveDate, Option<u64>)> {
    (0..days)
        .rev()
        .map(|ago| {
            let date = end - Duration::days(ago as i64);
            (date, h.tokens.get(&date).copied())
        })
        .collect()
}
pub fn path() -> std::path::PathBuf {
    crate::config::storage::config_path().with_file_name("quota-history.json")
}
pub async fn load() -> History {
    let mut h = match tokio::fs::metadata(path()).await {
        Ok(m) if m.len() <= 32 * 1024 * 1024 => tokio::fs::read(path())
            .await
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default(),
        _ => History::default(),
    };
    h.compact(Utc::now());
    h
}
/// One writer, newest snapshot wins; its owner flushes the final history on exit.
pub fn saver(runtime: &tokio::runtime::Runtime) -> crate::config::storage::SnapshotSaver<History> {
    saver_to(runtime.handle(), path())
}

/// Explicit destination keeps persistence tests isolated from the user's history.
pub fn saver_to(
    handle: &tokio::runtime::Handle,
    path: std::path::PathBuf,
) -> crate::config::storage::SnapshotSaver<History> {
    crate::config::storage::spawn_snapshot_saver(handle, "history", move |history| {
        let path = path.clone();
        async move {
            let bytes = serde_json::to_vec(&history)?;
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            crate::config::storage::atomic_write(&path, &bytes).await?;
            Ok(())
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn data(at: DateTime<Utc>, key: &str, used: u64) -> ProviderData {
        let mut d=ProviderData {account_key:Some(key.into()),plan_type:None,id:ProviderId::Codex,status:ProviderStatus::Ok,metrics:crate::providers::codex::parse_wham_usage(r#"{"rate_limit":{"secondary_window":{"used_percent":20,"limit_window_seconds":604800,"reset_at":2000000000}}}"#).unwrap(),updated_at:at,received_at:Some(std::time::Instant::now())};
        d.metrics[0].used = used;
        d
    }
    #[test]
    fn dedup_anchor_account_and_upsert() {
        let now = Utc::now();
        let mut h = History::default();
        assert!(h.record(&data(now, "a", 20)));
        assert!(!h.record(&data(now + Duration::minutes(1), "a", 20)));
        assert!(h.record(&data(now + Duration::minutes(15), "a", 20)));
        assert!(h.record(&data(now + Duration::minutes(16), "a", 21)));
        h.record(&data(now, "b", 90));
        assert_eq!(h.account(Some("a")).quota.len(), 3);
        assert_eq!(h.account(Some("b")).quota[0].remaining, 10.);
        let date = now.date_naive();
        h.update_tokens("a", &BTreeMap::from([(date, 10)]), now);
        h.update_tokens("a", &BTreeMap::from([(date, 20)]), now);
        assert_eq!(
            daily_values(&h.account(Some("a")), date, 2),
            vec![(date - Duration::days(1), None), (date, Some(20))]
        );
        h.update_tokens("a", &BTreeMap::from([(date, 0)]), now);
        assert_eq!(h.account(Some("a")).tokens[&date], 0);
    }
    #[test]
    fn gaps_cycles_and_retention() {
        let now = Utc::now();
        let mut h = History::default();
        h.record(&data(now, "a", 20));
        let a = h.account(Some("a")).quota[0].clone();
        let mut b = a.clone();
        b.at += Duration::minutes(31);
        assert!(!connects(&a, &b, 60));
        assert!(connects(&a, &b, 1800));
        b.at = a.at + Duration::minutes(1);
        b.reset = None;
        assert!(!connects(&a, &b, 60));
        b.reset = a.reset.map(|r| r + Duration::days(7));
        assert!(!connects(&a, &b, 60));
        h.compact(now + Duration::days(31));
        assert!(h.accounts.is_empty());
    }

    #[test]
    fn estimates_disk_cache_and_unknown_limits_do_not_create_observations() {
        let mut h = History::default();
        let mut d = data(Utc::now(), "a", 20);
        d.received_at = None;
        assert!(!h.record(&d));
        d.received_at = Some(std::time::Instant::now());
        d.status = ProviderStatus::Estimated;
        assert!(!h.record(&d));
        d.status = ProviderStatus::Ok;
        d.metrics[0].limit = None;
        h.record(&d);
        assert!(h.account(Some("a")).quota.is_empty());
        d.metrics[0].limit = Some(0);
        h.record(&d);
        assert!(h.account(Some("a")).quota.is_empty());
        d.metrics[0].limit = Some(100);
        d.metrics[0].observed_at = Some(d.updated_at - Duration::minutes(30));
        h.record(&d);
        assert!(h.account(Some("a")).quota.is_empty());
        d.metrics[0].observed_at = Some(d.updated_at);
        h.record(&d);
        assert_eq!(h.account(Some("a")).quota.len(), 1);
    }
}
