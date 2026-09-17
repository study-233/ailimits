pub mod extras;
pub mod history;
pub mod identity;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    OnPace,
    AbovePace,
    Unknown,
}
pub fn time_remaining(
    reset: Option<DateTime<Utc>>,
    seconds: Option<u64>,
    now: DateTime<Utc>,
) -> Option<f32> {
    let seconds = seconds.filter(|s| *s > 0)?;
    Some(
        ((reset?.signed_duration_since(now).num_seconds() as f64 / seconds as f64) * 100.)
            .clamp(0., 100.) as f32,
    )
}
pub fn pace(remaining: Option<f32>, time: Option<f32>, live: bool) -> Pace {
    match (remaining, time, live) {
        (Some(q), Some(t), true) if q.is_finite() && t.is_finite() => {
            if q < t {
                Pace::AbovePace
            } else {
                Pace::OnPace
            }
        }
        _ => Pace::Unknown,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn time_and_pace() {
        let now = Utc::now();
        assert_eq!(
            time_remaining(Some(now + chrono::Duration::seconds(50)), Some(100), now),
            Some(50.)
        );
        assert_eq!(
            time_remaining(Some(now - chrono::Duration::seconds(50)), Some(100), now),
            Some(0.)
        );
        assert_eq!(
            time_remaining(Some(now + chrono::Duration::seconds(200)), Some(100), now),
            Some(100.)
        );
        assert_eq!(time_remaining(None, Some(100), now), None);
        assert_eq!(time_remaining(Some(now), Some(0), now), None);
        assert_eq!(pace(Some(40.), Some(50.), true), Pace::AbovePace);
        assert_eq!(pace(Some(50.), Some(50.), true), Pace::OnPace);
        assert_eq!(pace(Some(40.), Some(50.), false), Pace::Unknown);
        assert_eq!(pace(Some(40.), None, true), Pace::Unknown);
    }
}
